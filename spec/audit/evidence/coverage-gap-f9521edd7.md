# What the audit apparatus has not covered

**Revision.** `dev` at `f9521edd7`. Read-class: every claim below is re-derived from the tree, and the
ones I did not open myself are marked `agent-reported` and named as such. None is armed with a run.

## 1. The gap, with evidence

The union of every study node across all three passes — the wargame's W-N1…N14, the acceptance
worksheet's U1…U10, `failure-hazop.md`'s U11…U16 — is: transport, deploy ingress, public read, admin,
key lifecycle, host, build, OCapN, validator, observability, upgrade/DR, DoS, entropy, clock, block
production, attestation, finality, merge, sync/join, PoS, deploy pool, wire, storage, operator surface,
reset, rewind, runtime root, refusal budget, RSpace checkpoint, reconciliation.

**Every one of those is about running a node. None is about the chain.** The thing this software exists
to do — let strangers run Rholang with value at stake — has no node in any pass. Three facts make that
precise:

- **`spec/review-ledger.tsv:598-627`** lists every one of the **30 system processes** as `T3 deferred`
  (not deep-read): `rho:io:http`, `rho:io:stdout`/`stderr`/`*Ack`, all five `rho:crypto:*`, all four
  `rho:gov:*`, all four `rho:qucalc:*`, `rho:rchain:pos`, `revVault`, `multiSigRevVault`, `deployerId:ops`,
  all four `rho:registry:*`, `rho:rev:address`, `rho:txn`, `sys:authToken:ops`.
- **The ledger's own roster row** (`review-ledger.tsv`, `roster self`): *"the ledger's own machinery —
  and the machinery is gone (2026-09-27) … nothing now re-derives the rosters or refuses a drift … No
  adversarial review of the ledger itself has run, and the reason a reviewer would start with it is that
  it is the denominator every other count on this register is a fraction of."*
- **`effect-scheduler`** appears **zero** times in the entire audit corpus.

## 2. Findings

### A — Multi-signature custody

**A1 · the published bundle exposes the arm that returns the private auth name.**  *(verified by reading)*

The genesis adaptation registers the contract's **own** bundle where the original registered a
**dispatcher**: `casper/src/genesis/standard_deploys.rs::registry_epilogue_adapted` emits
`rs!((9223372036854775807, bundle+{*AuthKey}), *deployerId, *uriOut)` and the same for
`MultiSigRevVault`, replacing the Scala tail `bundle+{*dispatcher}` where `dispatcher` came from
`systemContractManager!("createDispatcher", *AuthKey, …)` — a method-whitelisting shim that does not
exist in this port. The adaptation's own doc comment states the intent and shows the step was taken
knowingly: *"The adaptation registers the contract's own bundle instead, which is exactly what a
consumer destructures"*, and *"The head keeps whatever the contract did before the epilogue —
`MultiSigRevVault`'s `extractState` arm, for instance — because that is contract behaviour rather than
registry plumbing."*

`bundle+` is **write-only** (`rholang/src/runtime.rs::write_bundle` → `Bundle { write_flag: true,
read_flag: false }`), which permits **sending** method-calls — that is how the whole REV surface works:
`system_channel_alias` seeds `rho:rchain:revVault` as `(nonce, write_bundle(rev_vault))` and every caller
then does `revVault!("transfer", …)`. So a write-only bundle is exactly the ability needed to invoke

```rholang
for (ret <- @(*AuthKey, "extractState")) { ret!({"authKeyUnf": *_authKey}) }   // AuthKey.rho:75-77
for (ret <- @(*MultiSigRevVault, "extractState")) { ret!({"multiSigVaultUnf": *_multiSigRevVault}) }  // MultiSigRevVault.rho:272-274
```

by sending `("extractState", myRet)` to a name obtained from `rho:registry:lookup`. Both arms exist
solely to hand back the private unforgeable names. `spec/GENESIS.md` rows 4–5 confirm both contracts are
installed and that consumers reach them by `lookup!` → `@(_, MultiSigRevVault)`.

**The consequence chain, and where it stops.** With `_authKey` and `_multiSigRevVault` public, the shape
`check` compares against is public too: `verify` calls
`@AuthKey!("check", auth, (*_multiSigRevVault, pubKey), *ret)` (`MultiSigRevVault.rho:118`) and `check`
answers `response == bundle0{(*_authKey, shape)}` (`AuthKey.rho:66-72`) — a comparison over values an
attacker who holds both names can construct. `auth` is **caller-supplied** (`transfer` takes `@auth`), so
the check no longer proves possession of any secret, and N-of-M collapses toward 1 for anyone who can
reach a vault's `multiSig` channel.

