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
#
# <evidence> is what a reader checks: a test name, a path, a commit. It is required for `done`,
# because a `done` row with nothing behind it is the claim this whole register is built to avoid.
#
# Exit: 0 ok · 1 unknown id or a refusal from the renderer · 2 usage.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TSV="$ROOT/spec/findings.tsv"

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
