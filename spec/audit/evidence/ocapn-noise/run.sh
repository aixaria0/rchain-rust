#!/usr/bin/env bash
#
# run.sh — reproduce the Noise interop run.
#
# Fetches Agoric's Noise core at a **pinned commit**, inlines it into the harness, drives it against
# `rchain-ocapn`'s `noise` netlayer, and writes the transcript to `run-1.txt`.
#
# **Why the fetch is at a commit and not a branch.** `@endo/ocapn-noise` is unpublished (the npm
# registry 404s on it) and described as experimental, so "the reference" is a source revision and
# nothing more stable than that. A run against `master` would be a run against a moving target; the
# pin plus the recorded digest is what makes this transcript mean anything later.
#
# **The one line that is changed.** The reference's `lib.rs` opens with `#![no_std]`, a *crate*
# attribute — meaningful only when that file is a crate root, and an error when inlined into a harness
# that needs `std` for its sockets. That line is removed; nothing else is touched, and the script
# refuses to run if the file no longer starts with it, because that would mean upstream changed shape
# and the digest below is stale in a way a quiet pass would hide.
set -euo pipefail

COMMIT="356d6e70affc5adfd35cd65adda758119521ec5f"
URL="https://raw.githubusercontent.com/endojs/endo/${COMMIT}/rust/ocapn_noise/src/lib.rs"

HERE="$(cd "$(dirname "$0")" && pwd)"
REF="$HERE/ref"
mkdir -p "$REF"

echo "fetching the reference at $COMMIT"
curl -fsSL "$URL" -o "$REF/lib.rs.raw"

if ! head -1 "$REF/lib.rs.raw" | grep -qx '#!\[no_std\]'; then
  echo "run.sh: the reference no longer opens with #![no_std] — upstream changed shape;" >&2
  echo "        read the new file before inlining it." >&2
  exit 1
fi
if ! grep -q 'fn buffer_callback(buffer: \*const u8);' "$REF/lib.rs.raw"; then
  echo "run.sh: the reference no longer declares the host symbol in the shape this strips;" >&2
  echo "        read the new file before inlining it." >&2
  exit 1
fi

# Two mechanical edits, both stated in the transcript:
#   1. drop `#![no_std]`            — a crate attribute, an error when inlined into a std harness;
#   2. drop the `buffer_callback`   — a *declaration* of the one host symbol. Inlined into one crate
#      `extern "C" { … }`  block        it collides with the definition `main.rs` supplies (E0428);
#                                       the calls below it still resolve, to that definition.
tail -n +2 "$REF/lib.rs.raw" \
  | awk '
      /^unsafe extern "C" \{/ { skip = 1 }
      skip && /^\}/          { skip = 0; next }
      skip                   { next }
      { print }
    ' > "$REF/lib.rs"
rm -f "$REF/lib.rs.raw"

if grep -q 'fn buffer_callback(buffer: \*const u8);' "$REF/lib.rs"; then
  echo "run.sh: the host-symbol declaration survived the strip — check the awk above" >&2
  exit 1
fi
if ! grep -q 'buffer_callback(BUFFER.as_ptr())' "$REF/lib.rs"; then
  echo "run.sh: the host-symbol *calls* were removed too — the strip is too broad" >&2
  exit 1
fi

# What was actually inlined, so a later reader can tell whether the pin still resolves to this text.
sha256sum "$REF/lib.rs" | tee "$REF/lib.rs.sha256"

cd "$HERE"
cargo run --quiet 2>&1 | tee "$HERE/run-1.txt"
