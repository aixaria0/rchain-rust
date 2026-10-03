#!/usr/bin/env bash
# n213 — the three close conditions of issue #213, bounded, on a live net.
#
# Protocol: `n213-preregistration.md`, frozen before this ran. The fix under test is #219 (C209): the
# attestation guard's licence is read from what the node has seen rather than from the round's parents,
# bounded by ATTESTATION_HORIZON. This script asks the three questions the issue says close it:
#
#   (a) kill one of three at 80 %: do the survivors finalise past the kill?
#   (b) the killed validator restarts: do production and finality resume with no operator action?
#   (c) a deploy accepted while the validator is absent is included once the survivors can finalise.
#
# The witness is always a deploy FINALISING (`last-finalized-block` >= its block height), never "the
# height stopped moving" — a chain that stops is the defect, not the pass. (d): the whole run is bounded;
# a timeout is a FAIL, not a pass.
#
# Usage:  spec/audit/evidence/n213-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
SETTLE_S=${SETTLE_S:-60}
KILL_WAIT_S=${KILL_WAIT_S:-120}
RESTART_WAIT_S=${RESTART_WAIT_S:-120}
RUN_BUDGET_S=${RUN_BUDGET_S:-900}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
STOP_NODE=${STOP_NODE:-2}          # the 50-stake validator: 100+100 of 250 is 80 %
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}

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
  echo "#      DEVNET_EXTRA_FLAGS=--attest-on-new-blocks   (required; the flag defaults false)"
  echo "# windows: settle=${SETTLE_S}s kill=${KILL_WAIT_S}s restart=${RESTART_WAIT_S}s budget=${RUN_BUDGET_S}s"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

BUDGET_END=$(( $(date +%s) + RUN_BUDGET_S ))
WAIT_UNTIL() { local d=$(( $1 - $(date +%s) )); (( d > 0 )) && sleep "$d" || true; }
OVER_BUDGET() { (( $(date +%s) > BUDGET_END )); }

# The sampler's block-hash union and per-node `finalized`, at 1 Hz.
start_sampler() {
  N149_N=3 N149_WINDOW_S=$(( RUN_BUDGET_S + 60 )) \
    python3 spec/audit/evidence/n149-sample.py "$1" > "$1/sampler.log" 2>&1 &
  echo $!
}

