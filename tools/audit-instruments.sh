#!/usr/bin/env bash
# Falsify the instruments: plant the defect each gate exists to catch, and require the gate to fail.
#
# Why this exists. This repository's audit register states the rule twice —
#
#   "An instrument that cannot see the defect it names is not evidence" (spec/AUDIT.md, the 2026-09-24
#   blind-spot paragraph), and "an instrument that passes on the defect it names is not evidence, and
#   only the falsifier distinguishes" (spec/AUDIT.md, the U14 sweep).
#
# — and both times it was learned the hard way: the `panic` class could not see `debug_assert!` because
# a word boundary never occurs before `assert` after `_`, and the `escape` class could not see a public
# `.get()` because nobody had looked for that form. Each was green while the defect it named was live.
# A probe is the only thing that distinguishes "clean" from "the scan stopped looking", and a scan that
# has gone blind looks exactly like a clean tree.
#
# What it does. For each gate: plant the defect, run the gate, require a non-zero exit, restore. The
# tree is restored on every path, including a signal, and the script refuses to run on a dirty tree so
# that "restore" is always a known-good state rather than an assumption.
#
# Reading the result. A FAIL row means an instrument did not notice the defect it exists for — that is
# a finding about the instrument, and the more dangerous kind, because the gate has been reporting
# green. An OK row is the instrument working. This script's own exit code is 1 if any row is FAIL.
#
# Usage:  tools/audit-instruments.sh

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

SCRATCH="$(mktemp -d)"
FAILED=0
PROBES=0

die() { printf 'audit-instruments: %s\n' "$1" >&2; exit 2; }

# **A linked worktree, not the primary checkout.** The restore is a whole-file write from a snapshot
# taken *before* the probe ran, and there is no check of any kind in between — so a file another
# session edits during the run is not "reverted", it is overwritten with stale content. That happened:
# this harness rewrote a `spec/TEST-COVERAGE.md` row another session had just added, twice, and its
# probe plants showed up as that session's red gates for the length of the run.
#
# No amount of care in `restore_all` fixes this — backup-and-restore cannot be made safe against a
# concurrent writer. It can only be *confined* to a tree nobody else is writing, which is what this
# guard does. `git rev-parse --git-dir` equals `--git-common-dir` in the primary checkout and differs
# in a linked one. The flag is for a tree you genuinely own alone; it is not a formality to pass
# reflexively, and passing it restores exactly the behaviour this guard exists to prevent.
PRIMARY_OK=0
for arg in "$@"; do [[ "$arg" == "--shared-tree-ok" ]] && PRIMARY_OK=1; done
if [[ "$PRIMARY_OK" == "0" && "$(git rev-parse --git-dir 2>/dev/null)" == "$(git rev-parse --git-common-dir 2>/dev/null)" ]]; then
  die "this is the primary checkout; the probes edit tracked files under other sessions' feet. Run from a linked worktree (git worktree add --detach <path> HEAD), or pass --shared-tree-ok if the tree is yours alone"
fi

# The probes edit tracked files, so a *modified* tracked file makes the restore ambiguous — and a
# probe that cannot restore is worse than no probe, because it leaves a planted defect behind.
# Untracked files are fine: a probe cannot corrupt what it does not back up, and refusing on them
# would stop this script from ever running before it is committed.
DIRTY="$(git status --porcelain | grep -v '^??' || true)"
if [[ -n "$DIRTY" ]]; then
  printf '%s\n' "$DIRTY" >&2
  die "tracked files are modified; commit or stash first (the probes need a known-good restore target)"
fi

declare -a BACKED_UP=()
restore_all() {
  local f
  for f in "${BACKED_UP[@]:-}"; do
    [[ -n "$f" && -f "$SCRATCH/$(printf '%s' "$f" | tr / _)" ]] || continue
    cp "$SCRATCH/$(printf '%s' "$f" | tr / _)" "$f"
  done
}
trap 'restore_all; rm -rf "$SCRATCH"' EXIT INT TERM

backup() {
  local f="$1"
  cp "$f" "$SCRATCH/$(printf '%s' "$f" | tr / _)"
  BACKED_UP+=("$f")
}

# probe <label> <pass|fail> <expected-nonzero>  — records the row and the tally.
report() {
  local label="$1" rc="$2"
  PROBES=$((PROBES + 1))
  if [[ "$rc" != "0" ]]; then
    printf '  OK    %-34s caught it (exit %s)\n' "$label" "$rc"
  else
    printf '  FAIL  %-34s DID NOT FAIL — the gate cannot see its own defect\n' "$label"
    FAILED=$((FAILED + 1))
  fi
}

# run <label> <gate-command...> — runs the gate quietly and reports whether it refused.
run() {
  local label="$1"; shift
  local out rc
  out="$("$@" 2>&1)" && rc=0 || rc=$?
  report "$label" "$rc"
  if [[ "$rc" == "0" ]]; then printf '%s\n' "$out" | tail -3 | sed 's/^/        /'; fi
}

