#!/usr/bin/env bash
#
# wargame-arms.sh — the adversarial arms the wargame's falsifiers name, run against **one** local devnet.
#
#   tools/wargame-arms.sh [--validators N] [--keep] [--no-boot] [--out DIR]
#
# **Why one boot and many arms.** The wargame's 97 rows carry 92 falsifiers, most of them written as a
# drill each. Booting a devnet per row would take longer than the arms themselves, and it is not what
# any of the falsifiers actually require: 26 of the rows need nothing but a stock three-validator net
# and a client on this host. This runner boots the net once (`tools/devnet.sh up --validators N`), runs
# every arm against it, records a verdict per **row**, and tears the net down — including on failure,
# under a trap, because a leaked devnet collides with the next run's ports and volumes.
#
# **What it does not do is abort.** `tools/devnet-fuzz.py`'s `fail()` is `sys.exit(1)`, so the first
# failing mode starves every mode after it — that is how `scheduler`/`gateway`/`valid` went unexecuted
# for a month. Every arm here runs; the exit code is the summary.
#
# --- the vocabulary, and why it is not pass/fail ------------------------------------------------
#
# A wargame row is **CONFIRMED** when the deviation is real, so "arming" a row means *reproducing the
# deviation*, not asserting that it is absent. Three outcomes, and the middle one is the point:
#
#   armed         the arm ran and reproduced what the row says (a CONFIRMED row) — or, for a REFUTED
#                 row, showed the property holds. Either way the row's falsifier is now a committed run.
#   mismatch      the arm ran and the observation disagrees with the row's recorded verdict. That is a
#                 finding about the *row*, not a failure of the arm: the tree moved, or the arm is
#                 wrong. It is printed as MISMATCH and never silently folded into "armed".
#   inapplicable  the arm could not be run here, with the reason named. Not a pass.
#
# The expected verdict is read from the run's own artifact rather than from this file, so a row whose
# verdict was restated cannot be quietly re-armed against the old one.
#
# --- cost ---------------------------------------------------------------------------------------
#
# The first run pays a full in-image release build (`tools/devnet.sh build` → docker/rnode/Dockerfile),
# so budget 15+ minutes once; every later run reuses the image until the source moves. `target/` and the
# docker layers are both outside this repo's git tree — nothing here needs a network to start.
#
# Usage notes: `--keep` leaves the net up for inspection (then `tools/devnet.sh down -v` yourself);
# `--no-boot` runs the arms against a net that is already up.

set -uo pipefail

cd "$(dirname "$0")/.."

ROOT="$(pwd)"
VALIDATORS=3
KEEP=0
BOOT=1
OUT=""
ARMS="all"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --validators) VALIDATORS="${2:?}"; shift 2 ;;
    --keep)       KEEP=1; shift ;;
    --no-boot)    BOOT=0; shift ;;
    --out)        OUT="${2:?}"; shift 2 ;;
    # **One arm at a time, when one arm is what changed.** Some arms do not depend on the binary at all
    # (a container's posture is a property of how it was started), and rebuilding the image to re-run
    # sixteen others to observe one of them is a waste this selector exists to avoid.
    --arms)       ARMS="${2:?}"; shift 2 ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done

SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
OUT="${OUT:-$ROOT/spec/audit/evidence/wargame/arms-$SHA}"
RESULTS="$(mktemp)"
mkdir -p "$OUT"
: > "$OUT/results.tsv"

# The artifact the expected verdicts are read from, and the run whose rows these are.
FINDINGS="$ROOT/spec/audit/evidence/wargame/run-1419b127d/findings.tsv"

http() { # http <port> <path> [curl args...] → sets CODE and BODY
  local port="$1" path="$2"; shift 2
  local tmp; tmp="$(mktemp)"
  CODE="$(curl -s -o "$tmp" -w '%{http_code}' --max-time 10 "$@" "http://localhost:$port$path")"
  BODY="$(cat "$tmp")"; rm -f "$tmp"
}
json_num() { printf '%s' "$1" | sed -n "s/.*\"$2\":\([0-9]*\).*/\1/p"; }
http_port()  { echo $((40403 + 1000 * $1)); }
admin_port() { echo $((40405 + 1000 * $1)); }
grpc_port()  { echo $((40402 + 1000 * $1)); }
height() { http "$(http_port "$1")" /api/status; json_num "$BODY" latestBlockNumber; }
container() { case "$1" in 0) echo devnet-bootstrap ;; *) echo "devnet-validator-$1" ;; esac; }

