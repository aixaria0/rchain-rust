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

# **The caveats are part of the transcript, not annotations around it.** They used to be a header
# written by hand into `run-1.txt`, which a re-run silently discarded — leaving a green result with
# nothing saying what it does not cover, which is the failure this directory exists to avoid. The
# header is emitted here instead, so every run carries its own limits.
cd "$HERE"
{
  cat <<'HEADER'
# The Noise interop run: Agoric's implementation against this repository's netlayer.
#
# inlined:   spec/audit/evidence/ocapn-noise/ref/lib.rs, after two mechanical edits (README.md):
#            `#![no_std]` dropped, and the `unsafe extern "C" { fn buffer_callback(..); }`
#            declaration dropped because the harness supplies that symbol (else E0428). The
#            reference's *calls* to it are untouched and resolve to the harness's definition.
#
# What this run establishes: the handshake completes ACROSS IMPLEMENTATIONS — the reference, in the
# responder role, accepted the frame this repository's `ocapn/src/noise.rs` produced (the SYN behind
# its 32-byte intended-responder prefix, and the ACK), and the signature checks on both sides hold —
# and transport messages decrypt in both directions.
#
# What it does NOT establish, said here so a green result is not read as more than it is:
#   * THE RECORD FRAMING. The reference binds `encrypt`/`decrypt` over one record of at most 65535
#     bytes and leaves the record *boundaries* to a netlayer, and up to and including 2026-10-07 it
#     ships none — so the length prefix and chunking in `ocapn/src/noise.rs` still have no counterpart
#     to be tested against. See the README's section on C227 for what was read and when.
#   * A LIVE PEER. This drives the reference's core from a harness — which is what the reference is
#     built to be driven by, its own JS binding doing the same — not a shipped implementation.
#   * THE DESIGNATOR CONVENTION. This harness passes the responder's Ed25519 verifying key in the
#     locator's `verify` hint (base16), because the locator convention is not pinned by anything
#     reachable — though the reference's own netlayer, where one exists, does use hex.
HEADER
  echo "# reference: endojs/endo rust/ocapn_noise at commit $COMMIT"
  echo "# digest:    $(cut -d' ' -f1 "$REF/lib.rs.sha256")  ref/lib.rs"
  echo "# tree:      $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "# command:   bash spec/audit/evidence/ocapn-noise/run.sh"
  echo "#"
} > "$HERE/run-1.txt"
cargo run --quiet 2>&1 | tee -a "$HERE/run-1.txt"
