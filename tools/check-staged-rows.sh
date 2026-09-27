#!/usr/bin/env bash
# Refuse to commit another session's register rows.
#
# **Why this is a gate and not a habit.** Three times in one day a commit in this tree carried a row
# belonging to a second session, because `git add <path>` stages the whole file and both sessions edit
# `spec/AUDIT.md`. Twice the check ran and printed the foreign row first — `C144 | C159` — and the commit
# proceeded anyway, because the two were chained in one command and the evidence scrolled past a commit
# that ran unconditionally. A check whose output scrolls past the thing it is checking is a report, not
# a gate, and three resolutions to "read it this time" failed in the same way.
#
# So this exits non-zero, and it belongs on the left of `&&`:
#
#     tools/check-staged-rows.sh 'C159' && git commit -F - <<'MSG'
#     …
#     MSG
#
# A commit that adds no C-row at all declares `-`, so the tooling and spec commits have a spelling too
# and "I forgot to pass anything" is an error rather than a silent pass.
#
# Usage: tools/check-staged-rows.sh <C-rows you own, comma-separated, or ->
#
# Exit: 0 every staged C-row is declared · 1 nothing staged · 2 a row is undeclared.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

mine="${1:-}"

staged_files="$(git diff --cached --name-only)"
if [[ -z "$staged_files" ]]; then
  printf 'check-staged-rows: nothing is staged, so there is nothing to commit\n' >&2
  exit 1
fi

printf 'check-staged-rows: staged paths\n'
printf '%s\n' "$staged_files" | sed 's/^/  /'

# A C-row is the one thing whose author is unambiguous — the row opens with its number, so a commit
# that adds one is making a claim about that number. Anchored at the start of the added line, so a row
# that merely *mentions* another finding in its prose is not mistaken for adding it.
rows="$(git diff --cached -- spec/AUDIT.md | grep -E '^\+\| C[0-9]+' | grep -oE 'C[0-9]+' | sort -u || true)"

if [[ "$mine" == "-" ]]; then
  if [[ -n "$rows" ]]; then
    printf 'check-staged-rows: REFUSING — this commit adds %s and you declared no rows\n' \
      "$(printf '%s' "$rows" | tr '\n' ' ')" >&2
    exit 2
  fi
  printf 'check-staged-rows: ok — no C-row, as declared\n'
  exit 0
fi

if [[ -z "$rows" ]]; then
  printf 'check-staged-rows: REFUSING — you declared %s and this commit adds no C-row; declare `-` if that is right\n' "$mine" >&2
  exit 2
fi

# Compare as whole tokens. `C15` must not match `C159`, which a substring test would allow.
unexpected="$(printf '%s\n' "$rows" | grep -vxF -f <(printf '%s\n' "$mine" | tr ',' '\n' | sed 's/^ *//; s/ *$//') || true)"
if [[ -n "$unexpected" ]]; then
  printf 'check-staged-rows: REFUSING — undeclared C-row(s): %s\n' "$(printf '%s' "$unexpected" | tr '\n' ' ')" >&2
  printf 'check-staged-rows: they belong to another session. `git restore --staged spec/AUDIT.md`,\n' >&2
  printf 'check-staged-rows: then stage only your own row — or declare it, if it really is yours.\n' >&2
  exit 2
fi

printf 'check-staged-rows: ok — every staged C-row (%s) is declared\n' "$(printf '%s' "$rows" | tr '\n' ' ')"
