#!/usr/bin/env bash
# n214-rotation — the trigger-pattern experiment for the all-live finality stall.
#
# Issue #214 records the one manipulation ever seen to turn the stall off: six deploys to validator A
# left finality at `none`, while triggering the validators **in rotation** took it none -> f=1 -> f=9 ->
# f=12 and running. That observation is the lead this script turns into a measurement.
#
# It was a direct test of the pre-C209 suppression mechanism: `new_state_transition` was true only when
# an immediate parent carried a deploy, so one round after a deploy every validator was paced to a cadence
# that never came due on a chain that was not moving. **C209 (2026-10-04) changed the guard to read every
# input from `latest_msgs`** — what the node has seen — bounded by `ATTESTATION_HORIZON`; this script is
# kept as it was run, and its three arms are the before/after pair that distinguished the two rules.
#
# Usage:  spec/audit/evidence/n214-rotation-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
SETTLE_S=${SETTLE_S:-60}
SPACING_S=${SPACING_S:-12}
DEPLOYS=${DEPLOYS:-6}
READ_S=${READ_S:-90}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT_ROOT=${OUT_ROOT:-target/n214-rotation}
OUT="$OUT_ROOT/${TREE}-${STAMP}"
mkdir -p "$OUT"

echo "=== building rnode:local from $TREE ==="
tools/devnet.sh build 2>&1 | tail -2

{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy"
  echo "#      DEVNET_EXTRA_FLAGS=--attest-on-new-blocks"
  echo "# deploys: $DEPLOYS every ${SPACING_S}s; settle=${SETTLE_S}s read=${READ_S}s"
  echo "# arms: R1 all to bootstrap | R2 rotating 0,1,2 | R3 all to validator-1"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

wait_until() { local d=$(( $1 - $(date +%s) )); (( d > 0 )) && sleep "$d" || true; }

bring_up() {
  local dir="$1"
  tools/devnet.sh down >/dev/null 2>&1
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
    echo "  VOID: $joined of 3 past genesis" | tee "$dir/void.txt"; return 1
  fi
  return 0
}

capture() {
  local dir="$1"
  for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
    docker logs "$c" 2>&1 | grep -o 'finality did not advance at tip [0-9]*: .*' >> "$dir/stall-lines.txt" || true
    docker logs "$c" 2>&1 | grep -o 'round gate escaped[^"]*' >> "$dir/escape-lines.txt" || true
  done
}

# arm <name> <target sequence: space-separated node indices cycled across the deploys>
arm() {
  local name="$1" targets="$2"
  local dir="$OUT/$name"; mkdir -p "$dir"
  echo "=================== $name (targets: $targets)  $(date -u +%H:%M:%S) UTC ==========="
  if ! bring_up "$dir"; then return; fi
  local t0; t0=$(date +%s)

  N149_N=3 N149_WINDOW_S=$(( SETTLE_S + DEPLOYS * SPACING_S + READ_S + 30 )) \
    python3 spec/audit/evidence/n149-sample.py "$dir" > "$dir/sampler.log" 2>&1 &
  local sampler=$!

  wait_until $(( t0 + SETTLE_S ))
  local d0; d0=$(date +%s)
  local i=0
  # Target `-` is the bootstrap. It cannot be addressed by number: `validator_name 0` is
  # `devnet-validator-0`, which does not exist (`--validators 3` makes `devnet-bootstrap`,
  # `devnet-validator-1`, `devnet-validator-2`), and `node_container` passes `0` straight through with no
  # guard — so `--to 0` silently deploys nowhere. The bootstrap is reached by *omitting* `--to`, which is
  # what `cmd_deploy`'s default does.
  for t in $targets; do
    local where="bootstrap" args=()
    [[ "$t" != "-" ]] && { where="validator-$t"; args=(--to "$t"); }
    timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" "${args[@]}" >/dev/null 2>&1 \
      && echo "  deploy $((i+1)) -> $where at T+$(( $(date +%s) - t0 ))s" \
      || echo "  deploy $((i+1)) -> $where FAILED"
    i=$((i + 1))
    wait_until $(( d0 + i * SPACING_S ))
  done
  local d_end; d_end=$(date +%s)
  wait_until $(( d_end + READ_S ))
  local end; end=$(date +%s)
  printf 'arm\t%s\nt0\t%s\nfirst_deploy\t%s\nlast_deploy\t%s\nend\t%s\ntargets\t%s\n' \
    "$name" "$t0" "$d0" "$d_end" "$end" "$targets" > "$dir/marks.tsv"

  wait "$sampler"
  capture "$dir"
  tools/devnet.sh down >/dev/null 2>&1
  echo "  $name done at $(date -u +%H:%M:%S) UTC"
}

arm R1 "- - - - - -"
arm R2 "- 1 2 - 1 2"
arm R3 "1 1 1 1 1 1"

echo
echo "=== finality over each arm (any numeric value at all?) ==="
for a in R1 R2 R3; do
  n=$(grep -v '^#' "$OUT/$a/series.tsv" 2>/dev/null | awk -F'\t' '$5!="none" && $5!=""' | wc -l)
  last=$(grep -v '^#' "$OUT/$a/series.tsv" 2>/dev/null | awk -F'\t' '$5!="none" && $5!=""{print $5}' | tail -1)
  echo "  $a: $n numeric finality samples; last finalised = ${last:-none}"
done
echo
echo "=== the discriminator, per arm ==="
for a in R1 R2 R3; do
  echo "--- $a"; sort -u "$OUT/$a/stall-lines.txt" 2>/dev/null | sed 's/^/    /' || true
  echo "    escapes: $(wc -l < "$OUT/$a/escape-lines.txt" 2>/dev/null || echo 0)"
done
echo
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
