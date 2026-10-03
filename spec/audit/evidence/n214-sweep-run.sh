#!/usr/bin/env bash
# n214 — re-probe criteria 1 and 2 of issue #214 on the CURRENT tip.
#
# The protocol is `n214-preregistration.md`, frozen before this ran. It exists because the first edition
# of the acceptance page measured `1e5a64ed4` from an unfetched clone and so missed the landed fix for
# #213 (#215) and the tree criterion 1 cites. Two departures from the original audit's method, both in
# the pre-registration and both here:
#
#   * criterion 1 uses TWO marks — a second deploy must finalise — because a single deploy's bounded
#     production is satisfied *by construction* by the very wedge criterion 2 measures;
#   * the block count comes from the block-hash union (`blocks.tsv`), never from `latestBlockNumber`,
#     because several blocks share a height.
#
# Usage:  spec/audit/evidence/n214-sweep-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
SETTLE_S=${SETTLE_S:-60}
FINALITY_S=${FINALITY_S:-90}
KILL_S=${KILL_S:-120}
RECOVER_S=${RECOVER_S:-90}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}
STOP_NODE=${STOP_NODE:-2}          # the 50-stake validator: 100+100 of 250 is 80%

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT_ROOT=${OUT_ROOT:-target/n214}
OUT="$OUT_ROOT/${TREE}-${STAMP}"
mkdir -p "$OUT"

# The image is the thing under test. Rebuild it from the recorded tree, and record the tree it came
# from — a run against a stale `rnode:local` is the exact error this re-probe exists to correct.
echo "=== building rnode:local from $TREE (HEAD=$(git rev-parse HEAD)) ==="
tools/devnet.sh build 2>&1 | tail -3

