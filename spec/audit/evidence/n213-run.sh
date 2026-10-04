#!/usr/bin/env bash
# n213 — the three close conditions of issue #213, bounded, on a live net.
#
# Protocol: `n213-preregistration.md`. The fix under test is C209 (the attestation guard reads every input
# from `latest_msgs`) plus C210 (a refused propose arms one Automatic retry). This script asks the three
# questions the issue says close it:
#
#   (a) kill one of three at 80 %: do the survivors finalise past the kill?
#   (b) the killed validator restarts: do production and finality resume with no operator action?
#   (c) a deploy accepted while the validator is absent is included once the survivors can finalise.
#
# **The witness, and why v1 of this script was wrong.** The issue is explicit: the witness is the deploy
# FINALISING — `last-finalized-block >=` the height of the block that carries it — never "the height
# stopped moving". v1 keyed case (a) to `await_finality(baseline)`, which asks only that finality move past
# the *pre-kill* baseline; in the run it was used on, the post-kill deploy sat at height >= 6 while finality
# never passed 5, so it printed PASS without the deploy's block ever finalising. That is a proxy, not the
# observation. This version finds the block that carries each deploy, requires *that block* to finalise, and
# records `GET /api/is-finalized/{hash}` as the direct witness beside it.
#
# (d): the whole run is bounded; a timeout is a FAIL, not a pass.
#
# Usage:  spec/audit/evidence/n213-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
SETTLE_S=${SETTLE_S:-60}
FINALITY_S=${FINALITY_S:-120}
KILL_ABSENT_S=${KILL_ABSENT_S:-45}
RESTART_S=${RESTART_S:-120}
RUN_BUDGET_S=${RUN_BUDGET_S:-900}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
STOP_NODE=${STOP_NODE:-2}          # the 50-stake validator: 100+100 of 250 is 80 %
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}
# `--attest-on-new-blocks` is the merged default (`defaults.conf:16`); passed explicitly only so the run's
# argv is self-describing. It is NOT required — v1 of this rig claimed it was, and that was wrong.
EXTRA_FLAGS=${EXTRA_FLAGS:---attest-on-new-blocks}

TREE=$(git rev-parse --short HEAD)
HEAD_FULL=$(git rev-parse HEAD)
ORIGIN_DEV=$(git rev-parse --short origin/dev 2>/dev/null || echo unknown)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT_ROOT=${OUT_ROOT:-target/n213}
OUT="$OUT_ROOT/${TREE}-${STAMP}"
mkdir -p "$OUT"

