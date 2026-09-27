#!/usr/bin/env bash
# Render the audit check-off into `spec/AUDIT.md` from `spec/findings.tsv` and the review ledger.
#
# **Why this exists, in one measurement.** `spec/AUDIT.md` was 4,366 lines organised by audit *pass*,
# and a finding's state was a bolded phrase at an arbitrary position inside a 2 KB cell -- `**Fixed`
# 108 times, `**Decided` 9, `**Registered` 4, and a different vocabulary on top in each pass section.
# So "what is left?" took twenty-one sections and a hand merge, nothing could answer it, and the answer
# nobody could see was that six of §13's Low findings had sat unaddressed since pass 4.
#
# Three states now, in the order a person works: `todo`, `in progress`, `done`. The state is authored
# in the TSV; this renders it; `--check` refuses a check-off that disagrees.
#
# Usage:
#   tools/emit-findings-register.sh            # write the check-off into spec/AUDIT.md
#   tools/emit-findings-register.sh --check    # exit 1 if the committed check-off is stale
#
# Exit: 0 ok · 1 stale or malformed · 2 usage.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TSV="$ROOT/spec/findings.tsv"
AUDIT="$ROOT/spec/AUDIT.md"
LEDGER="$ROOT/spec/review-ledger.tsv"

# Everything below this marker in `spec/AUDIT.md` is generated; everything above it is the frame a
# human wrote. Splitting on a marker rather than on a line number means the frame can grow without
# the emitter and the file disagreeing about where the check-off starts.
MARKER='<!--EMPTY: the emitter writes the check-off below this line-->'
# **`|`-separated, not space-separated, because one of the states has a space in it.** The first
# version split the list on spaces, so the accepted tokens were `todo`, `in`, `progress` and `done` --
# and `in progress`, which is the state a person is actually in when they set one, was rejected by the
# vocabulary it belongs to. `tools/todo.sh start` is what found it, on its first run.
VOCAB="todo|in progress|done"

check=0
case "${1:-}" in
  --check) check=1 ;;
  "") ;;
  *) printf 'usage: %s [--check]\n' "$0" >&2; exit 2 ;;
esac

[[ -f "$TSV" ]] || { printf 'emit-findings-register: no %s\n' "$TSV" >&2; exit 1; }

rows="$(grep -v '^#' "$TSV" | grep -v '^[[:space:]]*$')"
n_total=$(printf '%s\n' "$rows" | wc -l)

# --- the state vocabulary, and the condition each word carries -----------------------------------
#
# **Parsed by awk, not by `read`.** `read` with `IFS=$'\t'` collapses runs of the delimiter, because
# tab is *IFS whitespace* -- so a row with an empty `alias` cell had every field shifted left and the
# state arrived as the title. The first run of this check reported all 197 rows as out of vocabulary,
# which was the check's own defect and not the register's.
bad_vocab="$(printf '%s\n' "$rows" | awk -F'\t' -v vocab="$VOCAB" '
  BEGIN { n = split(vocab, v, "|"); for (i = 1; i <= n; i++) ok[v[i]] = 1 }
  { if (!($4 in ok)) printf " %s=%s", $1, ($4 == "" ? "(empty)" : $4) }')"
bad_shape="$(printf '%s\n' "$rows" | awk -F'\t' '
  NF != 7 { printf " %s(%d fields)", $1, NF }
  $3 == "" || $4 == "" || $5 == "" { printf " %s(empty)", $1 }')"

# **A `todo` row that does not say what would close it is the defect this column exists for.** The
# register recorded five findings as "owed" without saying what was owed, which is how a row stays
# open for a year and reads as addressed.
todo_unowed="$(printf '%s\n' "$rows" | awk -F'\t' '$4 == "todo" && ($7 == "-" || $7 == "") { printf " %s", $1 }')"