**What I did not settle.** Whether an attacker can rebuild the *exact* Par `bundle0{(*_authKey, shape)}`
from a name received over a channel — Rholang has no term-level splice, and I could not decide from the
code whether `for (@x <- ret)` followed by `bundle0{(*x, shape)}` yields the identical term. So: **the
leak is established; the forge is a strong inference with one unverified step.** It is the first thing to
arm — a genesis-replay test that looks up the URI and reads the arm.

**A2 · `info.thn(0)`** (`MultiSigRevVault.rho:235`, and the vendored original) — a typo for `info.nth(0)`,
on the branch that records which unsealer confirmed. The contract-and-unsealer authorisation shape is
broken, not merely untested. *(verified)*

### B — The contract layer

**B1 · `rho:io:http` is not an oracle.**  *(verified)*
It never performs a fetch (`record_http`/`http_record` touch only `InMemNativeStore`,
`rholang/src/native_state.rs:951-984`). The stored value is whatever string the first writer supplied —
a write-only self-assertion registry presented as *"the deterministic HTTP-result oracle"*. "Deterministic"
means only replay-safe. First-writer-wins with no authorisation, no overwrite, no contest, no delete, no
expiry, no re-validation; the losing writer is silently ignored (its `record` returns `false`); the winner
is not stored, so it is unidentifiable; and any URL can be squatted before its intended writer.
No shipped contract consumes it today (`system_processes.rs:3155`), so this is a **latent** hazard with a
published API.

**B2 · uncharged quadratic state growth.**  *(verified)*
`record` makes **no** `cost`/`charge` call, and re-encodes the **whole** table on every write
(`encode_http_records` over all records, `native_state.rs:977-982`). One trie leaf grows without bound, at
O(total) per write — a free, permanent, unprunable bloat primitive. Same class as C221 (registry
no-delete), different leaf, no row.

**B3 · channel squatting.**  *(agent-reported)*
A `contract` is a persistent receive. Installing one on a guessable quoted name means every later send to
that name is consumed by the stranger's body and the intended recipient's `for` waits in silence
(law 38). `new`-names are immune; quoted and registry-published names are not.

**B4 · stdout/stderr.**  *(verified in part)*
Consensus-inert (they `println!`/`eprintln!`, never the trie — no fork), but **uncharged** and rendered
with no escaping, so a deploy emits raw attacker-controlled bytes — newlines, ANSI — into every
validator's log and terminal.

### C — Node-local configuration

**C1 · two proposer safety guards that do nothing.**  *(verified)*
`synchrony-constraint-threshold` (`defaults.conf:411`, `0.67`) and `height-constraint-threshold`
(`:415`, `1000`) are documented in `defaults.conf` as *"Node is allowed to propose a block only when …"*
— and a workspace-wide grep finds **every** occurrence in config plumbing (`conf.rs`, `hocon.rs`,
`configuration.rs`, `options.rs`, `config_mapper.rs`). **Nothing outside `node/src/configuration` reads
either.** The Scala oracle enforced both. An operator sets a safety bound and gets silence.

**C2 · two more.**  *(verified)* `fork-choice-stale-threshold` (`:403`) and
`fork-choice-check-if-stale-interval` (`:406`) — parsed, read by nothing.

**C3 · the rule that exists to catch C1/C2 misses exactly them.**  *(verified)*
`node/src/configuration/configuration.rs::check_inert_config` refuses nine parsed-but-unenforced settings,
and its own error text states the principle: *"This port parses these and enforces none of them …
Refused rather than accepted and ignored … a setting that does nothing must not look as though it did."*
The four above are not in its list.

**C4 · `effect-scheduler`.**  *(agent-reported)* Legal values `dfs` (default), `gate`, `relaxed`,
`relaxed-validated`. `dfs`/`gate`/`relaxed-validated` converge on the same post-state hash; **`relaxed`
refuses block-path deploy execution entirely** (`casper/src/runtime_manager.rs:571,701`) — a silent stall,
indistinguishable from a faulting validator. The value is published **nowhere**: not in the state hash,
not in `/api/v1/capabilities`, not in any handshake. A node running it is undetectable from its config.