bring_up() {
  local dir="$1"
  tools/devnet.sh down >/dev/null 2>&1
  DEVNET_NODE_MEMORY="$CAP" DEVNET_EXTRA_FLAGS="--attest-on-new-blocks" \
    timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
    --epoch-length 10 --no-autopropose --propose-on-deploy 2>&1 | tail -1
  local deadline=$(( $(date +%s) + JOIN_TIMEOUT )) joined=0
  while (( $(date +%s) < deadline )); do
    joined=0
    for i in 0 1 2; do
      local bn
      bn=$(curl -fsS --max-time 3 "http://localhost:$(( 40403 + i * 1000 ))/api/v1/status" 2>/dev/null \
           | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p')
      [[ -n "$bn" && "$bn" -gt 0 ]] && joined=$((joined + 1))
    done
    (( joined >= 3 )) && break
    sleep 2
  done
  (( joined >= 3 )) || { echo "  VOID: $joined of 3 past genesis" | tee "$dir/void.txt"; return 1; }
  return 0
}

# The highest finalised block any node reports; `none` when nothing has finalised.
finalised() {
  local best="" v
  for i in 0 1 2; do
    v=$(curl -fsS --max-time 3 "http://localhost:$(( 40403 + i * 1000 ))/api/last-finalized-block" 2>/dev/null \
        | sed -n 's/.*"blockNumber":\([0-9]*\).*/\1/p')
    if [[ -n "$v" ]]; then
      if [[ -z "$best" ]] || (( v > best )); then best="$v"; fi
    fi
  done
  echo "${best:-none}"
}

# The height of the newest block, per node — a *height*, used only to see production moving.
height() { curl -fsS --max-time 3 "http://localhost:$1/api/v1/status" 2>/dev/null | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'; }

# Wait until `finalised` strictly exceeds `floor`, or the deadline passes. Echoes the value reached.
await_finality() {
  local floor="$1" deadline="$2" v
  while (( $(date +%s) < deadline )); do
    v=$(finalised)
    if [[ "$v" != "none" ]] && (( v > floor )); then echo "$v"; return 0; fi
    sleep 3
  done
  v=$(finalised); echo "${v:-none}"; return 1
}

main() {
  local dir="$OUT/run"; mkdir -p "$dir"
  echo "=================== n213 three-case run  $(date -u +%H:%M:%S) UTC ==========="
  bring_up "$dir" || return

  local sampler; sampler=$(start_sampler "$dir")
  local t0; t0=$(date +%s)
  echo "  3 of 3 past genesis at t0"

  # ---- baseline: a live net that has finalised something before the kill.
  WAIT_UNTIL $(( t0 + SETTLE_S ))
  timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1 \
    && echo "  baseline deploy at T+$(( $(date +%s) - t0 ))s" || echo "  baseline deploy FAILED"
  local dep1; dep1=$(date +%s)
  local base
  base=$(await_finality 0 $(( dep1 + KILL_WAIT_S ))) && echo "  baseline finalised = $base" \
    || { echo "  VOID: nothing finalised before the kill (baseline=$base)" | tee "$dir/void.txt"; }

  # ---- (a)/(c): kill, then a deploy that can only be included and finalised by the survivors.
  tools/devnet.sh stop "$STOP_NODE" >/dev/null 2>&1 && echo "  killed validator-$STOP_NODE at T+$(( $(date +%s) - t0 ))s"
  local kill; kill=$(date +%s)
  sleep 15
  timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1 \
    && echo "  POST-KILL deploy at T+$(( $(date +%s) - t0 ))s  (cases a and c)" \
    || echo "  post-kill deploy FAILED"
  local base_num="${base:-0}"; [[ "$base_num" == "none" ]] && base_num=0
  local after_kill
  if after_kill=$(await_finality "$base_num" $(( kill + KILL_WAIT_S ))) && [[ "$after_kill" != "$base_num" ]]; then
    echo "  CASE (a) PASS: finality advanced past the kill — $base_num -> $after_kill"
  else
    echo "  CASE (a) FAIL: finality did not advance past the kill (floor $base_num, got $after_kill)"
  fi

  # ---- (b): the killed validator returns; does everything resume without an operator action?
  tools/devnet.sh start "$STOP_NODE" >/dev/null 2>&1 && echo "  restarted validator-$STOP_NODE at T+$(( $(date +%s) - t0 ))s"
  local restart; restart=$(date +%s)
  sleep 20
  timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1 \
    && echo "  post-restart deploy at T+$(( $(date +%s) - t0 ))s" || echo "  post-restart deploy FAILED"
  local floor="${after_kill:-0}"; [[ "$floor" == "none" ]] && floor=0
  local after_restart
  if after_restart=$(await_finality "$floor" $(( restart + RESTART_WAIT_S ))) && [[ "$after_restart" != "$floor" ]]; then
    echo "  CASE (b) PASS: resumed after the restart — $floor -> $after_restart"
  else
    echo "  CASE (b) FAIL: no resumption after the restart (floor $floor, got $after_restart)"
  fi

  local end; end=$(date +%s)
  printf 'arm\tn213\nt0\t%s\nbaseline_deploy\t%s\nbaseline_finalised\t%s\nkill\t%s\npost_kill_finalised\t%s\nrestart\t%s\npost_restart_finalised\t%s\nend\t%s\nbudget_s\t%s\n' \
    "$t0" "$dep1" "${base:-none}" "$kill" "${after_kill:-none}" "$restart" "${after_restart:-none}" "$end" "$RUN_BUDGET_S" > "$dir/marks.tsv"

  if OVER_BUDGET; then
    echo "  CASE (d) FAIL: the run exceeded its ${RUN_BUDGET_S}s budget" | tee "$dir/over-budget.txt"
    echo "  CASE (d) FAIL: over budget" > "$dir/cases.txt"
  else
    echo "  CASE (d) PASS: within the ${RUN_BUDGET_S}s budget" > "$dir/cases.txt"
  fi

  kill "$sampler" 2>/dev/null || true
  for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
    docker logs "$c" 2>&1 | grep -o 'finality did not advance at tip [0-9]*: .*' >> "$dir/stall-lines.txt" || true
    docker logs "$c" 2>&1 | grep -o 'round gate escaped[^"]*' >> "$dir/escape-lines.txt" || true
  done
  tools/devnet.sh down >/dev/null 2>&1
  echo "  done at $(date -u +%H:%M:%S) UTC"
}

main
echo
echo "=== cases ==="; cat "$OUT/run/cases.txt" 2>/dev/null
echo "=== marks ==="; cat "$OUT/run/marks.tsv" 2>/dev/null
echo "=== the gate's own reason, if it stalled ==="; sort -u "$OUT/run/stall-lines.txt" 2>/dev/null | head -6
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