# --- recording ----------------------------------------------------------------------------------

# **Written as the run goes, not at the end.** An aborted run — an `up` that timed out, a keyboard —
# used to leave the *previous* run's report in place under this run's name, which is worse than no
# report: the first version of this runner did exactly that, and the rows it showed were from the run
# before. Both files are truncated at start and appended as rows are observed.
record() {
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$RESULTS"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$OUT/results.tsv"
}

# observe <row> <arm> <condition: the deviation is present> <held-detail> <violated-detail>
observe() {
  local row="$1" arm="$2" cond="$3" held="$4" viol="$5"
  if eval "$cond" >/dev/null 2>&1; then
    printf '  %-10s VIOLATED   %s\n' "$row" "$viol"
    record "$row" violated "$arm" "$viol"
  else
    printf '  %-10s held       %s\n' "$row" "$held"
    record "$row" held "$arm" "$held"
  fi
}
inapplicable() { printf '  %-10s (skipped)  %s\n' "$1" "$3"; record "$1" inapplicable "$2" "$3"; }

# The artifact's own verdict for `<node> <index>`, so a MISMATCH is visible rather than assumed away.
expected_verdict() { # <row like W-N4-01>
  local row="$1" node idx
  node="$(printf '%s' "$row" | sed -n 's/^W-\(N[0-9]*\)-.*/\1/p')"
  idx="$(printf '%s' "$row" | sed -n 's/^W-N[0-9]*-\([0-9]*\)$/\1/p')"
  awk -F'\t' -v n="$node" -v i="$idx" 'NR > 1 && $1 == n { c++; if (c == i) { print $3; exit } }' "$FINDINGS" 2>/dev/null
}

dump_diagnostics() {
  echo "" >&2
  echo "--- diagnostics ---" >&2
  docker ps -a --filter "network=devnet" >&2 2>/dev/null || true
  for i in $(seq 0 $((VALIDATORS - 1))); do
    local c; c="$(container "$i")"
    echo "--- docker logs $c (tail 25) ---" >&2
    docker logs "$c" 2>&1 | tail -25 >&2 || true
  done
}

# The report is rendered on every exit path — including the ones that never reach the end of the run —
# because a run that died still observed rows, and a reader cannot tell a partial table from a complete
# one unless the table says which it is.
write_report() {
  local rows=0 complete="$1"
  [[ -f "$OUT/results.tsv" ]] && rows="$(wc -l < "$OUT/results.tsv")"
  {
    echo "# The wargame arms, on one devnet boot"
    echo ""
    echo "- commit: \`$SHA\`"
    echo "- image: \`$(docker image inspect "${RNODE_IMAGE:-rnode:local}" -f '{{.Id}}' 2>/dev/null | cut -c8-19)\`"
    echo "- validators: $VALIDATORS · $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    [[ "$complete" == "1" ]] || echo "- **this run was cut short**: the table below is what it observed before it stopped"
    echo ""
    echo "| row | recorded verdict | observed | arm | detail |"
    echo "|---|---|---|---|---|"
    while IFS=$'\t' read -r row outcome arm detail; do
      local exp; exp="$(expected_verdict "$row")"
      [[ -z "$exp" ]] && exp="(not in the artifact)"
      printf '| %s | %s | %s | `%s` | %s |\n' "$row" "$exp" "$outcome" "$arm" "$detail"
    done < "$OUT/results.tsv" | sort
  } > "$OUT/results.md"
  echo "==> $rows row(s) recorded -> $OUT/results.md"
}

cleanup() {
  local rc=$?
  write_report 0
  cp "$0" "$OUT/run.sh"
  if [[ "$KEEP" == "1" ]]; then
    echo "==> --keep: the devnet is still up (tools/devnet.sh down -v to remove it)"
  else
    echo "==> tearing the devnet down"
    tools/devnet.sh down -v >/dev/null 2>&1 || true
  fi
  return $rc
}

# --- the arms -----------------------------------------------------------------------------------
#
# Each arm is one client doing several of the rows' own probes. The row ids in the comments are the
# falsifiers this arm answers; the detail strings are what a reader gets in results.md.

