#!/usr/bin/env bash
#
# run-ws.sh — reproduce the websocket interop run.
#
# Starts a node with a `websocket` listener, waits for it to come up, dials it with **Agoric's**
# `@endo/ocapn` over its websocket netlayer, and writes the transcript to `run-4.txt`.
#
# **`run-3.txt` is kept as it was**: the recorded *hang*, where the client established the session and
# then sent nothing. C245 is the finding that it was this node's advertised location rather than the
# peer's client, and `run-4.txt` is the same harness after the fix — `FETCHED`, then `CALL REPLY`.
#
# **The peer is a published package.** `@endo/ocapn` 1.1.1 from npm — the same one `run-1`/`run-2` used
# — installed under `target/endo-spike` by the recipe in README.md. Nothing here is this repository's
# own client, which is the point: `node/tests/ocapn_listener.rs` already proves our two ends agree.
#
# Usage: bash spec/audit/evidence/endo-spike/run-ws.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
DATA="$(mktemp -d /tmp/rnode-ws-interop.XXXXXX)"
PORT=22060
IDENTITY="$DATA/ocapn-identity.key"

cleanup() {
  [[ -n "${NODE_PID:-}" ]] && kill "$NODE_PID" 2>/dev/null || true
  wait "${NODE_PID:-}" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$DATA/genesis"
# The deployer's REV account, so a bridged deploy has phlo to spend. The address is the same throwaway
# one `node/tests/common/mod.rs` uses, and matches the key in `ws-node.conf`.
printf '11112VYAt8rUGNRRZX3eJdgagaAhtWTK8Js7F7X5iqddMVqyDTtYau,1000000000000\n' \
  > "$DATA/genesis/wallets.txt"
cp "$HERE/ws-node.conf" "$DATA/rnode.conf"

echo "building rnode"
(cd "$ROOT" && cargo build -p rchain-node --bin rnode >/dev/null)

echo "starting the node on websocket :$PORT, identity $IDENTITY"
"$ROOT/target/debug/rnode" run --data-dir "$DATA" --ocapn-identity-key "$IDENTITY" \
  > "$DATA/node.log" 2>&1 &
NODE_PID=$!

# Wait for the identity file. **It is not a liveness signal**: `load_or_create_noise_identity` runs in
# `setup_node_program`, a *config* phase, strictly before `serve_ocapn` binds — so the file can appear
# before the listener exists. It is a *precondition* (the dialler needs it to name the node) and the
# sleep below is what actually covers the gap.
for _ in $(seq 1 600); do
  [[ -f "$IDENTITY" ]] && break
  sleep 0.1
done
if [[ ! -f "$IDENTITY" ]]; then
  echo "the node never wrote its identity; see $DATA/node.log" >&2
  tail -20 "$DATA/node.log" >&2
  exit 1
fi
# And a moment for the accept loop, which is the part the identity file's existence does NOT prove.
sleep 1

# `@endo/ocapn` is installed under `target/endo-spike` (README.md's recipe, and the same install
# `run-1`/`run-2` used). Node resolves an import from the **importing file's** directory, so the
# script is copied beside those modules and run from there — the committed copy is the record.
if [[ ! -d "$ROOT/target/endo-spike/node_modules/@endo/ocapn" ]]; then
  echo "no @endo/ocapn under target/endo-spike — run the install in README.md first:" >&2
  echo "  npm install --prefix target/endo-spike @endo/ocapn" >&2
  exit 1
fi
cp "$HERE/ws-round-trip.mjs" "$ROOT/target/endo-spike/ws-round-trip.mjs"

echo "dialling with @endo/ocapn"
# **Bounded**, so a dial that stalls is a named failure rather than a job that sits here: a harness
# that can hang for half an hour and print nothing is worse than one that says it timed out.
(cd "$ROOT/target/endo-spike" && timeout 120 node ./ws-round-trip.mjs "$IDENTITY" "$PORT") 2>&1 \
  | tee "$HERE/run-4.txt"
if [[ "${PIPESTATUS[0]}" == "124" ]]; then
  echo "the dial did not finish within 120s" >&2
  exit 1
fi