**C5 · `--epoch-length` divergence is a silent fork.**  *(agent-reported)* The proposer's epoch trigger is
computed from **node-local** `genesis_block_data.epoch_length`
(`node/src/runtime/node_runtime.rs:2096` → `proposer.rs:983-988`), while the actual transition uses the
**on-chain** `PosParams.epoch_length` (`native_state.rs:1426-1453`), and **no validation rule requires a
`CloseBlock` at a boundary** (`interpreter_util.rs:668-678` logs only). A node with a divergent local
value quietly emits boundary blocks with and without the epoch close at the same heights.

### D — Crash and durability

**D1 · an orphan block body is served and never purged.**  *(agent-reported)* The body lands in the
`blockstorage` env (`block_receiver.rs::incoming_blocks` → `block_store.put`) and the DAG metadata in
`dagstorage` (`dag.rs::insert` → `block_metadata_store.add`) — two transactions, two fsyncs. A `SIGKILL`
between them leaves a body the DAG does not hold; `node_running.rs::block_is_known` reads only
`block_store.contains` (verified), so it is **served**; the only purge is
`reset.rs::purge_orphan_bodies`, gated on a reset marker, so an ordinary boot never removes it.

**D2 · equivocation evidence is volatile.**  *(verified)* `casper/src/dag.rs:153-161` — *"In memory, so a
restart forgets"* — and the H-1 gate refuses the offending block **before any write**, so the stored
header is the only copy of the offence (C200 says as much). A restart between observing and proposing
destroys it and the slash never happens. C200 records "refused and never punished"; **no row records that
the evidence's only home is volatile.**

**D3 · disk-full is unhandled.**  *(agent-reported)* No pre-flight; `set_map_size` is fixed at open
(`reporting` at 10 TB); a full disk fails `txn.commit` mid-write and the only handling is a per-block
log-and-skip (`block_receiver.rs:467-476`) — no fail-fast, and ENOSPC is not distinguished from any other
store error.

**D4 · inconsistent durability.**  *(agent-reported)* The `shard-id` and `store-format` markers are written
with plain `std::fs::write` (`node_runtime.rs:2515,2549`) — no tmp+rename, no `sync_all` — while the
**dev faucet ledger** is written properly (tmp + `sync_all` + rename + dir sync, `web_api_impl.rs:101-116`).
The OCapN identity file is likewise unfsynced, and its absent case silently regenerates a different peer
name.

### E — The web layer

**E1 · error bodies carry raw internals.**  *(verified)* `json_result` (`http.rs:249-254`) returns the raw
`BlockApiException` string as the 400 body for **every** `web_api` route; POS, txn and reporting routes
return raw store errors on 500; `exploratory_deploy` returns `format!("{:?}", eval.errors)`; the faucet
path can embed a filesystem path. No row covers error-string leakage.

**E2 · the public OpenAPI discloses the admin schemas.**  *(agent-reported)* `GET /api/v1/openapi.json` is
served from the **public** listener and includes the admin-only `/txn`, `/propose` and the dev `/faucet`
paths. C29 covered staleness, not over-disclosure.

**E3 · the txn coordinator has no amount bound.**  *(agent-reported)* `POST /api/txn` bounds legs (32),
call rate (10/s) and ledger records (10 000) — but `amount: i64` is unbounded per leg, so one admitted
call escrows the validator's whole vault. C121 bounds the count; no row states the amount resid is
unbounded.

**E4 · the acceptance page cites a constant the code deleted.**  *(verified)*
`FAUCET_MAX_DRIPS_PER_ADDRESS` appears four times in `docs/src/spec/testnet-acceptance.md`
(`:520`, `:1360`, `:1413`, `:1829`) and **nowhere in `node/src`**. A whole bow-tie barrier — B12, pair P9
— rests on a symbol that no longer exists.

## 3. What is verified, and what is not

| findings | status |
|---|---|
| A1 (mechanism), A2, B1, B2, C1, C2, C3, D2, E1, E4 | **I read the code** — mechanism established as described |
| A1 (the forge step), B3, B4 (escaping), C4, C5, D1, D3, D4, E2, E3 | **agent-reported** — plausible, cited, not independently checked here |
| everything | **read-class** — no run arms any row |

## 4. What this does not claim

It is a coverage statement, not an exploit: nothing here was executed, and the one finding with real
money behind it — **A1** — has an unverified step that decides whether it is a leak or a theft. It also
does not claim the register is wrong; it claims the register's *denominator* has never been reviewed, and
the contract layer sits outside it.