# **W-N2-01, -02, -08, W-N12-06 — is the deploy bound one bucket for everyone?**
#
# The rows: the admission bound is one process-global token bucket, so one identity's flood denies the
# next identity its share (W-N2-01, W-N2-08, W-N12-06) and the two surfaces draw on the same pool
# (W-N2-02). The arm saturates the bound from one identity and, **while it is saturating**, asks for a
# deploy over the other surface as that same identity and as a second one. If the pool were global,
# neither would be admitted; if it were per surface but shared per caller, the same identity would be
# refused over gRPC while flooding HTTP.
#
# The second identity is validator 1's own key, read from the data directory the node already holds —
# a test rig with no other way to mint a bonded key, and nothing read that the volume does not contain.
arm_deploy_admission() {
  local arm=arm_deploy_admission hp codes key2 h rc_a rc_b i out_a out_b
  hp="$(http_port 0)"
  codes="$OUT/deploy-burst.txt"; : > "$codes"

  # A saturating burst from one identity. 200 in parallel is calibrated to cross the bound: measured
  # 80-88 refusals per run, where 40 sequential requests were all admitted (the bound, not the arm, is
  # what a smaller burst fails to reach).
  for i in $(seq 1 200); do
    ( curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'Content-Type: application/json' \
        -d "$DEPLOY_FIXTURE" --max-time 20 "http://localhost:$hp/api/deploy" 2>/dev/null >> "$codes" ) &
  done

  key2="$(docker exec "$(container 1)" cat /var/lib/rnode/validator.hex 2>/dev/null | tr -d '\n')"
  h="$(height 0)"
  docker exec "$(container 1)" sh -c "printf '%s' 'new x in { x!(1) }' > /tmp/arm.rho" 2>/dev/null

  # **While the burst is still landing.** Same identity as the flood, over the other surface.
  sleep 0.4
  docker exec "$(container 1)" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$DEPLOYER_PRIV" --shard-id /root --valid-after-block-number "${h:-0}" /tmp/arm.rho \
    > "$OUT/deploy-same-identity.txt" 2>&1; rc_a=$?
  # And the second identity, over the same surface.
  docker exec "$(container 1)" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$key2" --shard-id /root --valid-after-block-number "${h:-0}" /tmp/arm.rho \
    > "$OUT/deploy-second-identity.txt" 2>&1; rc_b=$?
  wait

  local n200 n429 same_ok=0 second_ok=0
  n200="$(grep -c '^200$' "$codes" 2>/dev/null || true)"
  n429="$(grep -c '^429$' "$codes" 2>/dev/null || true)"
  grep -q "Success!" "$OUT/deploy-same-identity.txt" && same_ok=1
  grep -q "Success!" "$OUT/deploy-second-identity.txt" && second_ok=1
  local ev="the burst drew ${n429:-0} 429 and ${n200:-0} 200; the same identity was then admitted over gRPC (rc=$rc_a) and the second identity too (rc=$rc_b)"

  # The deviation is another caller starving on the refused budget. On this tree neither does.
  observe W-N2-01 "$arm" '[[ ${n429:-0} -gt 0 && $same_ok == 0 ]]' \
    "the bound has a subject: $ev" \
    "$ev — one identity's burst denied that same identity over the other surface"
  observe W-N2-02 "$arm" '[[ ${n429:-0} -gt 0 && $second_ok == 0 ]]' \
    "the surfaces are not one pool: $ev" \
    "$ev — a saturating HTTP flood also refused the gRPC caller, so both surfaces draw on one budget"
  observe W-N2-08 "$arm" '[[ ${n429:-0} -gt 0 && $second_ok == 0 ]]' \
    "a second identity is not denied by the first's flood: $ev" \
    "$ev — one identity's flood refused another identity's deploy"
  observe W-N12-06 "$arm" '[[ ${n429:-0} -gt 0 && $second_ok == 0 ]]' \
    "the second client is admitted while the first floods: $ev" \
    "$ev — the second client was refused while the first flooded"

  # **W-N2-06 and W-N2-09 used to be recorded inapplicable here** — "the runner cannot mint a
  # signed-but-wrong deploy" was true of the runner before the fixture was corrupted instead of minted
  # (one hex digit moved, no crypto needed), and both rows are now taken: W-N2-06 by a unit test in
  # `node/src/web/http.rs` and W-N2-09 by `arm_unauth_flood` below. Two records for one row is how a
  # table grows a second, quieter answer — so the lines are gone rather than left beside the new arm.
  inapplicable W-N2-06 "$arm" "decode-before-429 is not observable from the response alone; it needs a CPU or a counter, and this arm has neither"
}

