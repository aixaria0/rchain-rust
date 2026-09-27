#!/usr/bin/env bash
# Plant a mechanism deletion, run the witness that is supposed to catch it, restore. One row, one run.
#
# Why this exists. The law sweep asks, for each law row's named witness: does that witness actually go
# red when the mechanism the law describes is deleted? A witness that stays green is a finding — it is
# a test that would pass with the law violated. Doing this by hand for 35 more rows has two hazards,
# and this session hit both of them:
#
#   1. A fixed backup path (`/tmp/mut.bak`) is shared state. Two sessions sweeping at once overwrite
#      each other's backup, and a "restore" then writes another session's file over this one.
#   2. Plant-then-restore is not atomic across an interruption. A call killed between the two left a
#      live mutation in the tree — a deleted linearity guard — until another session noticed it.
#
# So: backups go in a private `mktemp -d`; the restore is on a trap, so a signal restores; and a
# journal written *before* the plant survives `SIGKILL`, so `--recover` can undo a plant that no trap
# could catch. The journal is a plain list of `file<TAB>backup`, and it is empty at rest — if it is
# not empty, a previous run died mid-mutation and this script refuses to start until it is recovered.
#
# Three outcomes, and they mean different things:
#
#   red     the named witness failed while the mechanism was deleted. The witness is real evidence.
#   green   the witness passed with the mechanism deleted. The witness is vacuous — it cannot fail on
#           the defect it names. This is a finding about the test, and the more dangerous kind.
#   no-build  the crate did not compile with the plant in. The mechanism is load-bearing, but this
#           says nothing about the witness, so it is not reported as evidence either way.
#
# The last two are distinguished deliberately: a compile error is not a red witness, and a run that
# reported "red" off a build failure would be exactly the kind of unearned green this audit exists to
# find, in reverse.
#
# Usage:
#   tools/audit-mutate.sh --file F --old OLD --new NEW --crate C --test FILTER --label LABEL
#   tools/audit-mutate.sh --file F --old-file A --new-file B --crate C --test T --label L
#   tools/audit-mutate.sh --recover            # undo a plant left by a killed run
#   tools/audit-mutate.sh --journal            # show what is pending, restore nothing
#
# OLD and NEW are literal strings. For a plant that spans lines use --old-file/--new-file: the strings
# are read verbatim, so no shell quoting rule can reach them and the mutation is a file you can read
# before it runs. OLD must match the file exactly once — a plant that matched nothing, or matched
# somewhere unexpected, is a harness error and not a measurement.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

# Under target/, which is gitignored: the journal is state, not source. It is the one path that has to
# be predictable for --recover to find it, so it is the one path that is not per-run.
JOURNAL="$ROOT/target/audit-mutate.journal"
SCRATCH=""
BACKED_FILE=""
BACKED_COPY=""

die() { printf 'audit-mutate: %s\n' "$1" >&2; exit 2; }

# The journal is written before the plant and truncated after the restore. A non-empty journal at
# startup means a previous run was killed in between, and the tree is holding a planted defect.
recover() {
  local f b
  [[ -f "$JOURNAL" ]] || { printf 'audit-mutate: nothing to recover\n'; return 0; }
  while IFS=$'\t' read -r f b; do
    [[ -n "$f" ]] || continue
    [[ -f "$b" ]] || die "journal names $f but its backup $b is gone — restore it from git"
    cp "$b" "$f"
    printf 'restored %s\n' "$f"
  done < "$JOURNAL"
  : > "$JOURNAL"
}

case "${1:-}" in
  --recover) recover; exit 0 ;;
  --journal) [[ -s "$JOURNAL" ]] && cat "$JOURNAL" || printf 'audit-mutate: journal empty\n'; exit 0 ;;
  -h|--help) sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
esac

[[ -s "$JOURNAL" ]] && die "a previous run left a plant in the tree; run --recover first (see --journal)"

FILE="" OLD="" NEW="" CRATE="" TEST="" LABEL="" OLD_FILE="" NEW_FILE="" EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)  FILE="$2"; shift 2 ;;
    --old)   OLD="$2";  shift 2 ;;
    --new)   NEW="$2";  shift 2 ;;
    --crate) CRATE="$2"; shift 2 ;;
    --test)  TEST="$2"; shift 2 ;;
    --label) LABEL="$2"; shift 2 ;;
    --old-file) OLD_FILE="$2"; shift 2 ;;
    --new-file) NEW_FILE="$2"; shift 2 ;;
    --)      shift; EXTRA=("$@"); break ;;
    *)       die "unknown argument: $1" ;;
  esac
done

# Read verbatim from a file: no trailing-newline trimming, no escape interpretation, and the plant is
# a file you can read before it runs.
if [[ -n "$OLD_FILE" ]]; then OLD="$(cat "$OLD_FILE")"; fi
if [[ -n "$NEW_FILE" ]]; then NEW="$(cat "$NEW_FILE")"; fi