{
  echo "# tree=$TREE head=$HEAD_FULL origin/dev=$ORIGIN_DEV"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy"
  echo "#      DEVNET_EXTRA_FLAGS=$EXTRA_FLAGS   (explicit; already the default — defaults.conf:16)"
  echo "# windows: settle=${SETTLE_S}s finality=${FINALITY_S}s absent=${KILL_ABSENT_S}s restart=${RESTART_S}s budget=${RUN_BUDGET_S}s"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

BUDGET_END=$(( $(date +%s) + RUN_BUDGET_S ))
WAIT_UNTIL() { local d=$(( $1 - $(date +%s) )); (( d > 0 )) && sleep "$d" || true; }
OVER_BUDGET() { (( $(date +%s) > BUDGET_END )); }
PORT_OF() { echo $(( 40403 + $1 * 1000 )); }

start_sampler() {
  N149_N=3 N149_WINDOW_S=$(( RUN_BUDGET_S + 60 )) \
    python3 spec/audit/evidence/n149-sample.py "$1" > "$1/sampler.log" 2>&1 &
  echo $!
}

bring_up() {
  local dir="$1"
  tools/devnet.sh down >/dev/null 2>&1
  DEVNET_NODE_MEMORY="$CAP" DEVNET_EXTRA_FLAGS="$EXTRA_FLAGS" \
    timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
    --epoch-length 10 --no-autopropose --propose-on-deploy 2>&1 | tail -1
  local deadline=$(( $(date +%s) + JOIN_TIMEOUT )) joined=0
  while (( $(date +%s) < deadline )); do
    joined=0
    for i in 0 1 2; do
      local bn; bn=$(height "$i")
      [[ -n "$bn" && "$bn" -gt 0 ]] && joined=$((joined + 1))
    done
    (( joined >= 3 )) && break
    sleep 2
  done
  (( joined >= 3 )) || { echo "  VOID: $joined of 3 past genesis" | tee "$dir/void.txt"; return 1; }
  return 0
}

# The newest block height a node reports; empty if it cannot be read.
height() {
  curl -fsS --max-time 3 "http://localhost:$(PORT_OF "${1:-0}")/api/v1/status" 2>/dev/null \
    | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'
}

# The highest block any node reports as finalised; `none` when nothing has finalised.
finalised() {
  local best="" v
  for i in 0 1 2; do
    v=$(curl -fsS --max-time 3 "http://localhost:$(PORT_OF "$i")/api/last-finalized-block" 2>/dev/null \
        | sed -n 's/.*"blockNumber":\([0-9]*\).*/\1/p')
    if [[ -n "$v" ]]; then
      if [[ -z "$best" ]] || (( v > best )); then best="$v"; fi
    fi
  done
  echo "${best:-none}"
}

# **The block that carries a deploy**: the lowest-height block with `deployCount >= 1` seen above `floor`.
# Prints "<blockNumber> <blockHash>". This is what case (a)/(c) must key on — the proxy it replaces
# ("finality moved past the baseline") is satisfied by pre-kill blocks and says nothing about the deploy.
deploy_block() {
  local port="$1" floor="$2" deadline="$3" out="" h depth raw refused=0
  while (( $(date +%s) < deadline )); do
    # `GET /api/blocks/{depth}` clamps internally (probed: 200 at depth 50 on a one-block chain), and the
    # sampler's `min(50, height)` is kept as the documented idiom rather than a necessity.
    h=$(curl -fsS --max-time 3 "http://localhost:$port/api/v1/status" 2>/dev/null \
        | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p')
    depth=$(( ${h:-1} < 50 ? ${h:-1} : 50 )); (( depth < 1 )) && depth=1
    raw=$(curl -fsS --max-time 5 "http://localhost:$port/api/blocks/$depth" 2>/dev/null)
    out=$(printf '%s' "$raw" | python3 -c '
import json,sys
raw = sys.stdin.read()
try:
    bs = json.loads(raw)
except Exception:
    print("REFUSED"); sys.exit(0)
if not isinstance(bs, list):
    print("REFUSED"); sys.exit(0)
floor = int(sys.argv[1]); best = None
for b in bs:
    try:
        n = int(b.get("blockNumber", -1)); dc = int(b.get("deployCount", 0)); bh = b.get("blockHash")
    except Exception:
        continue
    if dc >= 1 and n > floor and bh and (best is None or n < best[0]):
        best = (n, bh)
print(f"{best[0]} {best[1]}" if best else "NONE")
' "$floor")
    case "$out" in
      REFUSED) refused=1; sleep 3 ;;   # the node refused the read — not a verdict on the chain
      NONE|"") sleep 3 ;;              # read cleanly; the deploy is not in a block above the floor yet
      *) echo "$out"; return 0 ;;
    esac
  done
  (( refused == 1 )) && return 2   # the read was refused at least once — instrument error
  return 1                         # read cleanly throughout, no deploy-bearing block above the floor
}

# `GET /api/is-finalized/{hash}` — the direct witness, recorded rather than printed to a terminal.
is_finalized() {
  local hash="$1" r
  for i in 0 1 2; do
    r=$(curl -fsS --max-time 4 "http://localhost:$(PORT_OF "$i")/api/is-finalized/$hash" 2>/dev/null)
    [[ -n "$r" ]] && { echo "$r"; return 0; }
  done
  echo "unreadable"
}

await_finality_at_least() {
  local target="$1" deadline="$2" v
  while (( $(date +%s) < deadline )); do
    v=$(finalised)
    [[ "$v" != "none" ]] && (( v >= target )) && { echo "$v"; return 0; }
    sleep 3
  done
  v=$(finalised); echo "${v:-none}"; return 1
}

# One deploy, its own block found, its own finalisation tested. Echoes "num hash ok|no|notincluded".
deploy_and_watch() {
  local label="$1" deadline_s="$2"
  # **The floor is the current max block number, not `latestBlockNumber`.** `latestBlockNumber` is
  # `max_height + 1` (genesis alone reports 1), so using it as the floor makes the search `n > 1` and
  # excludes the deploy's block at height 1 — which is exactly what the second version of this finder did,
  # reporting a deploy sent at genesis as "no deploy-bearing block appeared".
  local lb; lb=$(height 0); local before=$(( ${lb:-1} - 1 )); (( before < 0 )) && before=0
  if timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1; then
    echo "  [$label] deploy sent at T+$(( $(date +%s) - T0 ))s (tip before=${before})"
  else
    echo "  [$label] deploy FAILED"; echo "0 - notincluded"; return
  fi
  local db rc
  db=$(deploy_block "$(PORT_OF 0)" "${before:-0}" $(( $(date +%s) + deadline_s ))); rc=$?
  if (( rc == 2 )); then
    echo "  [$label] INSTRUMENT ERROR — /api/blocks was refused, so this is not a verdict on the chain"
    echo "0 - instrument-error"; return
  fi
  if (( rc != 0 )); then
    echo "  [$label] NO DEPLOY-BEARING BLOCK appeared — included: no"; echo "0 - notincluded"; return
  fi
  local num="${db%% *}" hash="${db##* }"
  echo "  [$label] block $num (${hash:0:12}…) carries it — included: yes"
  if local fin; fin=$(await_finality_at_least "$num" $(( $(date +%s) + deadline_s ))); then
    echo "  [$label] PASS: block $num finalised (finality $fin; is-finalized=$(is_finalized "$hash"))"
    echo "$num $hash ok"
  else
    echo "  [$label] FAIL: block $num NEVER finalised (finality reached $fin; is-finalized=$(is_finalized "$hash"))"
    echo "$num $hash no"
  fi
}