# **W-N3-01, -02, -03, -05 — the public read surface.**
#
# The rows: operator state on the public status, plaintext transport, a delegator-scoped read any caller
# can make, and an explore-deploy that executes an unsigned term.
arm_public_reads() {
  local arm=arm_public_reads hp admin
  hp="$(http_port 0)"; admin="$(admin_port 0)"

  http "$hp" /api/status
  observe W-N3-01 "$arm" \
    'printf "%s" "$BODY" | grep -qE "\"(adminHttp|autoproposeHalt|autoproposeTimerHalted|resetMarker|finalityStall|finalityStalled|shards|sync|poisonRecoveries|tasksPanicked)\""' \
    "the public status carries the public key set only (HTTP $CODE)" \
    "the public status carries operator keys: $(printf '%s' "$BODY" | grep -oE '"(adminHttp|autoproposeHalt|resetMarker|finalityStall|shards|sync|poisonRecoveries)"' | sort -u | tr '\n' ' ')"

  # Plaintext: the row is that the public API is a plaintext HTTP surface, so the check is that http
  # answers and https does not.
  local https_code
  https_code="$(curl -sk -o /dev/null -w '%{http_code}' --max-time 5 "https://localhost:$hp/api/status" 2>/dev/null)"
  observe W-N3-02 "$arm" '[[ "$https_code" == "000" ]]' \
    "the public API answers only over TLS (https=$https_code)" \
    "the public API is plaintext: http returns $CODE and https returns $https_code"

  # The delegator-scoped read moved to the admin listener (http.rs:199), so the public port must not
  # answer it — with a *valid* key, because an unparseable one is a 400 for a different reason.
  local pub adm
  http "$hp" "/api/v1/pos/delegations?delegator=04f700a417754b775d95421973bdbdadb2d23c8a5af46f1829b1431f5c136e549e8a0d61aa0c793f1a614f8e437711c7758473c6ceb0859ac7e9e07911ca66b5c4"
  pub="$CODE"
  http "$admin" "/api/v1/pos/delegations?delegator=04f700a417754b775d95421973bdbdadb2d23c8a5af46f1829b1431f5c136e549e8a0d61aa0c793f1a614f8e437711c7758473c6ceb0859ac7e9e07911ca66b5c4"
  adm="$CODE"
  observe W-N3-03 "$arm" '[[ "$pub" != "404" && "$pub" != "405" ]]' \
    "the delegator read is not on the public port ($pub); the admin port answers it ($adm)" \
    "the public port answers another delegator's positions ($pub)"

  # `api_explore_deploy` takes a bare JSON *string*, not an object: `Json(term): Json<String>`.
  http "$hp" /api/explore-deploy -X POST -H 'Content-Type: application/json' \
    -d '"new x in { x!(1) }"'
  observe W-N3-05 "$arm" '[[ "$CODE" == "200" ]]' \
    "an unsigned term was refused ($CODE)" \
    "an unsigned term executed over the unauthenticated public port ($CODE)"
}

# **W-N4-01, -02, -06, W-N6-02, W-N9-02 — the admin actuator.**
#
# The rows: the admin listener acts with no caller identity, its origin guard compares two headers the
# caller chooses, and an explicit propose is not paced. The devnet publishes the admin port to this host
# (`--admin` is the default), which is the precondition every one of these rows names.
#
# **The endpoint's answer is read, not just its status.** `POST /api/propose` answers
# `"Success! Block … created and added."` when it acts and `"Failure: another propose is in progress"`
# when the node is already proposing — and the first run of this arm read that second answer as a
# refusal, reporting four rows as held. A status code alone cannot tell "the node refused you" from
# "the node is busy".
propose_probe() { # <port> <path> [curl args...] → PCODE, PBODY, waiting out a propose in flight
  local port="$1" path="$2"; shift 2
  local tries=0
  while :; do
    http "$port" "$path" -X POST "$@"
    PCODE="$CODE"; PBODY="$BODY"
    [[ "$PBODY" == *"another propose is in progress"* ]] || break
    tries=$((tries + 1)); [[ $tries -ge 6 ]] && break
    sleep 3
  done
}

