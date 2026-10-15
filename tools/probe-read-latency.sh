#!/usr/bin/env bash
# Read latency, per node, with the chain's height beside it and the layers under it separated.
#
# Why this exists: on this codebase a *read* is an `explore-deploy` — the node evaluates a Rholang term
# against the state rather than serving a cached view — so the time a user feels is a function of the
# state it has to reach. That makes read latency a *scaling* property of a chain, not a constant, and it
# is worth a number attached to a height rather than an opinion.
#
# What it does: for each base URL, reads the height and finalised block, then times N calls of one fixed
# term, and prints median/min/max. The term is fixed so runs are comparable across nodes and over time.
#
# **Two arms, and the second one is the point** (issue #364). The read arm is the term above; the
# *control* arm is the same request shape with nothing to read at all (`new return in { return!(42) }`).
# A cost that a request doing nothing also pays is by definition not a read cost — and separating those
# two is what took an investigation four rounds, because a single arm cannot tell "this read is slow"
# from "this node is slow". The `delta` column is the reading: near zero means the term is free and the
# number is the node; large means the read itself is doing work. **Read it against the min/max on the
# next line rather than alone** — a delta inside the run's own spread is noise, and the honest way to
# shrink that spread is more samples (`--samples`), not a smaller number.
#
# **And the network is separated inside the same run.** One `curl -w` against `/api/status` per node
# prints DNS, TCP, TLS and time-to-first-byte; `ttfb − tls` is what is left for the node *or its reverse
# proxy* after the connection is up. Nothing from outside can split those two — three hosts behind one
# edge make a slow vhost look exactly like a slow node — so the second line also says how to split them:
# point the same probe at `http://127.0.0.1:<api-port>` **on the box**, where DNS, connect and TLS
# collapse to nothing and the answer is the node's own.
#
# Usage:  probe-read-latency.sh [--samples N] <BASE_URL> [<BASE_URL> ...]
# Example: probe-read-latency.sh https://playground.rhobot.net https://testnet.rhobot.net
# Local:   probe-read-latency.sh http://127.0.0.1:40403 http://127.0.0.1:50403
#
# It reports what it got: a non-200 or an empty `expr` is shown, not smoothed over. The term returns the
# bond set, so an empty result is a real answer worth noticing rather than an error to hide — and the
# control arm's result says whether the node answered at all, which distinguishes "the read failed" from
# "the node was silent".
set -u
N=7; [ "${1:-}" = "--samples" ] && { N=$2; shift 2; }
[ $# -ge 1 ] || { echo "usage: $0 [--samples N] <BASE_URL> [...]" >&2; exit 2; }

# The term: read the bonded set through the pos system process. Fixed, so every run measures the same work.
TERM='new return, pos(`rho:rchain:pos`), ret in { pos!("getBonds", [*ret]) | for (@b <- ret) { return!(b) } }'
# The control: the same request, nothing to read. One line, and it is what makes the read arm's number mean
# something (issue #364). It answers on the term's first `new`-bound name, which is where a reply is read
# from — as the read arm does — so both arms travel the same reply path.
NOOP='new return in { return!(42) }'
BODY=$(mktemp)
median() { printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'; }
kind() { # what came back, in words, so an empty answer cannot pass for a fast one
  case "$1" in
    *ExprInt*) printf 'values' ;;
    *'"expr":[]'*) printf 'EMPTY expr' ;;
    "") printf 'no response' ;;
    *) printf '%s' "$1" | head -c 44 ;;
  esac
}
# One arm: N samples of `term`, on stdout as "<median> <min> <max> <kind>".
arm() { # base term
  local base="$1" term="$2" t0 t1 out times=() last=""
  python3 -c 'import json,sys; json.dump(sys.argv[1], open(sys.argv[2],"w"))' "$term" "$BODY"
  for _ in $(seq 1 "$N"); do
    t0=$(date +%s%N)
    out=$(curl -s --max-time 60 -X POST "$base/api/explore-deploy" \
          -H 'Content-Type: application/json' --data-binary @"$BODY" 2>/dev/null)
    t1=$(date +%s%N)
    times+=( $(( (t1 - t0) / 1000000 )) )
    last="$out"
  done
  local sorted; sorted=$(printf '%s\n' "${times[@]}" | sort -n)
  printf '%s %s %s %s\n' "$(median "${times[@]}")" \
    "$(printf '%s' "$sorted" | head -1)" "$(printf '%s' "$sorted" | tail -1)" "$(kind "$last")"
}

printf '%-34s %7s %6s %9s %9s %8s  %s\n' \
  "node" "height" "final" "read med" "no-op med" "delta" "result"
for base in "$@"; do
  st=$(curl -s --max-time 20 "$base/api/status" 2>/dev/null)
  lf=$(curl -s --max-time 20 "$base/api/last-finalized-block" 2>/dev/null)
  H=$(printf '%s' "$st" | grep -oE '"latestBlockNumber":[0-9]+' | head -1 | cut -d: -f2)
  F=$(printf '%s' "$lf" | grep -oE '"blockNumber":[0-9]+' | head -1 | cut -d: -f2)

  read -r rmed rmin rmax rkind <<<"$(arm "$base" "$TERM")"
  read -r nmed nmin nmax nkind <<<"$(arm "$base" "$NOOP")"
  delta=$(( rmed - nmed ))

  # One connection, broken into its layers: what the network costs against what is left after it.
  net=$(curl -s -o /dev/null --max-time 20 \
        -w '%{time_namelookup} %{time_connect} %{time_appconnect} %{time_starttransfer}' \
        "$base/api/status" 2>/dev/null)
  set -- $net
  dns=${1:-?}; conn=${2:-?}; tls=${3:-?}; ttfb=${4:-?}

  printf '%-34s %7s %6s %9s %9s %8s  %s\n' \
    "$base" "${H:-?}" "${F:-?}" "$rmed" "$nmed" "$delta" "$rkind"
  printf '%-34s read %s ms (min %s max %s) · no-op %s ms · %s\n' "" "$rmed" "$rmin" "$rmax" "$nmed" "$nkind"
  awk -v d="$dns" -v c="$conn" -v t="$tls" -v f="$ttfb" 'BEGIN {
    printf "%-34s network: dns=%.0f conn=%.0f tls=%.0f ttfb=%.0f", "", d*1000, c*1000, t*1000, f*1000
    if (f > t) printf "  → after tls %.0f ms (the node or its proxy; loopback splits them)", (f-t)*1000
    print ""
  }'
done
rm -f "$BODY"
