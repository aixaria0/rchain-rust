#!/usr/bin/env bash
# check-no-conflict-markers.sh — no tracked file carries an unresolved merge conflict.
#
# **The failure mode is a document that reads as authoritative while saying two things.** Git's markers
# are ordinary text to every consumer that is not git, so `<<<<<<< HEAD` and its closing marker are
# published like any other line. Nothing objected when it happened here: `spec/audit/passes.md`
# carried a conflict through a rebase, through the full formal gate, and into `dev` via a merged PR
# (2026-10-02), because every check in the tree reads a *structured* file — the register is
# `spec/laws.tsv`, the findings are `spec/findings.tsv`, `sorry` is searched for in `.lean` — and this
# file is prose. The two sides of that conflict disagreed about whether a test existed.
#
# **Why a source scan rather than a test.** There is nothing to execute: the fact is a property of the
# text, and the only question is whether anyone looks. Seconds to run, and it belongs in the job that
# fails when a gate regresses rather than in the tree waiting for someone to remember it.
#
# Only `<<<<<<< ` and `>>>>>>> ` are searched for, **never the bare `=======` separator**: seven
# equals at the start of a line is a legitimate markdown setext heading underline, so matching it
# would refuse a document for being well-formed. Git always writes both of the others, with a space
# and a ref name after them, so the pair is what a conflict is.
#
# **A run that inspected no file fails rather than passing.** An empty match is the success case here,
# which makes "the scan found nothing" and "the scan looked at nothing" the same output — the failure
# mode `tools/check-workflow-pins.sh` names for its own zero-match case. The count is checked first.

set -euo pipefail

cd "$(dirname "$0")/.."

tracked=$(git ls-files | wc -l)
if [ "$tracked" -eq 0 ]; then
  echo "no tracked files: the scan inspected nothing, which is not a pass" >&2
  exit 1
fi

# `git grep` searches tracked files only, so a build tree is never in scope; it exits 1 on no match,
# which is the outcome we want and not an error.
if hits=$(git grep -n -E '^(<<<<<<< |>>>>>>> )' -- .); then
  echo "unresolved merge conflict markers in $tracked tracked files:" >&2
  echo "$hits" >&2
  exit 1
fi

echo "check-no-conflict-markers: $tracked tracked files, no conflict markers"
