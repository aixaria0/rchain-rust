#!/usr/bin/env bash
# The close-out status -- what is left, in one line and one list.
#
# **This is the loop.** It runs in seconds, and it answers the only question a person sitting down to
# work has: what is left, and is the check-off telling the truth. Everything expensive about the audit
# -- the pointer scan, the review-ledger join, the Lean build behind the law register -- is a
# cross-check of the *tree*, and belongs at the boundary, not in front of every edit.
#
# Measured: ~2.5 s warm, dominated by the emitter's evidence check (one pass over the tracked tree,
# ~1.5 s). The `review-ledger` join this deliberately does not run is 46 s of the register gate's 78.
# If this ever takes more than about five seconds, that number is the thing to look at -- it is here in
# the header so a regression is visible rather than felt.
#
# Usage:
#   tools/audit-status.sh            # status line, then the TODO list
#   tools/audit-status.sh --quiet    # the status line alone (what every other run prints first)
#   tools/audit-status.sh --full     # also the DONE list
#
# Exit: 0 the check-off is current · 1 it is stale or malformed.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TSV="$ROOT/spec/findings.tsv"
LEDGER="$ROOT/spec/review-ledger.tsv"

quiet=0 full=0
for a in "$@"; do
  case "$a" in
    --quiet) quiet=1 ;;
    --full) full=1 ;;
    *) printf 'usage: %s [--quiet|--full]\n' "$0" >&2; exit 2 ;;
  esac
done

[[ -f "$TSV" ]] || { printf 'audit-status: no %s\n' "$TSV" >&2; exit 1; }

rows="$(grep -v '^#' "$TSV" | grep -v '^[[:space:]]*$')"
count() { printf '%s\n' "$rows" | awk -F'\t' -v s="$1" '$4 == s' | wc -l; }
todo=$(count todo); prog=$(count 'in progress'); done_=$(count done)

t1_total=$(awk -F'\t' '$3 == "T1"' "$LEDGER" 2>/dev/null | wc -l)
t1_unread=$(awk -F'\t' '$3 == "T1" && $4 == "deferred"' "$LEDGER" 2>/dev/null | wc -l)

# The counts first, because that is the answer; the gate's verdict second, because it is the caveat.
printf 'Findings  TODO %s · IN PROGRESS %s · DONE %s   ·   Coverage  %s of %s T1 modules unread\n' \
  "$todo" "$prog" "$done_" "$t1_unread" "$t1_total"

if [[ "$quiet" == "0" ]]; then
  if (( todo > 0 )); then
    printf '\n'
    printf '%s\n' "$rows" | awk -F'\t' '$4 == "todo"' | sort -t$'\t' -k1,1V \
      | awk -F'\t' '{ printf "  %-6s %s\n", $1, $5 }'
  fi
  if (( t1_unread > 0 )); then
    printf '\n  (plus %s T1 modules nobody has read -- modules that can fork the chain or lose funds)\n' "$t1_unread"
  fi
  if [[ "$full" == "1" ]]; then
    printf '\nDONE:\n'
    printf '%s\n' "$rows" | awk -F'\t' '$4 == "done"' | sort -t$'\t' -k1,1V \
      | awk -F'\t' '{ printf "  %-6s %s\n", $1, $5 }'
  fi
  printf '\n'
fi

# The check-off must be what the TSV renders, and its rows must be well-formed -- the emitter owns
# those rules and this is the one place they are asked for cheaply.
if out="$("$ROOT/tools/emit-findings-register.sh" --check 2>&1)"; then
  [[ "$quiet" == "1" ]] || printf 'check-off current.\n'
  exit 0
fi
printf '%s\n' "$out" >&2
exit 1
