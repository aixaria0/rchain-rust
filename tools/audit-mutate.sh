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
# With `--law N` a red is only a red if the failing test is one the row declares. Otherwise the run is
# green, however many other tests went red — a row is falsified by its own witness or by nothing.
#
# The last two are distinguished deliberately: a compile error is not a red witness, and a run that
# reported "red" off a build failure would be exactly the kind of unearned green this audit exists to
# find, in reverse.
#
# Usage:
#   tools/audit-mutate.sh --file F --old OLD --new NEW --crate C --test FILTER --label LABEL
#   tools/audit-mutate.sh --file F --old-file A --new-file B --crate C --test T --label L
#     --law N            require row N's *declared* Rust witness (spec/laws.tsv col 14) to be the one
#                        that failed; a red elsewhere in the filter is not this row's evidence
#     --also-file F --also-old-file A --also-new-file B
#                        a second plant, for the rows with two independent guards (law 28, law 50a)
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

FILE="" OLD="" NEW="" CRATE="" TEST="" LABEL="" LAW="" OLD_FILE="" NEW_FILE="" EXTRA=()
declare -a ALSO_FILE=() ALSO_OLD=() ALSO_NEW=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)  FILE="$2"; shift 2 ;;
    --old)   OLD="$2";  shift 2 ;;
    --new)   NEW="$2";  shift 2 ;;
    --crate) CRATE="$2"; shift 2 ;;
    --test)  TEST="$2"; shift 2 ;;
    --label) LABEL="$2"; shift 2 ;;
    --law)   LAW="$2";   shift 2 ;;
    --old-file) OLD_FILE="$2"; shift 2 ;;
    --new-file) NEW_FILE="$2"; shift 2 ;;
    # A second (third, …) plant, for the rows with two independent guards: law 28's and law 50a's both
    # record that removing either alone leaves the witness green because the other still refuses, so a
    # single-plant sweep cannot falsify them at all.
    --also-file)     ALSO_FILE+=("$2"); ALSO_OLD+=(""); ALSO_NEW+=(""); shift 2 ;;
    --also-old-file) ALSO_OLD[-1]="$(cat "$2")"; shift 2 ;;
    --also-new-file) ALSO_NEW[-1]="$(cat "$2")"; shift 2 ;;
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
  local f c
  for f in "${PLANTED[@]:-}"; do
    [[ -n "$f" ]] || continue
    c="$SCRATCH/$(printf '%s' "$f" | tr / _)"
    [[ -f "$c" ]] && cp "$c" "$f"
  done
  [[ ${#PLANTED[@]} -gt 0 ]] && : > "$JOURNAL"
  rm -rf "$SCRATCH"
}
trap 'restore_all' EXIT INT TERM

# A row may need more than one plant: law 28's and law 50a's rows both record two independent guards
# where removing either alone leaves the witness green because the other still refuses. A sweep that
# can only delete one mechanism at a time cannot falsify those rows at all, so the plants are a list.
PLANTED=()
pl_shas=()
plant() {
  local file="$1" old="$2" new="$3"
  local key
  key="$(printf '%s' "$file" | tr / _)"
  cp "$file" "$SCRATCH/$key"
  pl_shas+=("$(sha256sum "$file" | cut -d' ' -f1)")
  export MUT_FILE="$file" MUT_OLD="$old" MUT_NEW="$new"
  # The exact-string edit, with the strings passed through the environment so no shell quoting rule
  # can reach them. `count` distinguishes "did not match" from "matched twice" — both are harness
  # errors, and both would otherwise be indistinguishable from a green witness.
  python3 - <<'PY' || exit 2
import os, sys, pathlib
p = pathlib.Path(os.environ["MUT_FILE"])
s = p.read_text()
old, new = os.environ["MUT_OLD"], os.environ["MUT_NEW"]
n = s.count(old)
if n != 1:
    print(f"audit-mutate: a plant's --old matches {n} times, need exactly 1", file=sys.stderr)
    sys.exit(2)
p.write_text(s.replace(old, new, 1))
PY
  [[ "$(sha256sum "$file" | cut -d' ' -f1)" != "${pl_shas[-1]}" ]] || die "the plant did not change $file"
  PLANTED+=("$file")
}

check_clean() {
  local file="$1"
  # The restore is verified against the file's content at the start of the run — so if that content is
  # already a plant, the verification blesses it and the run ends with the plant still live and a
  # "restored" line saying it is fine. Anchoring to HEAD closes that: a run may only start from the
  # committed content. (This is the failure mode the earlier hand-typed sweep would have had if two
  # plants had ever overlapped.)
  git diff HEAD --quiet -- "$file" ||
    die "$file is already modified relative to HEAD — commit it, or run --recover; a restore to an already-modified file verifies nothing"
}

check_clean "$FILE"
plant "$FILE" "$OLD" "$NEW"
for i in "${!ALSO_FILE[@]}"; do
  [[ -n "${ALSO_FILE[$i]}" ]] || continue
  check_clean "${ALSO_FILE[$i]}"
  plant "${ALSO_FILE[$i]}" "${ALSO_OLD[$i]}" "${ALSO_NEW[$i]}"
done

# The plants are on disk now, so the journal goes down before anything else can fail.
for f in "${PLANTED[@]}"; do
  printf '%s\t%s\n' "$f" "$SCRATCH/$(printf '%s' "$f" | tr / _)"
done > "$JOURNAL"

printf 'mutate: %s\n' "$LABEL"
for f in "${PLANTED[@]}"; do printf '  %s\n' "$f"; done
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

# `--test` is a *filter on test names*, not a target selector — `--test lean_parse_corpus` selects
# nothing, because the tests in that target are named `the_node_parser_agrees_with_the_lean_model` and
# `the_printers_output_round_trips_through_the_node`. The run then reports `ok. 0 passed; 0 failed; N
# filtered out` and the verdict is a green over zero tests, which is the failure this whole audit keeps
# finding and the one I hit myself: every corpus row I probed this way came back "green" for the wrong
# reason. A run where nothing ran is not evidence, so it is refused rather than reported.
if ! printf '%s' "$out" | grep -E '^test result:' | grep -qvE '0 passed; 0 failed'; then
  printf '%s\n' "$out" | grep -E '^test result:' | head -3 | sed 's/^/    /'
  die "the filter '$TEST' matched no test in any target — nothing ran, so there is no verdict"
fi

# The pass criterion, and it is not "a witness went red". It is "**the row's own declared witness** went
# red". A module-wide run that reddens some other row's witness is not evidence for this row, and the
# loose criterion records the opposite of the truth: C149 is the demonstration — law 46's declared
# witness stayed green while law 47's caught the mutation, so a run filtered on the module reads as
# `law 46 cleared` when law 46 is exactly the row whose evidence is vacuous. Passing `--law N` makes the
# criterion checkable instead of remembered: it reads row N's declared Rust witnesses out of
# `spec/laws.tsv` and requires the failure set to intersect them.
if [[ -n "$LAW" ]]; then
  export MUT_OUT="$out" MUT_LAW="$LAW"
  declared_result="$(python3 - <<'PY'
import os, subprocess, re, sys
law = os.environ["MUT_LAW"]
# A law row is (number, clause): 16 has four clauses a–d, each its own laws.tsv row with its own
# witnesses. Matching on the number alone unions all four, so `--law 16` would accept clause c's
# witness as evidence for clause d — the criterion would still be the wrong one, just less obviously.
m = re.match(r"^(\d+)([a-z]?)$", law)
if not m:
    print(f"none\t"); sys.exit(0)
num, clause = m.group(1), m.group(2)
prog = '$1==n && $2==c {print $14}' if clause else '$1==n && $2=="" {print $14}'
field = subprocess.run(["awk", "-F\t", "-v", f"n={num}", "-v", f"c={clause}", prog, "spec/laws.tsv"],
                       capture_output=True, text=True).stdout.strip()
if not field or field == "-":
    print("none\t"); sys.exit(0)
names = set()
for part in field.split(", "):
    part = part.strip()
    if part:
        names.add(part.rsplit(":", 1)[-1])
failed = set()
for line in os.environ["MUT_OUT"].splitlines():
    m = re.match(r"^test (\S+) \.\.\. FAILED$", line)
    if m:
        failed.add(m.group(1).rsplit("::", 1)[-1])
print(("hit\t" if names & failed else "miss\t") + ", ".join(sorted(names)))
PY
)"
  case "$declared_result" in
    none*)  die "law $LAW declares no Rust witness in spec/laws.tsv column 14 — it cannot be falsified this way"
            ;;
    miss*)  # `no-build` survives: the crate refusing the plant is a fact about the plant, and the
            # criterion has nothing to say about it. Writing this as a plain `else green` turned a
            # compile failure into a finding — a false one, and the worst kind for this tool.
            case "$verdict" in
              red)     verdict="red-undeclared" ;;
              no-build) : ;;
              *)       verdict="green" ;;
            esac
            # `$'\t'`, not `\t`: in a glob pattern a backslash-t is a literal `t`, so `#*\t` stripped
            # through the first `t` of the witness name and printed `he_gate_scheduler_...` for
            # `law21_the_gate_scheduler_...`. The verdict was right and the name it blamed was not.
            printf '\n  declared witness(es) for law %s: %s\n' "$LAW" "${declared_result#*$'\t'}"
            printf '  none of them failed — a red elsewhere in the filter is not this row'"'"'s evidence\n'
            ;;
  esac
fi

# Restore now, not at the trap, so the restore is verified while the run is still the topic.
for i in "${!PLANTED[@]}"; do
  f="${PLANTED[$i]}"
  cp "$SCRATCH/$(printf '%s' "$f" | tr / _)" "$f"
  [[ "$(sha256sum "$f" | cut -d' ' -f1)" == "${pl_shas[$i]}" ]] || die "$f did not restore to its pre-plant content"
done
: > "$JOURNAL"
PLANTED=()

printf '\n'
case "$verdict" in
  red)      printf '  RED    the witness fails with the mechanism deleted — real evidence\n' ;;
  red-undeclared) printf '  GREEN  something failed but NOT the row'"'"'s declared witness — the row is unfalsified by this run\n' ;;
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
# Always show every target's run summary. A green is only evidence if tests ran, and "0 passed;
# 0 failed; 12 filtered out" is a green that means the filter matched nothing — the failure this whole
# audit keeps finding, and the one a verdict line cannot show.
printf '%s\n' "$out" | grep -E '^test result:' | sed 's/^/    ran: /'
printf '  restored: %s file(s) at their pre-plant sha256\n' "${#pl_shas[@]}"

[[ "$verdict" == "red" ]] && exit 0
exit 1
