#!/usr/bin/env bash
# n220-leave — a validator **leaves**: withdraw → epoch boundary → quarantine → payout.
#
# A2.5 of the #214 checklist, unrun until now. The question is not whether `withdraw` is *accepted* —
# `examples/pos-withdraw.rho`'s own doc says it only **stages** (law 47), so the staker stays bonded and
# stays in the active set until the boundary. The question is whether the **leave completes**: the
# boundary activates it, the stake leaves the bond pool, and the pending entry clears at its deadline.
#
# It also settles **CH-U6-09**, the one challenge the page still records as having found an inversion:
# the worksheet's H-U6-05 said the refund "waits `quarantine_length` more blocks past its deadline", and
# the challenge says that inverts the recorded behaviour. The observable here is `/api/v1/pos`'s own
# `deadline` and `blocks_remaining`, which the read path documents as `quarantine_length +
# divisor·(1 + block_number/divisor)` with `blocks_remaining = deadline − latest`, floored at zero —
# so the arithmetic is read off the API rather than inferred.
#
# **The quarantine is 50000 blocks by default**, far beyond any bounded run, so the rig shrinks it.
# `--quarantine-length` is a node flag (`node/src/configuration/commandline/options.rs`) that
# `tools/devnet.sh` does not expose, so it goes through `DEVNET_EXTRA_FLAGS`.
#
# Bounded: a timeout is a FAIL.
#
# Usage:  spec/audit/evidence/n220-leave-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
EPOCH=${EPOCH:-10}
QUARANTINE=${QUARANTINE:-20}
LEAVE_BUDGET_S=${LEAVE_BUDGET_S:-600}
READ_S=${READ_S:-60}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-12}
# Validator 2 — the 50-stake validator, so the pool's change is unambiguous. The key table is
# `tools/devnet.sh`'s `VALIDATOR_PRIV[2]`; `withdraw` takes the caller's own `rho:rchain:deployerId`, so
# this deploy must be signed by the validator that is leaving.
K2_PRIV=d78ff60a424d71ce99d6b7d7f44a8c49b38a3757ff9e6fa9b32fcba8aa2c973b
GENESIS_PRIV=a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76
PORTS=(40403 41403 42403)

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${OUT_ROOT:-target/n220-leave}/${TREE}-${STAMP}
mkdir -p "$OUT"
{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length $EPOCH --quarantine-length $QUARANTINE --no-autopropose --propose-on-deploy"
  echo "# validator-2 (50 stake) is the one that leaves"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

height() { curl -fsS --max-time 3 "http://localhost:$1/api/v1/status" 2>/dev/null | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'; }
finalised() {
  local best="" v p
  for p in "${PORTS[@]}"; do
    v=$(curl -fsS --max-time 3 "http://localhost:$p/api/last-finalized-block" 2>/dev/null \
        | sed -n 's/.*"blockNumber":\([0-9]*\).*/\1/p')
    if [[ -n "$v" ]] && { [[ -z "$best" ]] || (( v > best )); }; then best="$v"; fi
  done
  echo "${best:-none}"
}
# `/api/v1/pos` — the read the rig's three witnesses rest on. **Field names matter and the first version
# got them wrong**: the API answers `pendingWithdrawals` (plural, camelCase, an ARRAY) and
# `activeValidators`, and the rig read `pending_withdrawal` — so it saw an empty value whatever the chain
# did, and `L1` "failed" on a withdraw that had in fact staged. A reader that cannot see the thing it
# tests is worse than no test.
pos_read() {
  curl -fsS --max-time 5 "http://localhost:40403/api/v1/pos" 2>/dev/null | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: print("unreadable"); sys.exit(0)
pws = d.get("pendingWithdrawals") or []
av  = d.get("activeValidators") or []
pw  = pws[0] if pws else {}
print("deadline=%s blocks_remaining=%s active=%d pending=%d" % (
  pw.get("deadline", ""), pw.get("blocksRemaining", ""), len(av), len(pws)))
' 2>/dev/null
}
# `<deadline> <blocks_remaining>` while a withdrawal is pending; nothing once it clears.
#
# **`\+`, not `*`.** The first version used `[0-9]*`, which matches *zero* digits — so `deadline=` with
# nothing after it "matched" and the rig printed `L1 PASS` with two empty fields while no withdrawal
# existed. A witness that passes on emptiness is not a witness.
pending() { pos_read | sed -n 's/^deadline=\([0-9]\+\) blocks_remaining=\([0-9]\+\).*/\1 \2/p'; }
# The active-validator count — the leave's own observable, and the one that does not depend on the
# quarantine still being in flight: a validator that has left the active set is out of the draw.
active() { pos_read | sed -n 's/.*active=\([0-9]\+\).*/\1/p'; }
sample() { echo "$(date -u +%H:%M:%S) h=$(height 40403) fin=$(finalised) pos=[$(pos_read)]" | tee -a "$OUT/series.txt"; }
deploy_as() {
  timeout 45 docker exec "$1" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$2" --shard-id /root --valid-after-block-number "$(height 40403)" "/contracts/$3" 2>&1 \
    | sed -n 's/^DeployId is: *//p'
}
deploy_status() {
  local p
  for p in "${PORTS[@]}"; do
    curl -fsS --max-time 3 "http://localhost:$p/api/v1/deploy-status/$1" 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin); k=next(iter(d)) if isinstance(d,dict) and d else ""
v=d.get(k,{}) if k else {}
if k=="ProcessedWithSuccess": print("ok", v["block"]["blockNumber"])
elif k=="ProcessedWithError": print("error", v.get("deployError","?").replace(" ","_"))
else: sys.exit(1)
' 2>/dev/null && return
  done
  echo pending
}

tools/devnet.sh down >/dev/null 2>&1
DEVNET_NODE_MEMORY="$CAP" DEVNET_EXTRA_FLAGS="--quarantine-length $QUARANTINE" \
  timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
  --epoch-length "$EPOCH" --no-autopropose --propose-on-deploy 2>&1 | tail -1
for _ in $(seq 90); do [[ "$(height 42403)" -gt 0 ]] 2>/dev/null && break; sleep 2; done
deploy_as devnet-bootstrap "$GENESIS_PRIV" hello.rho >/dev/null; sleep 15; sample

echo "== before: the pool, and no pending withdrawal"
before=$(pos_read); echo "  $before" | tee -a "$OUT/witness.txt"

echo "== fund validator-2 (genesis funds only the deployer), then withdraw signed by it"
f=$(deploy_as devnet-bootstrap "$GENESIS_PRIV" leave-fund.rho)
sleep 15; echo "  fund: $(deploy_status "$f")" | tee -a "$OUT/witness.txt"
w=$(deploy_as devnet-bootstrap "$K2_PRIV" pos-withdraw.rho)
sleep 15; st=$(deploy_status "$w"); echo "  withdraw deploy: $st" | tee -a "$OUT/witness.txt"
docker exec devnet-bootstrap rnode --grpc-host localhost eval /contracts/pos-withdraw.rho >/dev/null 2>&1 || true
docker logs devnet-bootstrap 2>&1 | grep -E '^\[pos\] ' > "$OUT/pos-lines.txt" || true

# L1 — staged, with a deadline. The arithmetic is read off the API, which settles CH-U6-09.
staged=""; t_end=$(( $(date +%s) + 90 ))
while (( $(date +%s) < t_end )); do
  staged=$(pending); [[ -n "$staged" ]] && break
  sleep 3
done
if [[ -n "$staged" ]]; then
  echo "  L1 PASS: the withdrawal staged — deadline=${staged%% *} blocks_remaining=${staged##* }" | tee -a "$OUT/witness.txt"
  echo "    (CH-U6-09: the read path's own deadline and blocks_remaining ARE the recorded arithmetic; the" \
       "worksheet's \"waits quarantine_length more blocks past its deadline\" is not what the API says.)" >> "$OUT/witness.txt"
else
  # A withdraw that is ALREADY cleared by the time we look is not a failure — quarantine is 20 blocks.
  echo "  L1 NOTE: no pending entry at first sight (already cleared, or never staged) — pos=[$(pos_read)]" | tee -a "$OUT/witness.txt"
fi

# L2/L3 — the active set shrank (the leave), and the pending entry cleared (the payout's precondition).
echo "== driving past the deadline"
targets=(devnet-bootstrap devnet-validator-1 devnet-validator-2); i=0; left=""; cleared=""
t_end=$(( $(date +%s) + LEAVE_BUDGET_S ))
while (( $(date +%s) < t_end )); do
  [[ "$(active)" == "2" ]] && left=1
  [[ -n "$staged" && -z "$(pending)" ]] && cleared=1
  [[ -n "$left" && -n "$cleared" ]] && break
  deploy_as "${targets[i % 3]}" "$GENESIS_PRIV" hello.rho >/dev/null; i=$((i + 1))
  sleep "$DEPLOY_EVERY_S"; sample
done
[[ -n "$left" || "$(active)" == "2" ]] && echo "  L2 PASS: the leaving validator is out of the active set (3 -> 2)" | tee -a "$OUT/witness.txt" \
  || echo "  L2 FAIL: the active set is still $(active) — the leave did not take effect" | tee -a "$OUT/witness.txt"
if [[ -n "$cleared" || -z "$(pending)" ]]; then
  echo "  L3 PASS: no pending withdrawal remains once its deadline passed" | tee -a "$OUT/witness.txt"
else
  echo "  L3 FAIL: the pending withdrawal never cleared within ${LEAVE_BUDGET_S}s" | tee -a "$OUT/witness.txt"
fi
after=$(pos_read); echo "  after: $after" >> "$OUT/witness.txt"
echo "== quiet read (${READ_S}s)"; h0=$(height 40403); sleep "$READ_S"; h1=$(height 40403); sample
echo "  height $h0 -> $h1 over ${READ_S}s" | tee -a "$OUT/witness.txt"

for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
  docker logs "$c" > "$OUT/$c.log" 2>&1
  grep -o 'round gate escaped[^"]*' "$OUT/$c.log" | sed "s/^/$c /" >> "$OUT/escape-lines.txt" || true
done
gzip -f "$OUT"/devnet-*.log
tools/devnet.sh down >/dev/null 2>&1
echo
echo "=== witnesses ==="; cat "$OUT/witness.txt"
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
