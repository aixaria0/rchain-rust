# Endo interop: a second foreign implementation

**What this is.** `@endo/ocapn` — **Agoric's own OCapN implementation**, the stack the request came
from — dialled this repo's `ocapn-tcp-testing` peer, fetched a sturdyref, called the object and got
a reply. Until now the only foreign implementation the port had spoken to was the OCapN project's
Python conformance suite; this is the one Agoric actually ships.

| | |
|---|---|
| Date | 2026-10-05 |
| `run-1.txt` under test | `rchain-ocapn` on `ocapn/ertp-interop` — the **fixture binary** (`cargo build -p rchain-ocapn --bin ocapn-tcp-testing`) |
| `run-2.txt` under test | **`rnode` itself** (`target/debug/rnode`, `spec/audit/evidence/endo-spike/node.conf`) — the ERTP round trip below |
| Peer | `@endo/ocapn` **1.1.1** on Node **v22.22.2**, `framing: 'syrup'` |
| Result | **handshake → `fetch` → call → reply, all green** (run-1) and **the ERTP round trip against a node** (run-2) |

`run-1.txt` is the full transcript. The operative lines:

```
FETCHED object Object [Alleged: Remote Object 1] {}
CALL REPLY [ Symbol(echo), 'foo', 1n, false ]
```

The reply carries a string, a bigint and a boolean — so the Syrup codec, the handshake, the session
id, the export/answer tables and the `fulfill` path are all exercised against a foreign peer, not
just against our own round-trip test.

## The framing question is settled

OCapN's TCP-for-testing netlayer is specified as "raw Syrup, no length prefix", and Endo keeps a
`framing: 'none'` mode for that described wire while **defaulting to `'syrup'`**, which it describes
as `<length>:<payload>`. Those two descriptions cannot both match the Python suite — and they do not:

| framing | against this port | why |
|---|---|---|
| `'syrup'` (our `netstring.rs`) | ✅ round trip | `<length>:<payload>`, which is exactly what the Python suite's `to_netstring` writes (`length.encode() + b":" + payload`, no trailing comma) |
| `'none'` | ❌ `Connection closed during handshake` | bare Syrup, which the Python suite would not accept either — its netlayer netstrings |

So Endo's `'none'` comment ("interoperate with the existing Python `ocapn-testing-suite` … no length
prefix") is **wrong about the Python suite**, and `'syrup'` is the framing all three converge on.
That is why this port's netstring framing is correct and needs no change.

## A divergence this found — the swiss number's type

The first run failed, and the failure is the finding. **That run's output was not kept** (the
transcript in `run-1.txt` is the successful run that followed it, and the HAZOP's row E5 is the
finding that this page used to quote a line appearing in no artifact). What it said, reconstructed
from the fix it forced: the fetch broke with a reason naming the swiss number's type.

Endo sends the swiss number as a **Syrup String**; the Python suite sends a **byte array**
(`b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w"`); the Locators draft calls it a string. **The two reference
implementations disagree with each other**, so "the reference implementation is the oracle" has no
single oracle here. `Bootstrap::deliver` now accepts either and keys its directory by bytes, which
is what lets one peer serve both. Recorded as AUDIT C217.

## How to reproduce

```sh
cargo build -p rchain-ocapn --bin ocapn-tcp-testing
./target/debug/ocapn-tcp-testing 127.0.0.1:22051 &
mkdir -p target/endo-spike && npm install --prefix target/endo-spike @endo/ocapn
node target/endo-spike/spike.mjs 22051 syrup
```

`spike.mjs` imports `@endo/init` first — Endo's packages need the SES bootstrap to install the
`assert` global they rely on. The script is a scratch artefact (`target/` is ignored); the
transcript above is the record.

## The ERTP round trip, against a **node** (run-2)

**What this is, and what the first spike was not.** `run-1.txt` dialled the standalone
`ocapn-tcp-testing` **fixture binary** and fetched the *echo* fixture: the handshake, the codec and
the export/answer tables crossed, but no ERTP object ever did, and no chain was involved. Issue
#249's clause 4 — "a peer implementation fetches an RChain issuer over OCapN and the round trip is
demonstrated" — was therefore **not met**, and the record said otherwise until the exploration
behind this run caught it.

`run-2.txt` meets it. `ertp-round-trip.mjs` dials a **running node** (the same `@endo/ocapn`
1.1.1) and walks the path the clause names:

```
FETCHED the ERTP contract: Object [Alleged: Remote Object 1] {}
KIT: [ Remote Object 2, Remote Object 3, Remote Object 4 ]     # brand, mint, issuer
PURSE: Object [Alleged: Remote Object 5] {}
BALANCE: [ true, 0n ]
```

Each call is a CapTP delivery that becomes a **signed deploy**, lands in a block, and answers from
the value that deploy put on its reply channel; the kit's three members and the purse are
unforgeable names that crossed as descriptors, so the peer holds live references to objects on
chain and never sees a registry URI.

The node is configured by `node.conf` (this directory): `dev-mode`, one bonded validator,
`propose-on-deploy`, and an OCapN listener. Reproduce with:

```sh
mkdir -p $DATA/genesis
printf '<validator pubkey> 100\n'            > $DATA/genesis/bonds.txt
printf '<deployer REV address>,1000000000000\n' > $DATA/genesis/wallets.txt
cp spec/audit/evidence/endo-spike/node.conf  $DATA/rnode.conf
target/debug/rnode run --data-dir $DATA &
node spec/audit/evidence/endo-spike/ertp-round-trip.mjs 22050
```

(`bonds.txt` needs the *public* half of `casper.validator-private-key`; `wallets.txt` needs the REV
address of `dev.deployer-private-key`, which is the key the bridge signs its deploys with — a
bridged deploy pays phlo out of that account.)

**Three defects this run found**, each invisible to the Python suite and each now a test or a
comment where it happened:

| found | what it was | fix |
|---|---|---|
| `preCharge: insufficient funds (0 < 1000000)` | the bridge registered a returned capability with `insertSigned`, whose URI comes from the *deployer* key — so a fresh key per object meant a deploy with no REV | `rho:registry:insertArbitrary`, which mints a fresh URI with no key, so the node's funded key signs |
| an empty reply where a purse was expected | the term bound the member with `for (@(_, root) <- cap)`, and a tuple pattern is **exact**: an ERTP kit's reply is `(brand, mint, issuer)`, so the 2-element pattern fell straight through and the deploy answered nothing | the pattern is built from the value's real shape |
| `Unexpected type "boolean", Syrup record labels must be strings, selectors, or bytestrings` | `par_value` mapped `ETuple` to a Syrup **record**; records are labelled, and `(true, 0)` made the label a boolean | a tuple crosses as a Syrup **list** |

