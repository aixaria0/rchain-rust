#!/usr/bin/env bash
# The #364 read-path sweep: one build, one box, several chain heights.
#
# Why this exists. The live cross-section that raised the issue is three builds on two boxes with two
# different read outcomes (`EMPTY expr` against values), so nothing in it is attributable. This holds
# build, box and outcome constant and varies **depth** — which is the one variable the public
# measurements cannot isolate. At each height it takes the same two readings the probe always takes
# (a read and a no-op control, with the delta) and the census the node now publishes, so "the cost is
# per-request setup" can be split into its stages on a running node rather than argued from source.
#
# Usage:  sweep-read-path.sh <label> [blocks-to-force-before-sampling]
set -u
BASE=${BASE:-http://localhost:40403}
# **The admin listener, not the public one.** `POST /api/propose` on the public port is a 404; the
# route lives on the admin listener, and a force loop pointed at the wrong port is a silent no-op that
# reports a height it never moved.
ADMIN=${ADMIN:-http://localhost:40405}
LABEL=${1:?usage: sweep-read-path.sh <label> [blocks]}
FORCE=${2:-0}
OUT=${OUT:-target/read-path-sweep}
mkdir -p "$OUT"

height() { curl -s --max-time 10 "$BASE/api/status" | grep -oE '"latestBlockNumber":[0-9]+' | head -1 | cut -d: -f2; }

if [ "$FORCE" -gt 0 ]; then
  echo "[$LABEL] forcing $FORCE blocks..."
  for _ in $(seq 1 "$FORCE"); do
    curl -s --max-time 30 -X POST "$ADMIN/api/propose" -o /dev/null 2>/dev/null
  done
fi

H=$(height)
echo "[$LABEL] height ${H:-?}"
tools/probe-read-latency.sh --samples 9 "$BASE" | tee "$OUT/probe-$LABEL.txt"
# **One more block after the probe, before scraping.** The census is published from the DAG's
# observability tick, which fires when a block is inserted — so on an idle node the gauges are frozen
# at boot and a scrape taken straight after the probe shows the reads as zero. A block after the probe
# is what makes the readings above visible.
curl -s --max-time 30 -X POST "$ADMIN/api/propose" -o /dev/null 2>/dev/null
sleep 2
curl -s --max-time 10 "$BASE/metrics" > "$OUT/metrics-$LABEL.txt"
{
  echo "# $LABEL  height=${H:-?}"
  grep -E '^rchain_runtime_read_' "$OUT/metrics-$LABEL.txt" || echo "(no read_* gauges found)"
} | tee "$OUT/census-$LABEL.txt"
