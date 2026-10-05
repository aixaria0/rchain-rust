#!/usr/bin/env bash
# audit-vendored-sources.sh — hold the vendored oracle contracts to the originals they came from.
#
# The blessed system contracts are **the oracle's own source text**, vendored into the node and
# compiled in with `include_str!` (`casper/src/genesis/standard_deploys.rs`). Editing one is the
# sharpest form of "the port quietly diverged from the specification": the contract still parses,
# still installs, still passes its tests, and nothing records that the text is no longer the oracle's.
#
# It has happened twice, and both were found by hand — by diffing the tree against `legacy/` while
# reading for something else — which is why this is a script:
#
#   * `RevVault.rho` — its block-data consumer was rewritten from the oracle's two-element pattern
#     (`for (@blockNumber, @sender <- bdCh)`) to three, to match the port's `rho:block:data` reply.
#     The behaviour is registered (`spec/API-SCHEMA.md`'s row for that urn, and AUDIT C46's
#     neighbourhood), but the *source edit* was not recorded anywhere, and nothing diffed it.
#   * `Registry.rho` — the `rho:id:` shorthand URIs are re-derived for the rust-first z-base-32
#     encoding instead of the Scala's CRC14 + 270-bit `ZBase32` (registered: AUDIT §10 F5).
#
# What it checks: every file under `casper/src/genesis/resources/` is diffed by **basename** against
# its `legacy/**/resources/` original.
#   1. identical            → ok
#   2. differs              → must be an allowlisted entry naming the register row that justifies it
#   3. no `legacy/` original → its own class, reported but not a failure: the port's own contracts,
#                             which have no upstream text to be held to. That is the `rgov/` set (ten
#                             `*.rho` and the directory's `NOTICE`) and, since issue #249, `ERTP.rho`
#                             — declared by name in `OWN` with the reason, because a classification
#                             that cannot tell "ours" from "theirs" is not one. The count is printed
#                             rather than assumed.
#
# The allowlist burns down rather than rotting: an entry whose file is now *identical* to its
# original is a failure, not a pass — a justified difference that no longer exists is exactly the
# kind of row that keeps a check looking green after it stopped checking anything.
#
# Usage: tools/audit-vendored-sources.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RES="$ROOT/casper/src/genesis/resources"
ORIGINALS="$ROOT/casper/src/genesis/vendored-originals"
failures=0
fail() { printf 'FAIL  %s\n' "$*"; failures=$((failures + 1)); }
ok() { printf 'ok    %s\n' "$*"; }

# A justified difference, and where it is registered. The value is the citation, so a reviewer can
# check that the reason still holds rather than trusting this file's summary of it.
declare -A ALLOW=(
  ["RevVault.rho"]="spec/API-SCHEMA.md's \`rho:block:data\` row: the consumer takes the port's three-element reply"
  ["Registry.rho"]="AUDIT §10 F5: the \`rho:id:\`/z-base-32 shorthand URIs replace the Scala's ZBase32 ones"
)

# **The port's own contracts, by name, with the reason each has no original.** These are not vendored
# text and there is nothing upstream to hold them to, so they are a *classification* rather than a
# justified difference — which is why they are declared here instead of being allowlisted as
# divergences. A name is required per file (rather than a path prefix alone) so that a new "ours"
# file has to say why, and so that the set can be read in one place. The reason is the citation, as
# in `ALLOW`.
declare -A OWN=(
  ["ERTP.rho"]="issue #249: our own ERTP object API, installed at a key derived from a named string (spec/GENESIS.md)"
)

if [ ! -d "$RES" ]; then
  fail "$RES does not exist — a check with no inputs is vacuously green"
  echo ""
  echo "===== $failures check(s) FAILED ====="
  exit 1
fi

same=0
no_original=0
declare -A seen_allow=()

while IFS= read -r path; do
  base="$(basename "$path")"
  rel="${path#"$ROOT"/}"

  # The originals are **vendored in this repository** rather than read out of `legacy/`, which was
  # archived away on 2026-09-28. Vendoring them is what stops this gate going **vacuously green**: the
  # old lookup searched `legacy/`, and a missing original was *reported, not failed* — so deleting that
  # tree would have turned every file into "no original" and left the check passing while comparing
  # nothing, which is the failure mode this file's own header warns about. A file directly under
  # `resources/` must now have an original here; only the port's own `rgov/` set may have none.
  # The port's own files: the `rgov/` set by path, and anything declared in `OWN` by name. The reason
  # is chosen into a variable rather than written as a `${var:-…}` default because **an apostrophe
  # inside a parameter expansion opens a single-quoted string that bash then hunts for to EOF** —
  # `bash -n` refuses the file at its last line, with a message about the *other* quote character, so
  # the symptom points away from the line that caused it.
  own_reason=""
  if [[ "$rel" == *"/rgov/"* ]]; then
    own_reason="the port's own rgov set"
  elif [[ -n "${OWN[$base]:-}" ]]; then
    own_reason="${OWN[$base]}"
  fi
  if [ -n "$own_reason" ]; then
    echo "  no original expected: $rel ($own_reason)"
    no_original=$((no_original + 1))
    continue
  fi

  original="$ORIGINALS/$base"
  if [ ! -f "$original" ]; then
    fail "$rel has no original at vendored-originals/$base — the comparison cannot be made, and a check that cannot compare is not a check"
    continue
  fi

  if diff -q "$original" "$path" >/dev/null 2>&1; then
    if [ -n "${ALLOW[$base]:-}" ]; then
      fail "$base is identical to its original again — the difference '${ALLOW[$base]}' no longer exists, so its allowlist entry should be removed"
    else
      same=$((same + 1))
    fi
    continue
  fi

  if [ -n "${ALLOW[$base]:-}" ]; then
    echo "  differs, allowlisted: $rel — ${ALLOW[$base]}"
    seen_allow["$base"]=1
    continue
  fi

  fail "$rel differs from $original and is not allowlisted:"
  diff "$original" "$path" | head -24 | sed 's/^/      /'
done < <(find "$RES" -type f | sort)

for base in "${!ALLOW[@]}"; do
  # Only a name with no file behind it is stale here: a file that exists and is now *identical* is
  # already reported above, with the message that says so, and reporting it twice (once as "no such
  # file was checked") would describe the wrong thing.
  if [ ! -f "$RES/$base" ]; then
    fail "the allowlist names $base, which does not exist — a stale entry"
  fi
done

# The own-list burns down the same way: a declared "ours" whose file is gone is a name claiming a
# classification nothing carries, and leaving it would let the next `ERTP.rho`-shaped file arrive
# unclassified while the list still looked inhabited.
for base in "${!OWN[@]}"; do
  if [ ! -f "$RES/$base" ]; then
    fail "the own-list names $base, which does not exist — a stale entry"
  fi
done

echo ""
if [ "$failures" -gt 0 ]; then
  echo "===== $failures vendored-source check(s) FAILED ====="
  exit 1
fi
echo "ok    vendored contracts: $same identical to their originals, ${#seen_allow[@]} allowlisted difference(s), $no_original file(s) with no original (the port's own set)"
echo "===== the vendored sources match their originals ====="