[[ -n "$FILE" && -n "$CRATE" && -n "$TEST" && -n "$LABEL" ]] || die "--file, --crate, --test and --label are required"
[[ -f "$FILE" ]] || die "no such file: $FILE"

# The restore is verified against the file's content at the start of the run — so if that content is
# already a plant, the verification blesses it and the run ends with the plant still live and a
# "restored" line saying it is fine. Anchoring to HEAD closes that: a run may only start from the
# committed content. (This is the failure mode the earlier hand-typed sweep would have had if two
# plants had ever overlapped.)
if ! git diff HEAD --quiet -- "$FILE"; then
  die "$FILE is already modified relative to HEAD — commit it, or run --recover; a restore to an already-modified file verifies nothing"
fi
[[ -n "$OLD" ]] || die "--old is empty; a plant that matches nothing measures nothing"

SCRATCH="$(mktemp -d)"
# The restore is on every path out, including a signal. This is the fix for the incident: the earlier
# in-line sweep restored on success and on failure, but not when the call was killed between the two.
restore_all() {
  if [[ -n "$BACKED_FILE" && -f "$BACKED_COPY" ]]; then
    cp "$BACKED_COPY" "$BACKED_FILE"
    : > "$JOURNAL"
  fi
  rm -rf "$SCRATCH"
}
trap 'restore_all' EXIT INT TERM

backup="$(printf '%s' "$FILE" | tr / _)"
cp "$FILE" "$SCRATCH/$backup"
sha_before="$(sha256sum "$FILE" | cut -d' ' -f1)"

# The exact-string edit, with the strings passed through the environment so no shell quoting rule can
# reach them. `count` distinguishes "did not match" from "matched twice" — both are harness errors,
# and both would otherwise be indistinguishable from a green witness.
export MUT_FILE="$FILE" MUT_OLD="$OLD" MUT_NEW="$NEW"
python3 - <<'PY' || exit 2
import os, sys, pathlib
p = pathlib.Path(os.environ["MUT_FILE"])
s = p.read_text()
old, new = os.environ["MUT_OLD"], os.environ["MUT_NEW"]
n = s.count(old)
if n != 1:
    print(f"audit-mutate: --old matches {n} times, need exactly 1", file=sys.stderr)
    sys.exit(2)
p.write_text(s.replace(old, new, 1))
PY

# The plant is on disk now, so the journal goes down before anything else can fail.
BACKED_FILE="$FILE"; BACKED_COPY="$SCRATCH/$backup"
printf '%s\t%s\n' "$BACKED_FILE" "$BACKED_COPY" > "$JOURNAL"

sha_planted="$(sha256sum "$FILE" | cut -d' ' -f1)"
[[ "$sha_planted" != "$sha_before" ]] || die "the plant did not change $FILE"

printf 'mutate: %s\n' "$LABEL"
printf '  %s\n' "$FILE"
printf '  witness: cargo test -p %s %s\n' "$CRATE" "$TEST"

out="$(cargo test -p "$CRATE" "${EXTRA[@]+"${EXTRA[@]}"}" "$TEST" 2>&1)"; rc=$?

# A build failure is not a red witness. `error[E...]` / `error: could not compile` is the crate
# refusing the plant, which is a different fact from the named test noticing the defect.
verdict=""
if printf '%s' "$out" | grep -qE '^error(\[E[0-9]+\])?: could not compile|^error\[E[0-9]+\]'; then
  verdict="no-build"
elif [[ "$rc" == "0" ]]; then
  verdict="green"
else
  verdict="red"
fi

# Restore now, not at the trap, so the restore is verified while the run is still the topic.
cp "$SCRATCH/$backup" "$FILE"
sha_after="$(sha256sum "$FILE" | cut -d' ' -f1)"
[[ "$sha_after" == "$sha_before" ]] || die "$FILE did not restore to its pre-plant content"
: > "$JOURNAL"
BACKED_FILE=""

printf '\n'
case "$verdict" in
  red)      printf '  RED    the witness fails with the mechanism deleted — real evidence\n' ;;
  green)    printf '  GREEN  the witness PASSES with the mechanism deleted — vacuous, this is a finding\n' ;;
  no-build) printf '  NO-BUILD  the crate rejects the plant; says nothing about the witness\n' ;;
esac
# Name the witness, not just the verdict. On red the row's evidence *is* which test failed — a filter
# that matches four tests says "something caught it", and the ledger needs to record which something.
if [[ "$verdict" == "red" ]]; then
  printf '%s\n' "$out" | grep -E '^test .* FAILED' | sed 's/^/    witness: /'
else
  printf '%s\n' "$out" | grep -E '^(test .* FAILED|failures:|assertion|thread .* panicked|error)' | head -8 | sed 's/^/    /'
fi
printf '  restored: sha256 %s verified\n' "${sha_before:0:12}"

[[ "$verdict" == "red" ]] && exit 0
exit 1
