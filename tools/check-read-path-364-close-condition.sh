#!/usr/bin/env bash
# Execute issue #364's observable close condition without inventing a threshold.
#
# This is intentionally an operator-side, read-only gate. It compares two loopback nodes on the same
# exact build, requires the read arm to return values on both, then checks the target median against an
# explicitly supplied "small factor". A build mismatch, unknown build provenance, missing response, or
# EMPTY expr is BLOCKED rather than silently treated as evidence for closure.
#
# Usage:
#   tools/check-read-path-364-close-condition.sh \
#     --max-factor 3 --samples 9 \
#     http://127.0.0.1:40403 http://127.0.0.1:43403
#
# Arguments are TARGET then CONTROL. Both must be loopback URLs: the point is to remove proxy/network
# ambiguity before comparing node-side behavior. The caller chooses --max-factor because issue #364
# says "within a small factor" but does not define that policy numerically; this tool must not invent it.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: check-read-path-364-close-condition.sh --max-factor N [--samples N] [--out DIR] TARGET CONTROL

TARGET and CONTROL must be loopback HTTP URLs (127.0.0.1 or localhost).
Exit 0: PASS      same exact build, both reads return values, target <= factor * control
Exit 1: FAIL      evidence is comparable but target exceeds the supplied factor
Exit 2: USAGE     invalid arguments
Exit 3: BLOCKED   evidence is not comparable or incomplete
EOF
}

blocked() {
  printf 'READ_PATH_364_ACCEPTANCE=BLOCKED\n'
  printf 'reason=%s\n' "$1"
  exit 3
}

SAMPLES=9
MAX_FACTOR=''
OUT_DIR=''

