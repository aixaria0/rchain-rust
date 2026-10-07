// agoric-side: does `@endo/ocapn`'s **websocket** netlayer reach this repository's node?
//
// This is `spike.mjs`'s counterpart for the second transport. What makes it a real interop run rather
// than a self-consistency one: the peer is Agoric's own implementation, and it authenticates the node
// with an **in-band challenge** — it sends `init:peer-auth`, the node signs the bytes it received, and
// this script checks that signature against the key it dialled. A node that could not produce that
// signature would be refused here, and one that signed something else would be refused too.
//
// Usage: node ws-round-trip.mjs <identity-file> <port> [swiss]
//
// **The designator is the key.** Endo encodes a peer's Ed25519 public key as base32 into the locator's
// `designator` and decodes it back to check the challenge (`base32Decode(remoteLocation.designator)`),
// so this script has to express this node's verifying key the same way — which it derives from the
// identity file the node itself wrote.

import '@endo/init';

import { createPrivateKey, createPublicKey } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { makeClient } from '@endo/ocapn';
import { makeWebSocketNetLayer } from '@endo/ocapn/netlayer/ws';
import { E } from '@endo/eventual-send';

/// The reference's own alphabet (`BASE32_ALPHABET` in its websocket netlayer): lowercase, no padding.
const BASE32_ALPHABET = 'abcdefghijklmnopqrstuvwxyz234567';

const base32Encode = bytes => {
  let value = 0;
  let bits = 0;
  let output = '';
  for (const byte of bytes) {
    value = value * 256 + byte;
    bits += 8;
    while (bits >= 5) {
      const divisor = 2 ** (bits - 5);
      const index = Math.floor(value / divisor);
      output += BASE32_ALPHABET[index];
      value -= index * divisor;
      bits -= 5;
    }
  }
  if (bits > 0) {
    output += BASE32_ALPHABET[value * 2 ** (5 - bits)];
  }
  return output;
};

/// The node's Ed25519 **verifying** key, from the seed it stores. Node will build a key object from
/// the raw seed if it is wrapped in the fixed PKCS#8 prefix Ed25519 uses, and the public half is the
/// last 32 bytes of the exported SPKI.
const verifyingKeyOf = identityFile => {
  const stored = readFileSync(identityFile);
  if (stored.length !== 64) {
    throw Error(`expected a 64-byte identity, got ${stored.length}`);
  }
  const pkcs8 = Buffer.concat([
    Buffer.from('302e020100300506032b657004220420', 'hex'),
    stored.subarray(0, 32),
  ]);
  const spki = createPublicKey(
    createPrivateKey({ key: pkcs8, format: 'der', type: 'pkcs8' }),
  ).export({ type: 'spki', format: 'der' });
  return spki.subarray(spki.length - 32);
};

const identityFile = process.argv[2];
const port = Number(process.argv[3] ?? 22060);
const swiss = process.argv[4] ?? 'IO58l1laTyhcrgDKbEzFOO32MDd6zE5w';

const verifyingKey = verifyingKeyOf(identityFile);
console.log(`node's Ed25519 verifying key: ${verifyingKey.toString('hex')}`);

const client = makeClient({ debugLabel: 'ws-interop', verbose: true });
await client.registerNetlayer((handlers, logger) =>
  makeWebSocketNetLayer({ handlers, logger, specifiedPort: 0 }),
);

const location = {
  type: 'ocapn-peer',
  designator: base32Encode(verifyingKey),
  transport: 'websocket',
  hints: { url: `ws://127.0.0.1:${port}` },
};

console.log(`dialling ${location.designator} over websocket at ${location.hints.url}`);
const ref = client.makeSturdyRef(location, swiss);
const obj = await client.enlivenSturdyRef(ref);
console.log('FETCHED', typeof obj, obj);

// Call it: the echo fixture replies with its arguments, so one round trip proves the whole path — the
// challenge, the CapTP handshake, the codec and the export/answer tables.
const reply = await E(obj).echo('foo', 1n, false);
console.log('CALL REPLY', reply);
process.exit(0);
