# The history chain

**A genesis-only, read-only chain carrying the REV allocation — dated 13 May 2026, frozen at that state
until it is updated with the documented final transactions.**

Unlike [the public testnet](testnet.md), this chain has **one block and never produces another**. It is a
public, queryable record rather than a network: no proposer, no peers, no finality, and no way to write to
it. It exists so that an address's allocation can be read from a chain instead of from a spreadsheet.

| | |
|---|---|
| Endpoint | **https://history.rhobot.net** (nginx → the node's HTTP API on the loopback) |
| Network id | `history` (its own, so it cannot be confused with `testnet`) |
| Chain | genesis block `f6aaa149…`, block number **0**, and nothing above it |
| Wallets | **13,946** REV addresses, **908,962,714 REV** in total (at 1e-8 precision) |
| Source of the allocation | [the allocation spreadsheet](https://docs.google.com/spreadsheets/d/1bLEn7WpgESNp8Hp8N9_-5Yf6XXRBMM5KbsH_-G4Fhks/edit) — `address, balance` |
| Host | `rhobot-2` (`138.197.65.34`), unit `rnode-history.service`, data `/var/lib/rnode-history` |

Short hashes here are the first twelve hex characters of the value they name. **The node id is not written
down** — a rebuild regenerates it, which is why [the testnet page](testnet.md) stopped naming its own; read
it from `GET /api/status` → `address`.

## Reading it: r-wallet

**R Wallet has this chain in its node dropdown** — choose **RChain → History (REV allocation)**. The
wallet talks to `https://history.rhobot.net` directly, so its Balance page answers for whatever address
is active; there is nothing to configure and no custom node to add.

What does *not* work there is deploying, by design: the deploy is refused with `403` and a plain-text
reason, because this chain has no proposer. Pick it to read an allocation, not to write one.

## Reading it: curl

The term below is **the wallet's own** (`src/utils/rho.ts`, `fn_check_balance` in the r-wallet repo), so
the two agree by construction:

```bash
ADDR=11112We8VJbQ…                 # any REV address

printf '"new return, vault(`rho:rchain:revVault`), ret in { vault!(\"getBalance\", \"%s\", *ret) | for (@b <- ret) { return!(b) } }"' \
  "$ADDR" > /tmp/balance.json

curl -s -X POST https://history.rhobot.net/api/explore-deploy \
  -H 'Content-Type: application/json' --data-binary @/tmp/balance.json
```

The answer is `{"expr":[{"ExprInt":<balance>}],"block":{…"blockNumber":0…}}`:

```bash
# just the number (in the chain's smallest unit, 1e-8 REV)
curl -s -X POST https://history.rhobot.net/api/explore-deploy \
  -H 'Content-Type: application/json' --data-binary @/tmp/balance.json | jq '.expr[0].ExprInt'

# what the chain is, and the genesis block
curl -s https://history.rhobot.net/api/status
curl -s https://history.rhobot.net/api/blocks/1
```

`0` means the address has **no vault** — it is an answer, not an error. Note the JSON body is a *bare
string* (the rholang term), not an object; posting an object gets `invalid type: map, expected a string`.

**Verified on 2026-09-30:** 8 of 8 sampled addresses return the sheet's own numbers — random draws plus
the largest allocation, the smallest non-zero, a balance of `1` and two zero balances — and a valid REV
address that is *not* in the sheet returns `0`. Every answer comes from the genesis block
(`blockNumber: 0`).

## What "read-only" means here, precisely

Three separate things, and it is worth knowing which one is doing the work:

1. **The node has no proposer.** It runs as genesis master (`-s`) exactly once, to build the block from
   the wallets file, and then never again: no `--propose-on-deploy`, no `--autopropose`. Nothing is ever
   mined, so a deploy would sit in the pool forever. This is the real mechanism.
2. **The edge refuses the write routes.** nginx answers `403` for `/api/deploy`, `/api/faucet`,
   `/api/txn` and `/api/v1/deploy`, so a caller is told the chain is read-only rather than being left to
   infer it from a deploy that never completes. `explore-deploy` and `data-at-name` are exploratory and
   change nothing, so they stay.
3. **There is no finality.** `GET /api/last-finalized-block` answers
   `"Finalized fringe is not available."` and always will — finality needs blocks to finalise and there
   are none. Reads anchor to the genesis block, which is the whole state.

So the chain is not a small testnet with writes switched off; it is a **snapshot with a read API**.

## How it was built

`wallets.txt` is derived from the allocation spreadsheet, in the form
[the vault parser](https://github.com/rchain-community/rchain-rust/blob/dev/casper/src/vault_parser.rs)
accepts — `<REV_address>,<balance>`, one per line:

- the sheet's `balance` column carries **thousands separators** (`1,000,000`); the parser wants bare
  digits, so the commas are stripped;
- the sheet's third column (`$0.004`) is not part of the chain;
- balances are used **as they are** — no scaling. The total is 908,962,714 REV at 1e-8 precision, which is
  the sanity check that the column is already in the chain's smallest unit;
- 13,946 addresses, all unique, all matching the parser's `[1-9A-Za-z]+` address rule; 209 of them have a
  zero balance and get an empty vault.

The genesis inputs are the ordinary two files:

```
/var/lib/rnode-history/genesis/wallets.txt   13,946 lines, from the spreadsheet
/var/lib/rnode-history/genesis/bonds.txt     one validator, stake 1000
```

and the validator key is a raw 64-hex secret at `/etc/rnode-history/validator.key` (the format the port
reads: `rnode keygen` cannot be used non-interactively — it loops on a password prompt and segfaults
without a TTY — so the key was generated directly and the public half derived from it).

PoS parameters: `--epoch-length 10 --quarantine-length 10 --bond-minimum 1 --bond-maximum 1000000
--number-of-active-validators 10`. They are written into the genesis block and, on a chain that never
advances, nothing reads them again.

A 13,946-vault genesis builds in well under a minute: the data directory is 3.7 MB and the node settles at
~54 MB RSS.

## Operating it

```
systemctl status rnode-history      # on rhobot-2
tail -f /var/log/rnode-history.log  # the node's log (this host's journald does not capture it)
```

Ports are **43400-based** (`--protocol-port 43400`, `--api-port-http 43403`, …) because the playground node
on the same host holds the whole 40400 family and the testnet join holds 42400. Only 80/443 are exposed;
nginx proxies the API from `127.0.0.1:43403`.

To rebuild the chain — which produces a **different genesis block**, and therefore a different chain —
stop the unit, replace `genesis/wallets.txt`, and delete `/var/lib/rnode-history` so the node starts from
an empty data directory. Then re-point anything holding the old genesis hash.
