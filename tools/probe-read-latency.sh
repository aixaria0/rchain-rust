#!/usr/bin/env bash
# Read latency, per node, with the chain's height beside it.
#
# Why this exists: on this codebase a *read* is an `explore-deploy` — the node evaluates a Rholang term
# against the state rather than serving a cached view — so the time a user feels is a function of the
# state it has to reach. That makes read latency a *scaling* property of a chain, not a constant, and it
# is worth a number attached to a height rather than an opinion.
#
# What it does: for each base URL, reads the height and finalised block, then times N calls of one fixed
# term, and prints median/min/max. The term is fixed so runs are comparable across nodes and over time.
#
# Usage:  probe-read-latency.sh [--samples N] <BASE_URL> [<BASE_URL> ...]
# Example: probe-read-latency.sh https://playground.rhobot.net https://testnet.rhobot.net
#
# It reports what it got: a non-200 or an empty `expr` is shown, not smoothed over. The term returns the
# bond set, so an empty result is a real answer worth noticing rather than an error to hide.
set -u
N=7; [ "${1:-}" = "--samples" ] && { N=$2; shift 2; }
[ $# -ge 1 ] || { echo "usage: $0 [--samples N] <BASE_URL> [...]" >&2; exit 2; }

# The term: read the bonded set through the pos system process. Fixed, so every run measures the same work.
TERM='new return, pos(`rho:rchain:pos`), ret in { pos!("getBonds", [*ret]) | for (@b <- ret) { return!(b) } }'
BODY=$(mktemp); python3 -c 'import json,sys; json.dump(sys.argv[1], open(sys.argv[2],"w"))' "$TERM" "$BODY"

jget() { grep -oE "\"$2\":[0-9]+" <<<"$1" | head -1 | cut -d: -f2; }
printf '%-34s %8s %8s %9s %9s %9s  %s\n' "node" "height" "final" "median" "min" "max" "result"
for base in "$@"; do
  st=$(curl -s --max-time 20 "$base/api/status" 2>/dev/null)
  lf=$(curl -s --max-time 20 "$base/api/last-finalized-block" 2>/dev/null)
  H=$(jget "$st" latestBlockNumber); F=$(jget "$lf" 'blockNumber')
  times=(); note=""
  for i in $(seq 1 "$N"); do
    t0=$(date +%s%N)
    out=$(curl -s --max-time 60 -X POST "$base/api/explore-deploy" \
          -H 'Content-Type: application/json' --data-binary @"$BODY" 2>/dev/null)
    t1=$(date +%s%N)
    times+=( $(( (t1-t0)/1000000 )) )
    case "$out" in
      *ExprInt*) note="read returned values" ;;
      *'"expr":[]'*) note="empty expr" ;;
      "") note="no response" ;;
      *) note="$(printf '%s' "$out" | head -c 44)" ;;
    esac
  done
  sorted=$(printf '%s\n' "${times[@]}" | sort -n)
  med=$(printf '%s\n' "$sorted" | awk '{a[NR]=$1} END{print a[int((NR+1)/2)]}')
  printf '%-34s %8s %8s %9s %9s %9s  %s\n' "$base" "${H:-?}" "${F:-?}" "$med" \
         "$(printf '%s\n' "$sorted" | head -1)" "$(printf '%s\n' "$sorted" | tail -1)" "$note"
done
rm -f "$BODY"