# --- every `evidence` token names something that exists ------------------------------------------
#
# **This is the check that makes `done` mean anything.** An `evidence` cell claims the tree contains
# what holds the fix; a token resolving to nothing is a citation to a test that was renamed, deleted,
# or never written -- the class pass 9 found three of (C152, C153, C154). One pass over the tracked
# tree, ~1.5 s, 22,878 identifiers. `legacy/` and the generated `docs/book/` are excluded: a name that
# only exists in the unported Scala is not evidence that the port holds the fix.
tree_ids="$(mktemp)"
git -C "$ROOT" ls-files 2>/dev/null | grep -vE '^(legacy/|docs/book/|target/)' \
  | xargs -r grep -hoE '[A-Za-z_][A-Za-z0-9_]{5,}' 2>/dev/null | sort -u > "$tree_ids"
# The subject is the *name*, not the path spelling: an evidence cell holds forms like
# `casper/src/gateway/ledger.rs::record_vote` and `tokio::task::spawn_blocking`, and the question worth
# asking of either is whether the symbol at the end of it is in this tree.
unresolved="$(
  printf '%s\n' "$rows" \
    | awk -F'\t' '$6 != "" && $6 != "-" {
        n = split($6, t, "/")
        for (i = 1; i <= n; i++) {
          q = t[i]; sub(/^.*::/, "", q); gsub(/[^A-Za-z0-9_]/, "", q)
          if (length(q) >= 6) print $1 "\t" q
        }
      }' \
    | while IFS=$'\t' read -r eid etok; do
        [[ -n "$eid" ]] || continue
        grep -qxF "$etok" "$tree_ids" || printf ' %s(%s)' "$eid" "$etok"
      done)"
rm -f "$tree_ids"

fail=0
if [[ -n "$bad_vocab" ]]; then
  printf 'emit-findings-register: state outside the closed vocabulary:%s\n' "$bad_vocab" >&2
  printf '  the three words are: %s\n' "$VOCAB" >&2
  fail=1
fi
if [[ -n "$bad_shape" ]]; then
  printf 'emit-findings-register: malformed row(s):%s\n' "$bad_shape" >&2
  fail=1
fi
if [[ -n "$todo_unowed" ]]; then
  printf 'emit-findings-register: todo row(s) naming nothing that would close them:%s\n' "$todo_unowed" >&2
  fail=1
fi
if [[ -n "$unresolved" ]]; then
  printf 'emit-findings-register: evidence naming nothing in the tree:%s\n' "$unresolved" >&2
  fail=1
fi
if [[ "$fail" == "1" ]]; then exit 1; fi

# --- the coverage half ---------------------------------------------------------------------------
#
# Read straight from `spec/review-ledger.tsv` rather than through `tools/emit-review-ledger.sh`, and
# that is a deliberate cost decision: the emitter's *join* (that the ledger's rows and the tree's
# rosters agree in both directions) is 46 of the register gate's 78 seconds, and it is a cross-check
# of the *tree*. What the check-off needs is a count over 624 rows, which is an awk. The join stays in
# `tools/audit-test-register.sh`, where it belongs.
t1_total=$(awk -F'\t' '$3 == "T1"' "$LEDGER" 2>/dev/null | wc -l)
t1_deferred=$(awk -F'\t' '$3 == "T1" && $4 == "deferred"' "$LEDGER" 2>/dev/null | wc -l)

# --- the check-off -------------------------------------------------------------------------------
count_of() { printf '%s\n' "$rows" | awk -F'\t' -v s="$1" '$4 == s' | wc -l; }

tbl_todo() {
  printf '%s\n' "$rows" | awk -F'\t' '$4 == "todo"' | sort -t$'\t' -k1,1V \
    | awk -F'\t' '{ printf "| `%s` | %s | %s | §%s |\n", $1, $5, $7, $3 }'
}
tbl_prog() {
  printf '%s\n' "$rows" | awk -F'\t' '$4 == "in progress"' | sort -t$'\t' -k1,1V \
    | awk -F'\t' '{ printf "| `%s` | %s | %s | §%s |\n", $1, $5, $7, $3 }'
}
tbl_done() {
  printf '%s\n' "$rows" | awk -F'\t' '$4 == "done"' | sort -t$'\t' -k1,1V \
    | awk -F'\t' '{ e = ($6 == "" || $6 == "-" ? "—" : $6); printf "| `%s` | %s | %s | §%s |\n", $1, $5, e, $3 }'
}
tbl_unread() {
  awk -F'\t' '$3 == "T1" && $4 == "deferred" { printf "| `%s` |\n", $2 }' "$LEDGER" 2>/dev/null | sort
}