while [ $# -gt 0 ]; do
  case "$1" in
    --samples)
      [ $# -ge 2 ] || { usage; exit 2; }
      SAMPLES=$2
      shift 2
      ;;
    --max-factor)
      [ $# -ge 2 ] || { usage; exit 2; }
      MAX_FACTOR=$2
      shift 2
      ;;
    --out)
      [ $# -ge 2 ] || { usage; exit 2; }
      OUT_DIR=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      usage
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

[ $# -eq 2 ] || { usage; exit 2; }
TARGET=$1
CONTROL=$2

[[ "$SAMPLES" =~ ^[1-9][0-9]*$ ]] || { usage; exit 2; }
[[ "$MAX_FACTOR" =~ ^[0-9]+([.][0-9]+)?$ ]] || { usage; exit 2; }
awk -v f="$MAX_FACTOR" 'BEGIN { exit !(f >= 1.0) }' || { usage; exit 2; }

is_loopback() {
  [[ "$1" =~ ^http://(127\.0\.0\.1|localhost)(:[0-9]+)?/?$ ]]
}

is_loopback "$TARGET" || blocked "target must be a loopback HTTP URL so proxy/network cost is excluded"
is_loopback "$CONTROL" || blocked "control must be a loopback HTTP URL so proxy/network cost is excluded"

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PROBE="$ROOT/tools/probe-read-latency.sh"
[ -f "$PROBE" ] || blocked "tools/probe-read-latency.sh is missing"

if [ -z "$OUT_DIR" ]; then
  STAMP=$(date -u +%Y%m%dT%H%M%SZ)
  OUT_DIR="$ROOT/target/read-path-364-acceptance/$STAMP"
fi
mkdir -p "$OUT_DIR"

capture() {
  local label=$1 base=$2
  curl -fsS --max-time 20 "$base/version" > "$OUT_DIR/$label.version.txt" \
    || blocked "$label /version could not be captured"
  curl -fsS --max-time 20 "$base/api/status" > "$OUT_DIR/$label.status.json" \
    || blocked "$label /api/status could not be captured"
  curl -fsS --max-time 20 "$base/api/last-finalized-block" > "$OUT_DIR/$label.last-finalized.json" \
    || blocked "$label /api/last-finalized-block could not be captured"

  if ! bash "$PROBE" --samples "$SAMPLES" "$base" > "$OUT_DIR/$label.probe.txt"; then
    blocked "$label latency probe failed"
  fi

  # The census is useful attribution context if the node exposes it. It is not itself a PASS condition:
  # the DAG publishes these gauges on its observability tick, so an idle scrape can be stale. Preserve
  # the bytes instead of pretending freshness that this read-only gate cannot establish.
  if curl -fsS --max-time 20 "$base/metrics" > "$OUT_DIR/$label.metrics.txt"; then
    printf 'available\n' > "$OUT_DIR/$label.metrics-capture.txt"
  else
    printf 'unavailable\n' > "$OUT_DIR/$label.metrics-capture.txt"
    : > "$OUT_DIR/$label.metrics.txt"
  fi
}

capture target "$TARGET"
capture control "$CONTROL"

trimmed_version() {
  tr -d '\r\n' < "$1"
}

extract_commit() {
  sed -nE 's/.*\(([0-9a-fA-F]{40}|[0-9a-fA-F]{64})\).*/\1/p' "$1" | head -1
}

TARGET_VERSION=$(trimmed_version "$OUT_DIR/target.version.txt")
CONTROL_VERSION=$(trimmed_version "$OUT_DIR/control.version.txt")
TARGET_COMMIT=$(extract_commit "$OUT_DIR/target.version.txt")
CONTROL_COMMIT=$(extract_commit "$OUT_DIR/control.version.txt")

[ -n "$TARGET_COMMIT" ] || blocked "target build provenance is unknown; exact-build comparison is impossible"
[ -n "$CONTROL_COMMIT" ] || blocked "control build provenance is unknown; exact-build comparison is impossible"
[ "$TARGET_COMMIT" = "$CONTROL_COMMIT" ] || blocked "target and control are different builds ($TARGET_COMMIT != $CONTROL_COMMIT)"
[ "$TARGET_VERSION" = "$CONTROL_VERSION" ] || blocked "target and control /version strings differ despite matching commit"

probe_row() {
  local base=$1 file=$2
  awk -v base="$base" '$1 == base { print; exit }' "$file"
}

TARGET_ROW=$(probe_row "$TARGET" "$OUT_DIR/target.probe.txt")
CONTROL_ROW=$(probe_row "$CONTROL" "$OUT_DIR/control.probe.txt")
[ -n "$TARGET_ROW" ] || blocked "target probe produced no parseable summary row"
[ -n "$CONTROL_ROW" ] || blocked "control probe produced no parseable summary row"

read -r _ TARGET_HEIGHT TARGET_FINAL TARGET_READ TARGET_NOOP TARGET_DELTA TARGET_KIND _ <<<"$TARGET_ROW"
read -r _ CONTROL_HEIGHT CONTROL_FINAL CONTROL_READ CONTROL_NOOP CONTROL_DELTA CONTROL_KIND _ <<<"$CONTROL_ROW"

[[ "$TARGET_READ" =~ ^[0-9]+$ ]] || blocked "target read median is not numeric"
[[ "$CONTROL_READ" =~ ^[0-9]+$ ]] || blocked "control read median is not numeric"
[ "$TARGET_KIND" = "values" ] || blocked "target read did not return values"
[ "$CONTROL_KIND" = "values" ] || blocked "control read did not return values"
[ "$CONTROL_READ" -gt 0 ] || blocked "control read median is zero; ratio is undefined"

RATIO=$(awk -v t="$TARGET_READ" -v c="$CONTROL_READ" 'BEGIN { printf "%.6f", t / c }')

{
  printf 'build=%s\n' "$TARGET_COMMIT"
  printf 'version=%s\n' "$TARGET_VERSION"
  printf 'samples=%s\n' "$SAMPLES"
  printf 'max_factor=%s\n' "$MAX_FACTOR"
  printf 'target=%s\n' "$TARGET"
  printf 'target_height=%s\n' "$TARGET_HEIGHT"
  printf 'target_final=%s\n' "$TARGET_FINAL"
  printf 'target_read_median_ms=%s\n' "$TARGET_READ"
  printf 'target_noop_median_ms=%s\n' "$TARGET_NOOP"
  printf 'target_delta_ms=%s\n' "$TARGET_DELTA"
  printf 'control=%s\n' "$CONTROL"
  printf 'control_height=%s\n' "$CONTROL_HEIGHT"
  printf 'control_final=%s\n' "$CONTROL_FINAL"
  printf 'control_read_median_ms=%s\n' "$CONTROL_READ"
  printf 'control_noop_median_ms=%s\n' "$CONTROL_NOOP"
  printf 'control_delta_ms=%s\n' "$CONTROL_DELTA"
  printf 'target_over_control=%s\n' "$RATIO"
  printf 'evidence_dir=%s\n' "$OUT_DIR"
} | tee "$OUT_DIR/summary.txt"

if awk -v t="$TARGET_READ" -v c="$CONTROL_READ" -v f="$MAX_FACTOR" 'BEGIN { exit !(t <= c * f) }'; then
  printf 'READ_PATH_364_ACCEPTANCE=PASS\n' | tee -a "$OUT_DIR/summary.txt"
  exit 0
fi

printf 'READ_PATH_364_ACCEPTANCE=FAIL\n' | tee -a "$OUT_DIR/summary.txt"
printf 'reason=target median %s ms exceeds %sx control median %s ms\n' \
  "$TARGET_READ" "$MAX_FACTOR" "$CONTROL_READ" | tee -a "$OUT_DIR/summary.txt"
exit 1
