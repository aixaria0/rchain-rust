// **Does Agoric's own stack hold an RChain issuer and call it?** (issue #249, clause 4)
//
// Usage: node ertp-round-trip.mjs <port>   — against a running **node**, not the fixture binary:
// the node must have `api-server.ocapn-listen` set and a dev deployer key, because the bridge signs
// a deploy for every call and needs a chain to carry it.
//
// The earlier spike (`spike.mjs`, transcript in `run-1.txt`) dialled the standalone
// `ocapn-tcp-testing` peer and fetched the **echo** fixture: the handshake, the codec and the
// export/answer tables were exercised, but no ERTP object ever crossed, and clause 4's words — "a
// peer implementation fetches an RChain *issuer*" — were never met. This walks that path:
//
//   the ERTP contract → `makeIssuerKit` → the issuer → a purse → its balance
//
// The kit and the issuer are *capabilities*: they arrive as descriptors (CapTP imports) because the
// bridge registers what each call returns into the chain's registry and hands the peer a reference
// to it. The balance is data, and comes back as a value.
import '@endo/init';

import { makeClient } from '@endo/ocapn';
import { makeTcpNetLayer } from '@endo/ocapn/netlayer/tcp-testing';
import { E } from '@endo/eventual-send';

const port = Number(process.argv[2] ?? 22045);
const client = makeClient({ debugLabel: 'ertp', verbose: true });
await client.registerNetlayer((handlers, logger) =>
  makeTcpNetLayer({
    handlers,
    logger,
    specifiedHostname: '127.0.0.1',
    specifiedPort: 0,
    framing: 'syrup',
  }),
);

const location = {
  type: 'ocapn-peer',
  designator: 'rnode',
  transport: 'tcp-testing-only',
  hints: { host: '127.0.0.1', port: String(port) },
};

// The node publishes the ERTP object API under this swiss number (the contract's registry
// shorthand, looked up on chain).
console.log(`dialling ${location.designator}.${location.transport} on ${port}`);
const ertp = await client.enlivenSturdyRef(client.makeSturdyRef(location, 'rho:rchain:ertp'));
console.log('FETCHED the ERTP contract:', ertp);

// A kit is three capabilities — a brand, a mint and an issuer — so the reply is three descriptors.
const kit = await E(ertp).makeIssuerKit();
console.log('KIT:', kit);

const issuer = kit[2];
// One capability answers as one descriptor, not a list of one.
const purse = await E(issuer).makeEmptyPurse();
console.log('PURSE:', purse);

// A value reply: `(true, 0)`.
const amount = await E(purse).getCurrentAmount();
console.log('BALANCE:', amount);
process.exit(0);