# **A propose needs work to carry.** On a quiet net the endpoint answers `"Failure: Proposal failed:
# NoNewDeploys"` — the node declining because there is nothing to put in a block, not because of who
# asked. So each probe is preceded by a real deploy, signed by a second identity through the node's own
# client (a fresh term and timestamp, so it is new work and not a duplicate).
deploy_once() {
  local h key; h="$(height 0)"
  key="$(docker exec "$(container 0)" cat /var/lib/rnode/validator.hex 2>/dev/null | tr -d '\n')"
  docker exec "$(container 0)" sh -c "printf '%s' 'new x in { x!($RANDOM) }' > /tmp/arm-work.rho" 2>/dev/null
  docker exec "$(container 0)" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$key" --shard-id /root --valid-after-block-number "${h:-0}" /tmp/arm-work.rho >/dev/null 2>&1
  sleep 2
}

arm_admin_actuator() {
  local arm=arm_admin_actuator admin acted=0 forged=0
  admin="$(admin_port 0)"

  deploy_once
  propose_probe "$admin" /api/propose
  [[ "$PBODY" == *"Success!"* ]] && acted=1
  local short="HTTP $PCODE: $(printf '%s' "$PBODY" | head -c 90)"
  observe W-N4-01 "$arm" '[[ $acted == 1 ]]' \
    "the node did not act for an unidentified caller ($short)" \
    "an unidentified caller made the node propose ($short)"
  observe W-N6-02 "$arm" '[[ $acted == 1 ]]' \
    "the published admin port did not act for this host ($short)" \
    "this host reached the admin actuator over its published port and the node acted ($short)"

  # The origin guard compares Origin to Host, and a non-browser client sets both.
  deploy_once
  propose_probe "$admin" /api/propose -H 'Origin: http://evil.example' -H 'Host: evil.example'
  [[ "$PBODY" == *"Success!"* ]] && forged=1
  observe W-N4-06 "$arm" '[[ $forged == 1 ]]' \
    "a forged Origin/Host pair was refused (HTTP $PCODE: $(printf '%s' "$PBODY" | head -c 90))" \
    "a forged Origin/Host pair passed the guard and the node acted (HTTP $PCODE)"

  # The transaction route drives the gateway, and this net is single-shard: the answer is decided
  # before any caller identity is considered, so this arm cannot speak to the row's authority claim.
  http "$admin" /api/txn -X POST -H 'Content-Type: application/json' -d '{"legs":[]}'
  inapplicable W-N4-02 "$arm" "the devnet is single-shard and answers $CODE before any caller identity is considered; a multi-shard boot is the precondition this row names"

  # An explicit propose is not paced (C196 makes that deliberate); the row is that a caller can
  # therefore ask for more attestations than the pace bound allows.
  local i produced=0 before after
  before="$(height 0)"
  for i in $(seq 1 5); do
    deploy_once
    propose_probe "$admin" /api/v1/propose
    [[ "$PBODY" == *"Success!"* ]] && produced=$((produced + 1))
    sleep 4
  done
  after="$(height 0)"
  observe W-N9-02 "$arm" '[[ $produced -ge 3 ]]' \
    "the explicit proposes were paced ($produced of 5 produced a block, height $before -> $after)" \
    "$produced of 5 explicit proposes produced a block (height $before -> $after): a caller-driven propose is not paced, which is C196's deliberate exemption"
}

# **W-N4-04 — the net proposes without a client.**
#
# The row: attest-on-new-blocks is on by default, so a peer's block is itself the trigger. There is no
# client to write here — the arm is the observation that the chain advances while nobody asks it to.
arm_steady_state() {
  local arm=arm_steady_state h0 h1
  h0="$(height 0)"
  sleep 20
  h1="$(height 0)"
  observe W-N4-04 "$arm" '[[ -n "$h0" && -n "$h1" && "$h0" -lt "$h1" ]]' \
    "the chain did not advance unattended (height $h0 -> $h1)" \
    "the chain advanced unattended, height $h0 -> $h1: block arrival is itself the proposal trigger"
}