main() {
  local dir="$OUT/run"; mkdir -p "$dir"
  echo "=================== n213 three-case run  $(date -u +%H:%M:%S) UTC ==========="
  bring_up "$dir" || return
  : > "$dir/witness.txt"
  local sampler; sampler=$(start_sampler "$dir")
  T0=$(date +%s)
  echo "  3 of 3 past genesis at t0"

  # ---- baseline: a live net that has finalised something before the kill.
  WAIT_UNTIL $(( T0 + SETTLE_S ))
  local base_line; base_line=$(deploy_and_watch baseline "$FINALITY_S")
  echo "$base_line" | tee -a "$dir/witness.txt" >/dev/null

  # ---- (a)/(c): kill, then a deploy only the survivors can include and finalise. That single deploy is
  # both (a)'s witness and (c)'s: it is *accepted while the validator is absent*.
  tools/devnet.sh stop "$STOP_NODE" >/dev/null 2>&1 \
    && echo "  killed validator-$STOP_NODE at T+$(( $(date +%s) - T0 ))s"
  local kill; kill=$(date +%s)
  WAIT_UNTIL $(( kill + KILL_ABSENT_S ))
  local absent_line; absent_line=$(deploy_and_watch "absent-deploy (cases a and c)" "$FINALITY_S")
  echo "$absent_line" >> "$dir/witness.txt"
  echo "  --- cases (a) and (c) ---"; echo "$absent_line" | sed 's/^/  /'

  # ---- (b): the killed validator returns.
  tools/devnet.sh start "$STOP_NODE" >/dev/null 2>&1 \
    && echo "  restarted validator-$STOP_NODE at T+$(( $(date +%s) - T0 ))s"
  local restart; restart=$(date +%s)
  WAIT_UNTIL $(( restart + 20 ))
  local restart_line; restart_line=$(deploy_and_watch "post-restart" "$RESTART_S")
  echo "$restart_line" >> "$dir/witness.txt"

  local end; end=$(date +%s)
  printf 'arm\tn213\nt0\t%s\nbaseline_block\t%s\nkill\t%s\nabsent_deploy_block\t%s\nrestart\t%s\npost_restart_block\t%s\nend\t%s\nbudget_s\t%s\n' \
    "$T0" "$(echo "$base_line" | tail -1 | cut -d' ' -f1)" "$kill" \
    "$(echo "$absent_line" | tail -1 | cut -d' ' -f1)" "$restart" \
    "$(echo "$restart_line" | tail -1 | cut -d' ' -f1)" "$end" "$RUN_BUDGET_S" > "$dir/marks.tsv"

  if OVER_BUDGET; then
    echo "  CASE (d) FAIL: exceeded the ${RUN_BUDGET_S}s budget" | tee -a "$dir/witness.txt"
    echo "FAIL" > "$dir/case-d.txt"
  else
    echo "  CASE (d) PASS: within the ${RUN_BUDGET_S}s budget" | tee -a "$dir/witness.txt"
    echo "PASS" > "$dir/case-d.txt"
  fi

  kill "$sampler" 2>/dev/null || true
  for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
    docker logs "$c" 2>&1 | grep -o 'finality did not advance at tip [0-9]*: .*' >> "$dir/stall-lines.txt" || true
    docker logs "$c" 2>&1 | grep -o 'round gate escaped[^"]*' >> "$dir/escape-lines.txt" || true
    docker logs "$c" 2>&1 | grep -o 'attestation at tip [0-9]*: [^"]*' >> "$dir/attestation-lines.txt" || true
  done
  tools/devnet.sh down >/dev/null 2>&1
  echo "  done at $(date -u +%H:%M:%S) UTC"
}

T0=0
main
echo
echo "=== the per-deploy witnesses (num hash ok|no|notincluded) ==="; cat "$OUT/run/witness.txt" 2>/dev/null
echo "=== (d) ==="; cat "$OUT/run/case-d.txt" 2>/dev/null
echo "=== the gate's own reason ==="; sort -u "$OUT/run/stall-lines.txt" 2>/dev/null | head -4
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
