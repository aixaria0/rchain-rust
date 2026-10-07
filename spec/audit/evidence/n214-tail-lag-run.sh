#!/usr/bin/env bash
#
# n214-tail-lag-run.sh — is A1.5's unfinalised tail a **lag** or a **loss**?
#
# A1.5 ("every consecutive deploy on a quiet net is finalised") fails, and the nine arms of the three
# stored runs say why: the last few heights of an idle chain are never finalised. `n214-rotation-run.sh`
# measures that wall; this rig measures what happens to it once the chain is given a reason to produce.
#
# **The two readings, and the one that matters.**
#   * the WALL: the greatest `finalized` while the chain is quiet. Expected a few heights below the tip.
#   * the LAG: the greatest `finalized` after **one more deploy**. If the sixth deploy's block is at or
#     below *this*, the tail was a delay rather than a loss — the chain had not finalised its own tail
#     because nothing had asked it to produce, and a single further block settled it.
#
# A `LOSS` is the defect A1.5 exists to catch: a deploy included in the DAG that the chain never
# commits, no matter what it is later asked to do.
#
# Usage: spec/audit/evidence/n214-tail-lag-run.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
cd "$REPO"

CAP=${CAP:-8g}
SETTLE_S=${SETTLE_S:-60}
SPACING_S=${SPACING_S:-12}
DEPLOYS=${DEPLOYS:-6}
READ_S=${READ_S:-90}
READ2_S=${READ2_S:-90}
JOIN_TIMEOUT=${JOIN_TIMEOUT:-180}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}
# **The shape matters.** The three stored runs left their sixth deploy unfinalised in the arms that sent
# every deploy to *one* validator (`R3`, `1 1 1 1 1 1`) — 2 of 3 of them — and rarely in the arms that
# sent them to the bootstrap, which leaves more slack. The default here is the shape that goes short, so
# the probe is likely to have something in the tail to rescue.
TARGETS=${TARGETS:-"1 1 1 1 1 1"}

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT_ROOT=${OUT_ROOT:-target/n214-tail-lag}
OUT="$OUT_ROOT/${TREE}-${STAMP}"
mkdir -p "$OUT"

echo "=== building rnode:local from $TREE ==="
tools/devnet.sh build 2>&1 | tail -2

{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy"
  echo "#      DEVNET_EXTRA_FLAGS=--attest-on-new-blocks"
  echo "# arms: R1 all to bootstrap | R2 rotating 0,1,2 | R3 all to validator-1, as n214-rotation-run.sh"
  echo "# phase 1: $DEPLOYS deploys every ${SPACING_S}s, settle=${SETTLE_S}s"
  echo "# phase 2: quiet for ${READ_S}s  — the WALL is read at the end of this"
  echo "# phase 3: one more deploy       — the LAG is read after a further ${READ2_S}s"
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

# arm <name> <target sequence> — the six deploys, the quiet read, then the probe.
arm() {
  local name="$1" targets="$2"
  local dir="$OUT/$name"; mkdir -p "$dir"
  echo "=================== $name (targets: $targets)  $(date -u +%H:%M:%S) UTC ==========="
  if ! bring_up "$dir"; then return; fi
  local t0; t0=$(date +%s)

  N149_N=3 N149_WINDOW_S=$(( SETTLE_S + DEPLOYS * SPACING_S + READ_S + READ2_S + 30 )) \
    python3 spec/audit/evidence/n149-sample.py "$dir" > "$dir/sampler.log" 2>&1 &
  local sampler=$!

  # --- phase 1: the six deploys -----------------------------------------------------------------
  wait_until $(( t0 + SETTLE_S ))
  local d0; d0=$(date +%s)
  local i=0 t
  for t in $targets; do
    i=$((i + 1))
    local args=()
    [[ "$t" != "-" ]] && args=(--to "$t")
    timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" "${args[@]}" >/dev/null 2>&1 \
      && echo "  deploy $i -> ${t/-/bootstrap} at T+$(( $(date +%s) - t0 ))s" \
      || echo "  deploy $i -> ${t/-/bootstrap} FAILED"
    wait_until $(( d0 + i * SPACING_S ))
  done
  local d_end; d_end=$(date +%s)

  # --- phase 2: go quiet, and read the WALL ------------------------------------------------------
  wait_until $(( d_end + READ_S ))
  local quiet_end; quiet_end=$(date +%s)

  # --- phase 3: one more deploy, and read the LAG ------------------------------------------------
  timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy "$DEPLOY_FILE" --to 1 >/dev/null 2>&1 \
    && echo "  deploy $(( DEPLOYS + 1 )) (the tail probe) at T+$(( $(date +%s) - t0 ))s" \
    || echo "  deploy $(( DEPLOYS + 1 )) (the tail probe) FAILED"
  local d7; d7=$(date +%s)
  wait_until $(( d7 + READ2_S ))
  local end; end=$(date +%s)

  printf 'arm\t%s\nt0\t%s\nfirst_deploy\t%s\nlast_deploy\t%s\nquiet_end\t%s\nseventh_deploy\t%s\nend\t%s\ntargets\t%s 1\n' \
    "$name" "$t0" "$d0" "$d_end" "$quiet_end" "$d7" "$end" "$targets" > "$dir/marks.tsv"

  wait "$sampler"
  capture "$dir"
  tools/devnet.sh down >/dev/null 2>&1
  echo "  $name done at $(date -u +%H:%M:%S) UTC"
}

# The three shapes the stored runs used. `-` is the bootstrap (a numeric 0 silently deploys nowhere);
# R1 leaves the most slack, R3 the least, and R2 sits between them.
for spec in "R1:- - - - - -" "R2:- 1 2 - 1 2" "R3:1 1 1 1 1 1"; do
  arm "${spec%%:*}" "${spec#*:}"
done

echo "artifacts: $OUT"
