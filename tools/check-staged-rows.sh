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
#
# **The two authored halves, and neither is `spec/AUDIT.md` any more** (2026-09-27). The register is
# now the check-off in `spec/AUDIT.md`, which the emitter writes; the finding *entries* live in
# `spec/audit/passes.md` and the finding *rows* in `spec/findings.tsv`. So a finding arrives in one of
# two places, and a gate watching only the emitted file would watch the one place nobody authors.
#
# The distinction that makes this usable: **a `findings.tsv` row for a number that already existed at
# `HEAD` is not a new finding.** Backfilling, correcting a state, or re-pointing a citation all edit
# existing rows, and a gate that refused them would be a gate every commit has to bypass — which is
# how the peer-row defect this check exists for would come straight back. So the TSV arm reports only
# the numbers that are genuinely new against `HEAD`.
# **`cut -f1`, not a pattern over the line.** The first cell is the id and the rest of the line is
# prose that names other findings, so a `grep -oE` over the whole line reports the ids a row *cites*
# as if it were adding them. And the class is `[A-Z]+[0-9]+` rather than a letter list: the first
# version named the letters and left out `T`, so it could not see a single `TS` row.
#
# **What counts as "already allocated" is read from every place a row has ever lived**, and that is
# the load-bearing half. A finding that *moved* between files is not a new finding: relocating the pass
# record out of `spec/AUDIT.md` showed all 158 ids as added, because a file that is new to git has
# every one of its lines as a `+`. Without this the gate is one the move has to bypass, and a gate that
# gets bypassed on a large change gets bypassed on the next one.
head_ids="$(
  { git show HEAD:spec/findings.tsv 2>/dev/null | grep -vE '^#' | cut -f1 | grep -E '^[A-Z]+[0-9]+$'
    git show HEAD:spec/AUDIT.md 2>/dev/null     | grep -oE '^- \*\*[A-Z]+[0-9]+' | grep -oE '[A-Z]+[0-9]+'
    git show HEAD:spec/AUDIT.md 2>/dev/null     | grep -oE '^\| [A-Z]+[0-9]+ '  | grep -oE '[A-Z]+[0-9]+'
  } | LC_ALL=C sort -u || true)"

# Both authored halves, then one subtraction -- subtracting from only one of them is what let the
# first version of this print all 158 ids as new.
added_ids="$(
  { git diff --cached -- spec/audit/passes.md  | grep -oE '^\+- \*\*[A-Z]+[0-9]+' | grep -oE '[A-Z]+[0-9]+'
    git diff --cached -- spec/audit/passes.md  | grep -oE '^\+\| [A-Z]+[0-9]+ '  | grep -oE '[A-Z]+[0-9]+'
    git diff --cached -- spec/findings.tsv     | grep -E '^\+[A-Z]+[0-9]+[[:space:]]' | sed 's/^+//' | cut -f1
  } | LC_ALL=C sort -u || true)"

# **Both sides lexically sorted for `comm`, and that is a fix rather than style** — the same one
# `tools/next-audit-number.sh` records as AUDIT C103. `comm` requires a lexical order and exits
# non-zero when it does not get one, and a numeric-or-locale sort of `100` against `99` disagrees with
# it. A failure here would read as a refusal to commit, which is the worst way for a gate to be wrong.
rows="$(comm -23 <(printf '%s\n' "$added_ids") <(printf '%s\n' "$head_ids") | grep -v '^$' || true)"

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
