# Endo interop: a second foreign implementation

**What this is.** `@endo/ocapn` — **Agoric's own OCapN implementation**, the stack the request came
from — dialled this repo's `ocapn-tcp-testing` peer, fetched a sturdyref, called the object and got
a reply. Until now the only foreign implementation the port had spoken to was the OCapN project's
Python conformance suite; this is the one Agoric actually ships.

| | |
|---|---|
| Date | 2026-10-05 |
| Implementation under test | `rchain-ocapn`, branch `ocapn/ertp-interop`, `cargo build -p rchain-ocapn --bin ocapn-tcp-testing` |
| Peer | `@endo/ocapn` **1.1.1** on Node **v22.22.2**, `framing: 'syrup'` |
| Result | **handshake → `fetch` → call → reply, all green** |

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

The first run failed, and the failure is the finding:

```
args: [ Symbol(break), 'fetch expects a byte-array swiss number' ]
```

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
