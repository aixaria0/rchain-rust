#!/usr/bin/env bash
# Tick a check-off row. One command, and the state it writes is the state the register renders.
#
# **Why this exists as a command rather than as an instruction to edit a TSV.** A state you have to
# hand-edit is a state that lags what you actually did, and a register that lags is the thing that let
# six of §13's Low findings sit unaddressed since pass 4 -- the work was as invisible as the state was.
# The edit itself is trivial; remembering to make it is not. So it is one word and an id.
#
# Usage:
#   tools/todo.sh                    the status, then the next thing to work on
#   tools/todo.sh start <id>         todo        -> in progress
#   tools/todo.sh done  <id> <evidence...>
#                                    -> done, with what holds it
#   tools/todo.sh block <id> <why...>
#                                    -> todo, with why it is not done
#   tools/todo.sh read  <path> <symbol> <what it found, or why nothing>
#                                    review-ledger: verdict `cleared`, depth `deep`
#   tools/todo.sh found <path> <symbol> <C-number> <what the defect is>
#                                    review-ledger: verdict `finding`
#
# The last two take a *path* where the first three take a C-number, because the coverage half of the
# check-off is rows in `spec/review-ledger.tsv` rather than rows in `spec/findings.tsv`.
#
# <evidence> is what a reader checks: a test name, a path, a commit. It is required for `done`,
# because a `done` row with nothing behind it is the claim this whole register is built to avoid.
#
# Exit: 0 ok · 1 unknown id or a refusal from the renderer · 2 usage.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TSV="$ROOT/spec/findings.tsv"
LEDGER="$ROOT/spec/review-ledger.tsv"

die() { printf 'todo: %s\n' "$*" >&2; exit 1; }

[[ -f "$TSV" ]] || die "no $TSV"

set_state() { # <id> <state> <owes> <evidence>
  local id="$1" st="$2" ow="$3" ev="$4"
  grep -qE "^${id}[[:space:]]" "$TSV" || die "no row for '$id' (ids are the leftmost column)"
  awk -F'\t' -v OFS='\t' -v id="$id" -v st="$st" -v ow="$ow" -v ev="$ev" '
    $1 == id { $4 = st; if (ow != "") $7 = ow; if (ev != "") $6 = ev }
    { print }
  ' "$TSV" > "$TSV.tmp" && mv "$TSV.tmp" "$TSV"
}

# **The coverage half lives in another file, and until 2026-09-27 this tool could not reach it.**
# `findings.tsv` holds the findings; the twenty unread T1 modules are rows in `review-ledger.tsv`, so
# the larger half of the check-off was listed on the front page by a tool that refused to close it.
# The two verbs below take a *path* where the ones above take a C-number.
#
# Columns: kind id tier verdict depth sample reason evidence registers note (1-10).
set_ledger() { # <path> <verdict> <evidence> <registers> <note>
  local path="$1" verdict="$2" ev="$3" reg="$4" note="$5"
  [[ -f "$LEDGER" ]] || die "no $LEDGER"
  awk -F'\t' -v p="$path" '$1 == "file" && $2 == p { found = 1 } END { exit !found }' "$LEDGER" \
    || die "no review-ledger row for '$path' (the leftmost path column, kind 'file')"
  awk -F'\t' -v OFS='\t' -v p="$path" -v v="$verdict" -v ev="$ev" -v reg="$reg" -v n="$note" '
    $1 == "file" && $2 == p {
      $4 = v; $5 = "deep"; $8 = ev; $9 = reg; $10 = n
    }
    { print }
  ' "$LEDGER" > "$LEDGER.tmp" && mv "$LEDGER.tmp" "$LEDGER"
}

show_ledger() {
  awk -F'\t' -v p="$1" '$1 == "file" && $2 == p { printf "  %s  %s  (%s)\n", $2, $4, $9 }' "$LEDGER"
}

show_state() {
  awk -F'\t' -v id="$1" '$1 == id { printf "  %s  %s  (%s)\n", $1, $4, $5 }' "$TSV"
}

case "${1:-next}" in
  next|"")
    tools_status="$ROOT/tools/audit-status.sh"
    "$tools_status" || exit 1
    ;;

  start)
    [[ $# -ge 2 ]] || die "usage: todo.sh start <id>"
    set_state "$2" "in progress" "-" ""
    show_state "$2"
    ;;

  done)
    [[ $# -ge 3 ]] || die "usage: todo.sh done <id> <evidence>   (evidence is required: a test name, a path, a commit)"
    id="$2"; shift 2
    ev="$*"
    [[ -n "$ev" ]] || die "evidence is required for 'done'"
    set_state "$id" "done" "-" "$ev"
    show_state "$id"
    ;;

  # **The two coverage verbs.** A T1 module is closed by *being read*, and the ledger records what
  # that produced: `cleared` and the symbol you read, or `finding` and the C-number it became. Both
  # are `deep` by construction — the tier definition says a T1 row cannot be closed shallow
  # (`spec/TEST-COVERAGE.md:528-530`), and the depth column is what holds that.
  read)
    [[ $# -ge 4 ]] || die "usage: todo.sh read <path> <symbol> <what the read found, or why it found nothing>"
    path="$2"; symbol="$3"; shift 3
    set_ledger "$path" "cleared" "$path:$symbol" "-" "$*"
    show_ledger "$path"
    ;;

  found)
    [[ $# -ge 5 ]] || die "usage: todo.sh found <path> <symbol> <C-number> <what the defect is>"
    path="$2"; symbol="$3"; cnum="$4"; shift 4
    set_ledger "$path" "finding" "$path:$symbol $cnum" "$cnum" "$*"
    show_ledger "$path"
    ;;

  block)
    [[ $# -ge 3 ]] || die "usage: todo.sh block <id> <why it is not done yet>"
    id="$2"; shift 2
    why="$*"
    set_state "$id" "todo" "$why" "-"
    show_state "$id"
    ;;

  *) die "usage: todo.sh [start|done|block] <id> ..." ;;
esac

# Re-render, then say what the status is now. A state change that does not render is a state change
# only this command knows about.
if out="$("$ROOT/tools/emit-findings-register.sh" 2>&1)"; then
  printf '\n'
  "$ROOT/tools/audit-status.sh" --quiet
else
  printf '%s\n' "$out" >&2
  exit 1
fi