printf 'Falsifying the instruments (plant → require failure → restore)\n\n'

# ---------------------------------------------------------------------------------------------
# tools/audit-type-system.sh — the four hard classes and the both-ways ratchet.
# A file in the crate roster that is not on the panic allowlist.
PROBE_FILE="graphz/src/lib.rs"

printf 'audit-type-system.sh\n'
backup "$PROBE_FILE"
printf '\npub fn probe_panic() { let _ = Some(1u8).unwrap(); }\n' >> "$PROBE_FILE"
run "panic: a production .unwrap()" bash tools/audit-type-system.sh panic
restore_all

backup "$PROBE_FILE"
printf '\npub fn probe_unsafe() { unsafe { let _p: *const u8 = std::ptr::null(); } }\n' >> "$PROBE_FILE"
run "unsafe: an unsafe block" bash tools/audit-type-system.sh unsafe
restore_all

backup "$PROBE_FILE"
printf '\npub fn probe_silent() { let _x: u8 = 300u16.try_into().unwrap(); }\n' >> "$PROBE_FILE"
run "silent: a flattened fallible conversion" bash tools/audit-type-system.sh silent
restore_all

# The escape class matches inside an `impl` naming a refinement, in the files that hold them.
backup shared/src/refined.rs
printf '\nimpl std::ops::Deref for BlockHeight {\n    type Target = i64;\n    fn deref(&self) -> &i64 { &self.0 }\n}\n' >> shared/src/refined.rs
run "escape: a Deref on a refinement" bash tools/audit-type-system.sh escape
restore_all

backup "$PROBE_FILE"
printf '\npub fn probe_cast() { let _x = 1u64 as u32; }\n' >> "$PROBE_FILE"
run "cast ratchet: a site arrives" bash tools/audit-type-system.sh cast
restore_all

# The ratchet is *bidirectional* (AUDIT C98): a site leaving fails too, which is what stops the number
# moving without a commit saying so. Raise the baseline and the unchanged tree must fail.
backup tools/type-system-baseline.tsv
perl -0pi -e 's/^cast\t336$/cast\t337/m' tools/type-system-baseline.tsv
run "cast ratchet: a site leaves" bash tools/audit-type-system.sh cast
restore_all

# ---------------------------------------------------------------------------------------------
printf '\naudit-test-register.sh\n'

# Check 14: every `C<n>` pointer anywhere in spec/ must resolve to an allocated finding.
backup spec/RUST-VS-SCALA.md
printf '\nSee AUDIT C999 for the detail.\n' >> spec/RUST-VS-SCALA.md
run "check 14: a C-pointer to nothing" bash tools/audit-test-register.sh
restore_all

# Check 2: a named test in the `## Machine-checked claims` table must exist. Probe the table the check
# actually reads — the law-property matrix is a *different* table and is covered by no check (see the
# ledger's `tool` roster).
backup spec/TEST-COVERAGE.md
perl -0pi -e 's/`insert_rejects_equivocation_same_seq_num`/`insert_rejects_equivocation_same_seq_num_XYZ`/' spec/TEST-COVERAGE.md
run "check 2: a named test that is not there" bash tools/audit-test-register.sh
restore_all

# Check 9: a `path:line` citation in the *emitted* register must resolve and be inside the file.
backup spec/laws.tsv
perl -0pi -e 's{([a-z-]+/src/[a-z_/]+\.rs):\d+}{$1:99999}' spec/laws.tsv
run "check 9: an anchor past EOF" bash tools/audit-test-register.sh
restore_all

# ---------------------------------------------------------------------------------------------
# Check 15, clause by clause. The letters are the body's own (a)-(j), and each probe plants the defect
# that clause exists to catch and requires the register gate to refuse. The plan that wrote this check
# found clause (h) vacuous — it compared nothing while reporting "48 tier(s)" — so the clauses are
# probed rather than assumed. **Two of them had no subject in the tree at all when this was written**:
# (f) applies only to a `sampled` row and the ledger holds none, and the header/body clause (i) is
# self-referential. For those a probe is the only thing that separates "the clause is satisfied" from
# "the clause never ran", which is the distinction this whole file exists for.
printf '\ncheck 15 (the review ledger), clause by clause\n'

# (a) rows and the roster agree in **both** directions — one direction alone is C80's defect.
backup spec/review-ledger.tsv
perl -0pi -e 's/^class\tcast\t.*\n//m' spec/review-ledger.tsv
run "15(a): a roster item with no row" bash tools/audit-test-register.sh
restore_all

backup spec/review-ledger.tsv
printf 'class\ta_class_no_roster_derives\tT3\tdeferred\t-\t-\t-\t-\t-\t-\n' >> spec/review-ledger.tsv
run "15(a): a row about nothing" bash tools/audit-test-register.sh
restore_all