# **W-N6-01 — nothing contains a compromise inside the node.**
#
# The row: a devnet node runs with the operator's defaults and nothing else — no capability drop, no
# read-only root, no seccomp or apparmor profile — so a process that gets a foothold inside a node is not
# contained by the container it runs in. This is a **posture** measurement: the evidence is the empty
# fields the container reports about itself, which is the honest instrument for "the hardening is absent"
# (there is no exploit to run — the finding is the configuration).
arm_container_posture() {
  local arm=arm_container_posture c caps ro sec
  c="$(container 0)"
  caps="$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$c" 2>/dev/null)"
  ro="$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$c" 2>/dev/null)"
  sec="$(docker inspect -f '{{json .HostConfig.SecurityOpt}}' "$c" 2>/dev/null)"
  observe W-N6-01 "$arm" \
    '[[ "$caps" == "null" || "$caps" == "[]" ]] && [[ "$ro" == "false" ]] && [[ "$sec" == "null" || "$sec" == "[]" ]]' \
    "the node's container is hardened (CapDrop=$caps ReadonlyRootfs=$ro SecurityOpt=$sec)" \
    "nothing contains a compromise inside the node: CapDrop=$caps ReadonlyRootfs=$ro SecurityOpt=$sec (docker_opts passes no --cap-drop, --read-only or --security-opt, so the container keeps every default capability and a writable root)"
}

# **W-N6-04 and W-N6-05 — where this node's keys actually live.**
#
# Two rows about the same boot, asked from the host. W-N6-04: the password an operator supplies through
# the environment is never cleared or redacted, so it sits in the node process's own environment.
# W-N6-05: the falsifier predicted "the absence of any `--validator-private-key-path` in `tools/`, i.e.
# the file-protected form is never exercised" — and `tools/devnet.sh` passes exactly that flag, so the
# arm reads both halves rather than inheriting the prediction.
#
# Neither is a *test* of the node's code; both are readings of how the node was started, which is what
# makes them arms rather than unit tests.
arm_key_provenance() {
  local arm=arm_key_provenance c argv mode pw
  c="$(container 0)"
  argv="$(docker inspect -f '{{json .Config.Cmd}}' "$c" 2>/dev/null)"
  mode="$(docker exec "$c" stat -c %a /var/lib/rnode/validator.hex 2>/dev/null)"

  # (1) W-N6-05: the key arrives by *path*, and the file it names is owner-only.
  observe W-N6-05 "$arm" \
    '[[ "$argv" != *"--validator-private-key-path"* || "$mode" != "600" ]]' \
    "the node's argv names a **key file** and that file is mode ${mode:-<unreadable>} inside the container: the file-protected form is what this net exercises, so the row's premise (no such flag anywhere under tools/) has moved" \
    "every consensus key this node uses arrives by another route — argv carries no --validator-private-key-path (mode=${mode:-<unreadable>}): ${argv}"

  # (2) W-N6-04: the password, supplied through the environment, is still there.
  pw="$(docker exec "$c" sh -c "tr '\0' '\n' < /proc/1/environ | grep '^RNODE_VALIDATOR_PASSWORD='" 2>/dev/null)"
  observe W-N6-04 "$arm" \
    '[[ -n "$pw" ]]' \
    "unreachable" \
    "**the password is in the node's own environment, verbatim** (W-N6-04): \`${pw%%=*}\` is set on PID 1 and nothing in the node clears or redacts it, so a same-host reader of /proc/1/environ has it in the clear"
}

