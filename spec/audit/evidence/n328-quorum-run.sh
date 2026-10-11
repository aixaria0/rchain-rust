#!/usr/bin/env bash
# n328-quorum — the post-mesh `quorum_reachable` reading (#315's second clause; item 3 of #328).
#
# **The clause, and why it was unread.** #315's close condition asked for *"no `quorum_reachable=false`
# persisting after the mesh is complete"*, and the two runs that closed the issue captured the **API**
# surface — finality advancing past the kill, every validator attesting. `quorum_reachable` is not on
# that surface: it is computed in the proposer (`proposer.rs`) and rendered only into a log line
# (`attestation at tip N: … quorum_reachable=…`), rate-limited to fire on change and then every 100
# tips. So the clause is a reading of a proposer-side line, and nothing had read it.
#
# **Why this rig and not a fresh autopropose net.** The line only exists when the proposer considers
# attesting, so the chain has to produce — and a fresh four-validator autopropose net **self-diverges**
# into the §92 self-reject stall (`n-anchor-drill/self-reject-stall.txt`), which is a different
# measurement. So this is the n213/n214 shape: three validators, `--no-autopropose
# --propose-on-deploy`, and deploys to drive production.
#
# **The acceptance.** Once the mesh is complete (all three nodes producing at one height, finality
# advancing past genesis), every collected `attestation at tip …` line reads `quorum_reachable=true`.
# Any early `false` is reported rather than hidden, and named against `n213-results.md`'s recorded
# bootstrap reading (the first cut read `false` on the genesis round, which is not a mesh state).
#
# Usage: spec/audit/evidence/n328-quorum-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
RUN_BUDGET_S=${RUN_BUDGET_S:-360}
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}
SETTLE_S=${SETTLE_S:-45}

PORT_OF() { echo $(( 40403 + $1 * 1000 )); }
OUT=${OUT:-spec/audit/evidence/n328-quorum}
mkdir -p "$OUT"

TREE=$(git rev-parse --short HEAD)
HEAD_FULL=$(git rev-parse HEAD)

height() {
  curl -fsS --max-time 3 "http://localhost:$(PORT_OF "${1:-0}")/api/v1/status" 2>/dev/null \
    | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'
}
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

{
  echo "# n328-quorum — the post-mesh quorum_reachable reading (#315's clause, #328 item 3)"
  echo "# tree=$TREE head=$HEAD_FULL"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/run.txt"

tools/devnet.sh down -v >/dev/null 2>&1
DEVNET_NODE_MEMORY="$CAP" timeout 900 tools/devnet.sh up --validators 3 --fresh \
  --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy \
  >> "$OUT/run.txt" 2>&1

echo "== 1. the mesh (all three past genesis) ==" | tee -a "$OUT/run.txt"
deadline=$(( $(date +%s) + JOIN_TIMEOUT )); joined=0
while (( $(date +%s) < deadline )); do
  joined=0
  for i in 0 1 2; do
    bn=$(height "$i")
    [[ -n "$bn" && "$bn" -gt 0 ]] && joined=$((joined + 1))
  done
  (( joined >= 3 )) && break
  sleep 2
done
echo "  $joined of 3 past genesis; heights $(height 0) $(height 1) $(height 2); finalised $(finalised)" \
  | tee -a "$OUT/run.txt"
if (( joined < 3 )); then
  echo "  VOID: the mesh never completed, so nothing below is a post-mesh reading" | tee -a "$OUT/run.txt"
fi

# **Production, and the mesh's steady state.** A deploy per round keeps blocks flowing; finality has to
# pass genesis before the run is a post-mesh reading at all.
echo "== 2. production and finality ==" | tee -a "$OUT/run.txt"
deadline=$(( $(date +%s) + RUN_BUDGET_S )); deploy_ok=0
while (( $(date +%s) < deadline )); do
  if tools/devnet.sh deploy "$DEPLOY_FILE" >>"$OUT/run.txt" 2>&1; then
    deploy_ok=$((deploy_ok + 1))
  fi
  f=$(finalised)
  echo "  t+$(( RUN_BUDGET_S - (deadline - $(date +%s)) ))s heights=$(height 0)/$(height 1)/$(height 2) finalised=$f deploys=$deploy_ok" \
    | tee -a "$OUT/run.txt"
  if [[ "$f" != "none" && "$f" -gt 0 ]] && (( $(height 0) > 6 )); then
    break
  fi
  sleep 10
done

# Let the steady state settle before reading, so the tail is a mesh that has been complete for a while
# rather than the moment it completed.
sleep "$SETTLE_S"

echo "== 3. the attestation lines, from all three nodes ==" | tee -a "$OUT/run.txt"
: > "$OUT/attestation-lines.txt"
for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
  docker logs "$c" 2>&1 | grep -o 'attestation at tip [0-9]*: [^"]*' >> "$OUT/attestation-lines.txt" || true
done

total=$(wc -l < "$OUT/attestation-lines.txt")
fals=$(grep -c 'quorum_reachable=false' "$OUT/attestation-lines.txt" || true)
trues=$(grep -c 'quorum_reachable=true' "$OUT/attestation-lines.txt" || true)
{
  echo "  lines=$total quorum_reachable=true:$trues false:$fals"
  echo "  final heights=$(height 0)/$(height 1)/$(height 2) finalised=$(finalised)"
  echo "== 4. every line that reads false, in full =="
  grep 'quorum_reachable=false' "$OUT/attestation-lines.txt" || echo "  (none)"
  echo "== 5. the last eight lines — the steady state =="
  tail -8 "$OUT/attestation-lines.txt"
} | tee -a "$OUT/run.txt"

echo "== the rig's own state (left up for inspection; 'tools/devnet.sh down -v' when done) ==" \
  | tee -a "$OUT/run.txt"