panel="$(mktemp)"
{
  printf '## Check-off\n\n'
  printf '**Findings  TODO %s · IN PROGRESS %s · DONE %s** &nbsp;&nbsp;·&nbsp;&nbsp; **Coverage  %s of %s T1 modules unread**\n\n' \
    "$(count_of todo)" "$(count_of 'in progress')" "$(count_of done)" "$t1_deferred" "$t1_total"
  printf 'Closed when both halves are zero. A **done** row is settled -- fixed, assessed faithful, a\n'
  printf 'deliberate deviation, or refuted -- and names what holds it. A **todo** row names what would\n'
  printf 'close it. **%s of the %s findings name no evidence**, which is a column here rather than an\n' \
    "$(printf '%s\n' "$rows" | awk -F'\t' '$4 == "done" && ($6 == "" || $6 == "-")' | wc -l)" "$(count_of done)"
  printf 'implication: a `done` row says the fix is in the tree, not that it is correct.\n\n'

  printf '### TODO — findings (%s)\n\n| id | what | what closes it | account |\n|---|---|---|---|\n' "$(count_of todo)"
  tbl_todo; printf '\n'
  printf '### TODO — unread T1 modules (%s)\n\n' "$t1_deferred"
  printf 'The modules that can fork the chain or lose funds, and that nobody has read. In remit and not\n'
  printf 'yet read, which is what `deferred` means in [`REVIEW-LEDGER.md`](REVIEW-LEDGER.md).\n\n'
  printf '| module |\n|---|\n'
  tbl_unread; printf '\n'
  printf '### IN PROGRESS (%s)\n\n| id | what | what closes it | account |\n|---|---|---|---|\n' "$(count_of 'in progress')"
  tbl_prog; printf '\n'
  printf '### DONE (%s)\n\n| id | what | evidence | account |\n|---|---|---|---|\n' "$(count_of done)"
  tbl_done; printf '\n'
} > "$panel"

# --- splice, or check ------------------------------------------------------------------------------
render_or_check() {
  local frame_from from to committed
  from=$(grep -nF "$MARKER" "$AUDIT" | head -1 | cut -d: -f1)
  if [[ -z "$from" ]]; then
    printf 'emit-findings-register: %s carries no marker to write below\n' "spec/AUDIT.md" >&2
    return 1
  fi

  if [[ "$check" == "0" ]]; then
    { head -n "$from" "$AUDIT"; printf '\n'; cat "$panel"; } > "$AUDIT.tmp"
    mv "$AUDIT.tmp" "$AUDIT"
    printf 'emit-findings-register: %s findings -> spec/AUDIT.md (%s todo, %s in progress, %s done)\n' \
      "$n_total" "$(count_of todo)" "$(count_of 'in progress')" "$(count_of done)"
    return 0
  fi

  committed="$(mktemp)"
  # `sed '1{/^$/d}'` drops the one blank line the splice writes between the marker and the panel. The
  # whole rendered region is compared, not a hash of it, because the diff is what tells a reader which
  # row moved.
  tail -n "+$((from + 1))" "$AUDIT" | sed '1{/^$/d}' > "$committed"
  if ! diff -q "$committed" "$panel" >/dev/null; then
    printf 'emit-findings-register: the check-off is stale against spec/findings.tsv\n' >&2
    diff "$committed" "$panel" | head -30 >&2
    rm -f "$committed"; return 1
  fi
  rm -f "$committed"
  printf 'emit-findings-register: ok — spec/AUDIT.md matches spec/findings.tsv (%s rows: %s todo, %s done)\n' \
    "$n_total" "$(count_of todo)" "$(count_of done)"
  return 0
}

render_or_check
rc=$?
rm -f "$panel"
exit $rc