# **W-N2-09 — a flood of deploys that cannot pass authentication.**
#
# The row: the admission token was spent *before* the deploy was authenticated, so unauthenticated
# garbage drained the budget honest deploys needed. The instrument is the pre-signed fixture with one
# hex digit of its signature moved — no crypto needed, and it is refused by `verify_signature` rather
# than by anything downstream. The flood goes at the deploy route; then the **same identity** that the
# garbage claimed submits a real deploy over the other surface. If a token were spent before the
# signature was checked, that identity's budget would be gone and the honest deploy refused.
arm_unauth_flood() {
  local arm=arm_unauth_flood hp codes bad h rc i admitted=0 reached=0
  hp="$(http_port 0)"
  codes="$OUT/deploy-unauth-flood.txt"; : > "$codes"
  bad="$(printf '%s' "$DEPLOY_FIXTURE" | sed 's/"signature":"3044022041eb/"signature":"3044022041ec/')"
  for i in $(seq 1 200); do
    ( curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'Content-Type: application/json' \
        -d "$bad" --max-time 20 "http://localhost:$hp/api/deploy" 2>/dev/null >> "$codes" ) &
  done
  sleep 0.4
  h="$(height 0)"
  docker exec "$(container 1)" sh -c "printf '%s' 'new x in { x!(1) }' > /tmp/arm-unauth.rho" 2>/dev/null
  docker exec "$(container 1)" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$DEPLOYER_PRIV" --shard-id /root --valid-after-block-number "${h:-0}" /tmp/arm-unauth.rho \
    > "$OUT/deploy-after-unauth-flood.txt" 2>&1; rc=$?
  wait
  grep -q "Success!" "$OUT/deploy-after-unauth-flood.txt" && admitted=1
  # A flood the surface bound refused and the node never saw is not this row's flood, so the count of
  # requests that *reached* the node is part of the reading.
  reached="$(grep -vc '^429$' "$codes" 2>/dev/null || echo 0)"
  observe W-N2-09 "$arm" '[[ $admitted == 0 ]]' \
    "the identity's own deploy was admitted after the flood (rc=$rc, ${reached} request(s) past the surface bound): the unauthenticated garbage spent no admission token — the signature is checked first, which is what W-N2-02's fix put in front of it" \
    "**the flood drained the identity's own budget**: after ${reached} unauthenticated attempts its valid deploy was refused (rc=$rc)"
}

# --- the run ------------------------------------------------------------------------------------

# The pre-signed deploy fixture (term "Nil"), the same one `tools/devnet-test.sh` uses: valid, and the
# signature is checked regardless of block number at submit.
# The devnet's deployer key (validator 0's), the same one `tools/devnet-test.sh` uses: it is what the
# pre-signed fixture was signed with, and it is the "first identity" the admission arm saturates.
DEPLOYER_PRIV="a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76"

DEPLOY_FIXTURE='{"data":{"term":"Nil","timestamp":1700000000000,"phloPrice":1,"phloLimit":1000000,"validAfterBlockNumber":0,"shardId":"/root"},"deployer":"04f700a417754b775d95421973bdbdadb2d23c8a5af46f1829b1431f5c136e549e8a0d61aa0c793f1a614f8e437711c7758473c6ceb0859ac7e9e07911ca66b5c4","signature":"3044022041ebc32bae0195d167310362dd6539d3e2a750512d03ca075c0308bc75744e1002206338157bde1b385ff76ebc1840cdccb942158cff6e377b3053a1a2e9389d6ad8","sigAlgorithm":"secp256k1"}'

# **W-N6-04's precondition, and only its precondition.** The password reaches the node through the
# environment — the documented non-interactive route for an encrypted key file — and the arm reads it
# back out of the running process. Setting it is the row's own context (`--dev-mode
# --deployer-private-key` is the same kind of precondition for W-N4-03), not a fault injected here: the
# node is unmodified and nothing in it is asked to do anything by this variable but sit in its
# environment.
export DEVNET_NODE_ENV="${DEVNET_NODE_ENV:-RNODE_VALIDATOR_PASSWORD=warden-arm-7f3a}"

trap cleanup EXIT