{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rust_diff_vs_HEAD=$([ -n "$(git diff --stat HEAD -- '*.rs' 2>/dev/null)" ] && echo 'NON-EMPTY' || echo 'empty')"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# nodes=3 stakes=100,100,50 argv='--no-autopropose --propose-on-deploy --epoch-length 10' (--autopropose omitted)"
  echo "# windows: settle=${SETTLE_S}s finality=${FINALITY_S}s kill=${KILL_S}s recover=${RECOVER_S}s cap=$CAP"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

wait_until() { local d=$(( $1 - $(date +%s) )); (( d > 0 )) && sleep "$d" || true; }

# Bring up a fresh 3-validator net and wait until all three are past genesis. `latestBlockNumber` is
# max_height + 1, so genesis alone reports 1 — wait for that, not for the HTTP server.
bring_up() {
  local dir="$1"
  tools/devnet.sh down >/dev/null 2>&1
  # `--attest-on-new-blocks` is REQUIRED: the tap is gated on `conf.attest_on_new_blocks &&
  # !conf.no_attest_on_new_blocks` and the positive flag defaults false, so without it the node never
  # attests, the fringe never advances and nothing finalises at any N. The option doc says "attestation
  # is on by default"; the code disagrees. #213's and #214's configurations both carry the flag.
  DEVNET_NODE_MEMORY="$CAP" DEVNET_EXTRA_FLAGS="--attest-on-new-blocks" \
    timeout 900 tools/devnet.sh up --validators 3 --fresh \
    --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy 2>&1 | tail -1
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
  if (( joined < 3 )); then
    echo "  VOID: $joined of 3 nodes committed genesis within ${JOIN_TIMEOUT}s" | tee "$dir/void.txt"
    tools/devnet.sh down >/dev/null 2>&1
    return 1
  fi
  return 0
}

collect_logs() {
  local dir="$1"
  for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
    docker logs "$c" 2>&1 | grep -E ' (WARN|ERROR) ' >> "$dir/logs-${c}.txt" || true
  done
}

# ---------------------------------------------------------------- arm A: criterion 1, two marks
arm_a() {
  local dir="$OUT/n214-a"
  mkdir -p "$dir"
  echo "=================== arm A — criterion 1 (two marks)  $(date -u +%H:%M:%S) UTC =============="
  if ! bring_up "$dir"; then return; fi
  local t0; t0=$(date +%s)
  echo "  3 of 3 past genesis at t0"

  N149_N=3 N149_WINDOW_S=$(( SETTLE_S + 2 * FINALITY_S + 30 )) \
    python3 spec/audit/evidence/n149-sample.py "$dir" > "$dir/sampler.log" 2>&1 &
  local sampler=$!

  wait_until $(( t0 + SETTLE_S ))
  local d1; d1=$(date +%s)
  if timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1; then
    echo "  deploy #1 at T+$(( d1 - t0 ))s"
  else
    echo "  deploy #1 FAILED — VOID" | tee "$dir/void.txt"
  fi

  wait_until $(( d1 + FINALITY_S ))
  local d2; d2=$(date +%s)
  if timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1; then
    echo "  deploy #2 at T+$(( d2 - t0 ))s"
  else
    echo "  deploy #2 FAILED — VOID" | tee "$dir/void.txt"
  fi

  wait_until $(( d2 + FINALITY_S ))
  local end; end=$(date +%s)
  printf 'arm\tA\nt0\t%s\ndeploy1\t%s\ndeploy2\t%s\nend\t%s\n' \
    "$t0" "$d1" "$d2" "$end" > "$dir/marks.tsv"

  wait "$sampler"
  collect_logs "$dir"
  tools/devnet.sh down >/dev/null 2>&1
  echo "  arm A done at $(date -u +%H:%M:%S) UTC"
}

# ---------------------------------------------------------------- arm B: criterion 2, kill / restart
arm_b() {
  local dir="$OUT/n214-b"
  mkdir -p "$dir"
  echo "=================== arm B — criterion 2 (kill/restart)  $(date -u +%H:%M:%S) UTC ==========="
  if ! bring_up "$dir"; then return; fi
  local t0; t0=$(date +%s)
  echo "  3 of 3 past genesis at t0"

  N149_N=3 N149_WINDOW_S=$(( SETTLE_S + 30 + KILL_S + RECOVER_S + 30 )) \
    python3 spec/audit/evidence/n149-sample.py "$dir" > "$dir/sampler.log" 2>&1 &
  local sampler=$!

  wait_until $(( t0 + SETTLE_S ))
  local dep; dep=$(date +%s)
  timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" >/dev/null 2>&1 \
    && echo "  pre-kill deploy at T+$(( dep - t0 ))s" \
    || echo "  pre-kill deploy failed (not fatal for arm B)"

  wait_until $(( t0 + SETTLE_S + 30 ))
  local kill; kill=$(date +%s)
  echo "  stopping validator-$STOP_NODE (the 50-stake validator) at T+$(( kill - t0 ))s"
  tools/devnet.sh stop "$STOP_NODE" || echo "  STOP FAILED — VOID" | tee "$dir/void.txt"

  wait_until $(( kill + KILL_S ))
  local kill_end; kill_end=$(date +%s)
  local restart; restart=$(date +%s)
  echo "  restarting validator-$STOP_NODE at T+$(( restart - t0 ))s"
  tools/devnet.sh start "$STOP_NODE" || echo "  START FAILED — VOID" | tee "$dir/void.txt"

  wait_until $(( restart + RECOVER_S ))
  local rec_end; rec_end=$(date +%s)
  printf 'arm\tB\nt0\t%s\nsettle_deploy\t%s\nkill\t%s\nkill_end\t%s\nrestart\t%s\nrecover_end\t%s\nstopped_node\tv%s\n' \
    "$t0" "$dep" "$kill" "$kill_end" "$restart" "$rec_end" "$STOP_NODE" > "$dir/marks.tsv"

  wait "$sampler"
  collect_logs "$dir"
  tools/devnet.sh down >/dev/null 2>&1
  echo "  arm B done at $(date -u +%H:%M:%S) UTC"
}

arm_a
arm_b

echo
echo "=== the pre-registered readings, computed by the committed program ==="
python3 spec/audit/evidence/n214-summarise.py "$OUT" | tee "$OUT/readings.txt"
echo
echo "run root: $OUT   (commit it: spec/audit/evidence/ is the home for a run's artefacts)"
