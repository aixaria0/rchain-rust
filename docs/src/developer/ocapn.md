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

## 6. Putting a shard in front of the peers

Everything above serves **one node**. The bridge signs every delivery with that node's key
(`dev.deployer-private-key`), so the chain's `deployerId` is the node and the node's REV pays the phlo.
That is fine for a demo and wrong for a boundary you publish: a peer's call spends your REV, and
nothing on chain records whose call it was.

A deployment that fixes the *shape* of that uses machinery already here: make the boundary a **shard**.

A node is a member of the shards listed in `casper.shards` — file-configured only, the first entry is
the primary and later ones get their own data directory under `<data-dir>/shard/`. A node with more
than one membership is a **gateway**; a shard whose only bonded key is that node is a **single-node
shard**. So:

```hocon
casper {
  shards = [
    { shard-name = root,  parent-shard-id = / }
    { shard-name = ocapn, parent-shard-id = /root, genesis-block-data { ... } }
  ]
  # The cross-shard routes, on the ADMIN server. Only a node with more than one shard
  # membership and a validator key serves them; the coordinator signs each leg with that
  # key and spends from its own REV account (AUDIT C121).
  enable-txn-api = true
}
api-server {
  ocapn-listen = "127.0.0.1:22045"
}
dev {
  deployer-private-key = "<hex secp256k1 private key>"
}
```

The peers dial that node. The ERTP objects they hold live in `root/ocapn`'s state, and a call that has
to reach the rest of the network leaves as a **cross-shard transaction** — a two-phase commit this node
coordinates (`casper/src/txn_coordinator.rs`, Laws 26–29).

**What it buys.** The perimeter becomes a *named shard* rather than "the node", and the spend becomes
bounded and visible: one shard's REV, one validator key, a shard-level fact — instead of the node's
own key paying for calls it did not make. The txn routes' config note says the same thing in its own
words: the coordinator "signs each leg with this node's validator key and spends from that key's own
REV account".

**Why it is not a trick.** The coordinator's record is deliberately **node-local, not consensus state**
(`casper/src/gateway/ledger.rs`, first paragraph): writing one node's off-chain coordination into the
content-addressed trie would make a shard's state hash depend on that node's off-chain work and diverge
consensus. That is the same rule the OCapN session already follows — the export and answer tables are
per-session and never chain state — so a shard acting as the boundary does nothing a gateway does not
already do.

**What it costs, and what it does not fix.** Each call that crosses becomes a 2PC transaction: atomic,
but dearer than today's single deploy, and it has a coordinator that can stall. A one-key shard
localises trust rather than removing it — the boundary's integrity is that validator. And the peer
still is not the payer: the boundary shard's key is *yours*. That obligation is stated, and left open,
in `spec/Rchain/Attribution.lean`; it closes only when the peer is itself a chain, which is what the
next section is about.

**Status.** The pieces are built and tested separately — `node/tests/gateway.rs` runs two shards in one
node and coordinates a two-shard transaction, and the bridge's tests are above — but the *combination*
in this section has not been run in this repository. It is the deployment these parts add up to, not a
recipe anyone has followed.

## 7. Writing a transport

The node speaks exactly one: `tcp-testing-only`, the conformance suite's own transport — plain TCP, no
encryption, no authentication, which is why the listener is off unless you name an address. A second
transport is small, because the seam is two functions (`ocapn/src/netlayer.rs`):

```rust
async fn new_outgoing_connection(&self, locator: &PeerLocator) -> io::Result<Box<dyn NetConn>>;
async fn accept_incoming_connection(&self) -> io::Result<Box<dyn NetConn>>;
```

plus `NetConn::peer_address` and a provided `new_outgoing_connection_from(locator, origin)`, which
exists so the dial policy can tell a *remote* peer from a local one (see the node page). Three things
to know before writing one:

- **The locator already names the transport.** It is `ocapn://<designator>.<transport>`, and designator
  plus transport *is* the spec's peer identity (`ocapn/src/locator.rs`) — so a new transport is a name
  plus whatever hints it needs. What does not exist yet is per-transport dispatch: the node holds one
  `Arc<dyn Netlayer>`.
- **`recv` promises a bidirectional FIFO and nothing else.** Not liveness, not that the session stays
  up. CapTP above assumes it does, and `op:abort` has no analogue on a packet network — so decide what
  a session means when a packet is merely late before writing one.
- **A transport built on a chain gives you the payer.** Packets on IBC are sent by a chain that pays
  for its own gas, which closes the attribution obligation this repository states and leaves open. That
  is the strongest reason to want one.

## See also

- [OCapN interoperability](../node/ocapn.md) — the node side: config, the bridge, the wire.
- [ERTP](../node/ertp.md) — the object API, and its ledger.
- `spec/audit/evidence/endo-spike/` — a transcript of `@endo/ocapn` against a node, and the node
  config it ran with.