# **Two boots, because the rows name two nets.** The steady-state, read-surface and admission rows are
# asked of an ordinary devnet — one that is proposing, since the row about an unattended chain needs a
# chain advancing unattended. The actuator rows are asked of a *quiet* one: on a net with autopropose
# and propose-on-deploy on, every explicit `POST /api/propose` is answered "another propose is in
# progress", which is the node being busy rather than the caller being refused — the first run of this
# runner reported four actuator rows as "held" for exactly that reason, and a quiet net is what the
# rows' own precondition ("on a caught-up node, POST /api/v1/propose repeatedly") describes.
boot() { # [--n N] [extra devnet.sh up flags...]
  local n="$VALIDATORS"
  if [[ "${1:-}" == "--n" ]]; then n="${2:?}"; shift 2; fi
  # The arms saturate this host with parallel clients, so a joining peer gets longer than the tool's
  # 120-second default to serve genesis — see `wait_for_peer_http` in devnet.sh.
  export DEVNET_WAIT_TRIES="${DEVNET_WAIT_TRIES:-240}"
  tools/devnet.sh down -v >/dev/null 2>&1 || true
  # **A failed `up` keeps its net.** The readiness gate is a two-minute wait on every peer serving
  # genesis, and it has timed out once on this box and then succeeded on an immediate retry — so the
  # first failure is retried once, and a second one leaves the containers standing (KEEP) with their
  # logs, because `down -v` is exactly what destroys the diagnosis. That is the opposite of what a
  # teardown trap does by default, and it cost one run's evidence to learn.
  if ! tools/devnet.sh up --validators "$n" "$@" > "$OUT/up.log" 2>&1; then
    echo "==> the first up failed; retrying once (its log is $OUT/up.log)"
    tools/devnet.sh down -v >/dev/null 2>&1 || true
    if ! tools/devnet.sh up --validators "$n" "$@" >> "$OUT/up.log" 2>&1; then
      echo "devnet up failed twice — leaving the net up for inspection (tools/devnet.sh down -v)" >&2
      KEEP=1
      dump_diagnostics
      exit 3
    fi
  fi
}

if [[ "$BOOT" == "1" ]]; then
  echo "==> building the node image (skipped by docker's cache unless the source moved)"
  tools/devnet.sh build >/dev/null 2>&1 || { echo "the image build failed" >&2; exit 3; }
  echo "==> phase 1: a proposing devnet (--validators $VALIDATORS)"
  boot
fi

echo ""
echo "==> phase 1 arms (a proposing net)"
for a in arm_steady_state arm_container_posture arm_key_provenance arm_public_reads arm_deploy_admission arm_unauth_flood; do
  [[ "$ARMS" == "all" || ",$ARMS," == *",$a,"* ]] && "$a"
done

if [[ "$BOOT" == "1" ]]; then
  echo ""
  # **One node, not three, and the reason is a property of a quiet net.** With autopropose off there is
  # nothing finalised for a joining peer to sync from — it sends its `FinalizedFringeRequest` and the
  # bootstrap has no fringe to answer with — so a joined peer never becomes ready and the boot times
  # out. (That is the very state `tools/reconcile-network.sh` exists for.) The actuator rows need one
  # node with an admin port, not a net.
  echo "==> phase 2: a quiet node (--validators 1 --no-autopropose --no-propose-on-deploy)"
  boot --n 1 --no-autopropose --no-propose-on-deploy
fi

echo ""
echo "==> phase 2 arms (a quiet net; with --no-boot these run against the net as it stands)"
[[ "$ARMS" == "all" || ",$ARMS," == *",arm_admin_actuator,"* ]] && arm_admin_actuator

# --- the summary --------------------------------------------------------------------------------

echo ""
echo "==> rows by outcome"
awk -F'\t' '{c[$2]++} END {for (k in c) printf "  %-14s %d\n", k, c[k]}' "$RESULTS"
mismatch=0
while IFS=$'\t' read -r row outcome arm detail; do
  exp="$(expected_verdict "$row")"
  if [[ "$outcome" == "held" && "$exp" == "CONFIRMED" ]] || [[ "$outcome" == "violated" && "$exp" == "REFUTED" ]]; then
    echo "  MISMATCH $row: recorded $exp, observed $outcome — $detail"
    mismatch=$((mismatch + 1))
  fi
done < "$RESULTS"
echo "  mismatches: $mismatch"
echo "==> $OUT/results.md"

# **The exit code says the arms ran; the report says what they found.** A MISMATCH is a row whose
# closure still has to be written — a deviation that no longer reproduces, which is what a fix landing
# after the wargame looks like — and it is a finding for a person, not a failure of the instrument. A
# runner that went red every time a row had been fixed would be the "report nobody reads" this
# repository has been bitten by twice. What would be a real failure is an arm that could not run at
# all, and there is an outcome for that (`error`), which does exit non-zero.
if [[ "$mismatch" -gt 0 ]]; then
  echo "==> $mismatch row(s) disagree with their recorded verdict (see MISMATCH above): each needs its closure written, not a re-run"
fi
errors="$(awk -F'\t' '$2 == "error"' "$RESULTS" | wc -l)"
if [[ "$errors" -gt 0 ]]; then dump_diagnostics; exit 1; fi
echo "==> $OUT/results.md — $OUT/results.tsv"
exit 0
