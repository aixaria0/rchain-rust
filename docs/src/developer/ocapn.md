# Talking to a node from another implementation

This page is for a developer whose code is **not Rholang**: you have an Agoric vat, an `@endo/ocapn`
client, or another CapTP implementation, and you want to hold and call objects on a running node.

For what the node exposes and how to configure it, see [OCapN interoperability](../node/ocapn.md).
This page is the client side: connect, fetch, call, read.

## 1. Turn the listener on

The listener is off by default. On the node you are dialling, set:

```hocon
api-server {
  ocapn-listen = "127.0.0.1:22045"
}
```

A chain-backed capability — anything an ERTP or REV call reaches — needs the node to have a deployer
key as well, because every call becomes a signed deploy:

```hocon
dev {
  deployer-private-key = "<hex secp256k1 private key>"
}
```

Without a key the node still completes a handshake and serves the conformance fixtures, but there is
no chain behind it to answer a call.

> **Bind loopback unless you mean it.** `tcp-testing-only` is plain TCP with no authentication, and a
> peer that connects can make the node spend its own REV as a deployer. See the warning in
> [OCapN interoperability](../node/ocapn.md) before publishing the port.

## 2. Dial

The netlayer is `tcp-testing-only`, and messages are netstring-framed (`<length>:<payload>`). With
Endo, the framing is `'syrup'` — its `'none'` mode interoperates with nothing.

```js
import '@endo/init';
import { makeClient } from '@endo/ocapn';
import { makeTcpNetLayer } from '@endo/ocapn/netlayer/tcp-testing';
import { E } from '@endo/eventual-send';

const client = makeClient({ debugLabel: 'rnode', verbose: true });
await client.registerNetlayer((handlers, logger) =>
  makeTcpNetLayer({
    handlers,
    logger,
    specifiedHostname: '127.0.0.1',
    specifiedPort: 0,
    framing: 'syrup',
  }),
);
```

From Rust, the same dial is a `Session`:

```rust
use std::sync::Arc;

use rchain_ocapn::bootstrap::Bootstrap;
use rchain_ocapn::conn::{Identity, Session};
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

let locator = PeerLocator {
    designator: "rnode".into(),
    transport: "tcp-testing-only".into(),
    hints: [("host".to_string(), "127.0.0.1".to_string()),
            ("port".to_string(), "22045".to_string())].into(),
};
let dialer = TcpTestingOnly::bind("127.0.0.1:0").await?;
let connection = dialer.new_outgoing_connection(&locator).await?;
let identity = Identity::fresh(locator.clone())?;
let mut session = Session::dial(connection, &identity, Arc::new(Bootstrap::default())).await?;
```

The designator is the label *you* give the peer in the locator you dial; it does not have to match the
node's own.

## 3. Fetch a capability

A peer is handed the node's **bootstrap object** at export 0. `fetch(swiss)` on it resolves a swiss
number to a capability and gives you back a reference you can call.

```js
const location = {
  type: 'ocapn-peer',
  designator: 'rnode',
  transport: 'tcp-testing-only',
  hints: { host: '127.0.0.1', port: '22045' },
};

const ertp = await client.enlivenSturdyRef(
  client.makeSturdyRef(location, 'rho:rchain:ertp'),
);
```

The node publishes two chain-backed swiss numbers — `rho:rchain:ertp` and
`rho:rchain:revVault/getBalance` — plus the conformance fixtures. In Rust the fetch is an `op:deliver`
to export 0:

```rust
use rchain_ocapn::captp::{Deliver, Desc};
use rchain_ocapn::syrup::Value;

let fetch = Deliver {
    to: Desc::Export(0u64.into()),
    args: vec![
        Value::Symbol("fetch".into()),
        Value::Bytes(b"rho:rchain:ertp".to_vec()),
    ],
    answer_pos: None,
    resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
};
session.send_message(&fetch.to_syrup()).await?;
```

## 4. Call it

A delivery names a method and arguments, and the reply is either a value or a reference to another
object. When a call **returns a capability**, you get an object you can call in turn — that is what
makes the ERTP round trip work.

```js
// A kit is three capabilities: a brand, a mint and an issuer.
const kit = await E(ertp).makeIssuerKit();
const issuer = kit[2];

const purse = await E(issuer).makeEmptyPurse();
const balance = await E(purse).getCurrentAmount();   // [true, 0]
```

Two behaviours to expect:

- **Every call is a deploy.** It is signed by the node's key and submitted; the reply arrives when a
  block carrying it is produced, so a call takes a block interval, not a round trip. The node pays the
  phlo. On a node with autopropose off, you must cause a block.
- **Capabilities cross as references, values cross as data.** The kit's members and the purse above
  are remote references — the underlying Rholang names never leave the node. A `(true, 0)` reply comes
  back as the list `[true, 0]`.

## 5. What works today

| | |
|---|---|
| fetch a capability from the bootstrap | yes |
| call an arm that takes plain values | yes — strings, numbers, booleans, byte arrays, lists |
| call an arm that takes **a capability** | yes — pass a remote reference as an argument |
| hold a returned capability and call it | yes |
| pass an **amount** (`(brand, value)`) back to the node | **no** |

The last row is the current edge. A tuple crosses to a peer as a **list**, and a list coming back does
not match a contract's `(brand, value)` tuple pattern. So the ERTP arms that read or take no amount —
`getCurrentAmount`, `makeEmptyPurse`, `getBrand` — are reachable, while the arms that *move* value —
`mintPayment(amount)`, `withdraw(amount)`, `revFund(funder, amount)` — are not. A peer can make a
purse and read it, but cannot yet put anything in it. See [ERTP](../node/ertp.md) for the ledger
underneath and [`spec/AUDIT.md`](../../../spec/AUDIT.md) (C226) for the wire-shape decision it needs.

Also note:

- **Bridged deploys are rate-limited** to 4 per second across every session and capability; past that
  a delivery is refused with a reason naming the bound.
- **The node is the on-chain caller.** Every bridged deploy is signed by the node's deployer key, so
  the chain sees the node, not you. There is no session-to-deployer binding yet.
- **The listener serves a fixed number of sessions** (64). Past that, new connections are closed.

## See also

- [OCapN interoperability](../node/ocapn.md) — the node side: config, the bridge, the wire.
- [ERTP](../node/ertp.md) — the object API, and its ledger.
- `spec/audit/evidence/endo-spike/` — a transcript of `@endo/ocapn` against a node, and the node
  config it ran with.