# (b) the emitter is the site of the join — an edit that does not re-emit is not the derived page.
backup spec/review-ledger.tsv
perl -0pi -e 's/(law\t5\tT1\tcleared\tdeep\t-\t-\t[^\t]*\t-\t)mutation sweep/$1mutation sweep (edited by the probe)/' spec/review-ledger.tsv
run "15(b): the page is not what the TSV emits" bash tools/audit-test-register.sh
restore_all

# (c) the vocabulary is closed and the cells are conditional on the verdict.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t10\tT1\t)cleared/$1banana/m' spec/review-ledger.tsv
run "15(c): a verdict outside the vocabulary" bash tools/audit-test-register.sh
restore_all

backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t10\tT1\tcleared\tdeep\t)-\t/$1 3\/60\t/m' spec/review-ledger.tsv
run "15(c): a sample on a row that is not sampled" bash tools/audit-test-register.sh
restore_all

# (d) a `finding` row names an allocated C-number sharing a token with that finding.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t46\tT1\tfinding\tdeep\t-\t-\t[^\t]* )C149/$1C999/m' spec/review-ledger.tsv
run "15(d): a finding number that is allocated by nothing" bash tools/audit-test-register.sh
restore_all

# (e) the tier floor: a review claim at T1 is a `deep` read.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t10\tT1\tcleared\t)deep/$1-/m' spec/review-ledger.tsv
run "15(e): a T1 row claiming no depth" bash tools/audit-test-register.sh
restore_all

# (f) a `sampled` row's denominator is the roster's own count. **No row in the tree is `sampled`**, so
# the probe has to make one: the clause is turned on rather than exercised.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t10\tT1\t)cleared(\tdeep\t)-\t/$1sampled$2 1\/999\t/m' spec/review-ledger.tsv
run "15(f): a sampled denominator that is remembered" bash tools/audit-test-register.sh
restore_all

# (g) ceilings, committed against derived.
backup spec/review-ledger.tsv
perl -0pi -e 's/^# ceiling\ttool\t(\d+)/"# ceiling\ttool\t" . ($1 + 1)/me' spec/review-ledger.tsv
run "15(g): a ceiling the tree does not derive" bash tools/audit-test-register.sh
restore_all

# (h) the tier word agrees with the register's own tier table. Law 10's row inherits T1 from
# spec/TEST-COVERAGE.md; demoting it here makes the two disagree.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t10\t)T1/$1T2/m' spec/review-ledger.tsv
run "15(h): a tier that contradicts the register" bash tools/audit-test-register.sh
restore_all

# (i) the header names every check the body runs — C91's rule, one level down: a check the header does
# not name is a check nobody knows ran.
backup tools/audit-test-register.sh
perl -0pi -e 's/^# Usage:/#  16. **a check the body does not run**\n#\n# Usage:/m' tools/audit-test-register.sh
run "15(i): a header naming a check nothing runs" bash tools/audit-test-register.sh
restore_all

# (j) every allocated C-number is named by a row or counted against `unhousedCeiling`.
backup spec/review-ledger.tsv
perl -0pi -e 's/^# unhousedCeiling\t.*\n//m' spec/review-ledger.tsv
run "15(j): no unhousedCeiling to count against" bash tools/audit-test-register.sh
restore_all

# …and the other direction: an allocated number that no row names must move the count.
backup spec/review-ledger.tsv
perl -0pi -e 's/^(law\t46\tT1\tfinding\tdeep\t-\t-\t[^\t]* )C149/$1C-none/m' spec/review-ledger.tsv
run "15(j): C149 allocated and named by no row" bash tools/audit-test-register.sh
restore_all

# ---------------------------------------------------------------------------------------------
printf '\ncheck-rust-witnesses.sh\n'
# The tool's whole point is the zero-match refusal: `cargo test <filter>` exits 0 when nothing
# matches, so a renamed witness would silently pass. A real `fn` that is not a test must fail.
printf 'comm/src/transport/chunker.rs:chunk_it\n' > "$SCRATCH/witness.txt"
out="$(timeout 900 tools/check-rust-witnesses.sh "$SCRATCH/witness.txt" 2>&1)" && rc=0 || rc=$?
report "zero-match refusal" "$rc"
if [[ "$rc" == "0" ]]; then printf '%s\n' "$out" | tail -3 | sed 's/^/        /'; fi

# ---------------------------------------------------------------------------------------------
printf '\naudit-vendored-sources.sh\n'
# This gate is in **no workflow** (only `make check-register` runs it), so it is reported here even
# though a probe cannot tell whether anyone would run it.
if grep -rq 'audit-vendored-sources' .github/workflows/ 2>/dev/null; then
  printf '  OK    %-34s reachable from CI\n' "vendored sources in CI"
else
  printf '  FAIL  %-34s %s\n' "vendored sources in CI" "in no workflow — make check-register is the only entry point"
  FAILED=$((FAILED + 1))
fi
PROBES=$((PROBES + 1))

printf '\n%d probe(s), %d instrument(s) did not catch their own defect.\n' "$PROBES" "$FAILED"
[[ "$FAILED" == "0" ]] || exit 1
