# ERTP: tokens are capabilities, and a brand is what they are denominated in

> Requested alongside OCapN by **Dan Connolly** of Agoric; the provenance and the close condition are
> [issue #249](https://github.com/rchain-community/rchain-rust/issues/249). The OCapN half is
> [](ocapn.md); the two are one design, because ERTP's issuer/brand/purse/payment are exactly the
> capabilities that have to cross CapTP.

**Status: built.** The **native issuer ledger** (`PREFIX_ERTP = 0x0A`,
`rholang/src/native_state.rs`), the **object API** over it (`casper/src/genesis/resources/ERTP.rho`,
reached as `rho:rchain:ertp` after the genesis blessing) and **REV as a standard brand with no mint**
are all in the tree, with the close condition's clauses asserted end to end
(`casper/tests/ertp.rs`). This page is the design they were built against; the sections that were
open when it was written now record the decisions and, where something was refused, the reason.

## What ERTP is, and what RChain had

ERTP's five pieces:

| | |
|---|---|
| **brand** | the asset's *identity* — the thing that answers "what is this?" |
| **mint** | the brand's unique creator of new assets |
| **issuer** | the source of truth for how much each purse and payment holds |
| **purse** | an object holding assets of one brand |
| **payment** | an object for *moving* assets; generally not held |
| **amount** | a `(brand, value)` pair — the asset itself lives with the issuer |

RChain already had the shape, half-built: `casper/src/genesis/resources/MakeMint.rho` gives a mint
whose **brand is a private unforgeable name** (`decr`) compared by `bundle0{*decr}` equality, and
whose purses hold `NonNegativeNumber` cells. What it has no notion of is an *issuer* — the balance
lives in the purse, not in a ledger the issuer owns — and therefore no `payment` distinct from a
purse, no `amount`, no `amountMath`, and only one token, REV, whose vault is native.

## Where the parts live

**Balances are consensus state, held natively** (`PREFIX_ERTP`), and the contracts are the API over
it. That follows [`spec/RUST-FIRST.md`](../../../spec/RUST-FIRST.md)'s precedent for the registry,
PoS and the REV vault, and it is where the *spend rule* belongs: a payment consumed exactly once is
an invariant of consensus, not of an interpreter's evaluation order.

The ledger holds two kinds of leaf, told apart by their **key** and, since the totals over the prefix
are *enumerations* rather than running sums, by a tag byte in the **value** as well:

- a **brand** leaf (`0x01`): `blake2b256(brand) → the bytes of the name that may mint it`;
- a **holding** leaf (`0x02`): `blake2b256(len(brand) ‖ brand ‖ holder) → u32_le(brand.len()) ‖ brand ‖
  i64_le(amount) ‖ live`.

**The brand is in the value as well as the key**, which is not redundant: the key is a hash, so a
REV total over `PREFIX_ERTP` could not be taken at all without it — and that total is
`REV_SUPPLY_IS_CONSERVED`'s subject. A leaf read under a key whose brand it does not name is refused
rather than read as data.

### The authority model, which is the part to get right

- **A brand belongs to the name that registered it.** Registration is idempotent for the same
  authority and refused for a different one: a brand whose minter can be replaced is a brand whose
  scarcity is not a fact.
- **Only that name may mint or issue.** `ertp_mint` and `ertp_withdraw` check it.
- **`ertp_deposit` checks nothing.** A purse's own name *is* the capability to deposit into it, so
  requiring more would be requiring a second key to a door that already has one.
- **A holding of one brand is not a holding of another.** An unknown payment is *refused*, not
  treated as zero: the two are different facts, and flattening them is how a foreign brand's payment
  would look acceptable.

### How a purse or payment is identified — settled, and the reason it is not obvious

A Rholang contract **cannot** convert its own `new` name to bytes, and no operation may be added that
does: possessing the bytes *is* possessing the capability, so exposing them would unmake the name's
unforgeability.

So the identities cannot come from Rholang. **The native side mints them**, exactly as the vault does
for its per-call handles (`install_vault_handle`), and keys the ledger by the hash of the minted
name — a hash, never the name, so the ledger's key leaks nothing.

## The API, and the reply shapes

`makeIssuerKit` replies a **3-tuple** `(brand, mint, issuer)` (Agoric's shape). Everything else
replies `(true, value)` / `(false, reason)`, so a caller has one shape to match — except
`getRevIssuer`, whose reply is a **2-tuple** `(brand, issuer)`: the absence of a mint is visible in
the *shape*, before any check.

| Object | Arms |
|---|---|
| issuer | `getBrand`, `getAmountMath`, `makeEmptyPurse`, `getAmountOf` |
| mint | `mintPayment(amount)` |
| amountMath | `make(brand, value)`, `add`, `subtract`, `getValue`, `getBrand`, `isEqual`, `isEmpty` |
| purse | `deposit(payment)`, `withdraw(amount)`, `getCurrentAmount`, `getDepositFacet` |
| payment | `getAllegedBrand` |
| REV purse (as well) | `revFund(funder, amount)`, `revRedeem(amount, to)` |

The ledger underneath is `rho:rchain:ertp:ledger`: `makeKit`, `makePurse`, `balance`, `mint`,
`withdraw`, `deposit`, and the four REV ops below. **A kit's purse has no `revFund` arm at all** —
the REV-only arms arrive as a capability the REV issuer passes, and a kit's purse is given `Nil`, so
the absence is structural rather than a brand check that could be got wrong.

## The amount bound is a fidelity cut, and it is stated

**Amounts are `(brand, value)` with an `Int` value, and the ledger holds a `NonNegI64`.** `amountMath`
therefore *refuses* a value the ledger could not hold rather than letting one exist that cannot be
deposited: `i64::MAX + 1` promotes to a `BigInt` in Rholang arithmetic, and a `BigInt` is refused —
never truncated, never wrapped. This is a deliberate cut against ERTP's arbitrary-precision `NAT`. A
copy of `NonNegativeNumber.rho`'s overflow guard would have been vacuous here (`if (v + x >= v)` never
fires, because this port's arithmetic never wraps), which is why the *type-exact* pattern is what
refuses it.

## REV is an ERTP brand, backed by an escrow

REV's supply is fixed at genesis and it lives in vaults (`PREFIX_VAULT`) — so the ERTP layer presents
REV as a brand whose **issuer has no mint arm**, and a REV amount is a claim on a **reserve** that
vault REV backs:

- the **brand** is `blake2b256("rchain:rev:brand")`, the **authority** `blake2b256("rchain:rev:authority")`
  and the **reserve** the REV address of the unforgeable name `blake2b256("rchain:rev:ertp-reserve")` —
  all derived from named strings, so anyone can recompute them;
- `revFund(funder, amount)` moves REV **from the caller's own vault** (derived from the presented
  `deployerId`, never a supplied address) into the reserve and *then* credits the purse;
- `revRedeem(purse, amount, to)` checks the holding, pays the address out of the reserve and *then*
  debits;
- `revWithdraw(purse, amount)` mints a payment out of a purse — without it a REV purse would be a
  roach motel.

**The authority is a Rust constant that is never replied on any channel**, which is safe for a reason
that is tested rather than asserted: no Rholang term can construct a `GPrivate`, so the authority can
never be *presented*. A `GByteArray` of exactly its bytes is not a name, and the four spellings a
caller might try are each refused. REV's brand registration is idempotent and refuses a *different*
authority, and no term can choose a brand's bytes (`makeKit` mints them from the send's RNG), so this
code is the only registrant.

**The reserve cannot be spent by a deploy.** Its address is derived from a name nobody can construct,
so the vault authority map has no entry for it and never can — `unforgeableAuthKey` requires
*presenting* the name. A `findOrCreate` handle over it can read and cannot move, which is measured.

`REV_SUPPLY_IS_CONSERVED` is the ledger's instrument over all of it, in the direction each half can be
stated: the **vault total is invariant** (funded, redeemed and withdrawn REV is *moved*, never
created — the ERTP layer never makes vault REV), and the **float is backed**: `Σ REV holdings ≤ the
reserve`. The inequality is the safety direction, and it holds under a partial write because REV
enters the reserve before a holding is credited and leaves before one is debited; it is *exact* until
somebody donates to the reserve, and a donation reads as exactly that.

**`revVault` is untouched.** Its reply shapes are pinned by wallet and rgov vectors, `deposit` stays
refused with its message unchanged, and REV's ERTP path is *new ops*, not a branch inside the old
ones: one op name answering with a different handler is the silent downgrade **AUDIT C114** exists to
stop.

## Settled, and one thing not taken

- **A purse is a Rholang contract over a natively-minted name**, and so is a payment. The identity is
  native (the ledger keys by it), the *behaviour* is Rholang — which is what makes a purse auditable
  by an ocapp reader and what makes the deposit protocol a single readable place.
- **`amountMath` is a contract per brand** (`issuer.getAmountMath()`), not an operation on the
  issuer: a caller gets one object to reason with, and the brand check lives inside it.
- **The per-purse vault was rejected**: a REV purse *could* have been a vault-ish object of its own,
  one per purse, and that would have made `deposit` a vault transfer instead of an ERTP credit. It was
  not taken because it multiplies native state by the number of purses, and because "a payment is
  consumed exactly once" is a property of the ledger rather than of a vault — the ERTP ledger already
  has it, one native step wide.

## The capabilities cross CapTP, demonstrated

The point of ERTP here is that its issuer, brand, purse and payment are **capabilities**, and the
request that produced both halves of #249 was for a peer to hold one. That now happens: Agoric's own
`@endo/ocapn` dials a **node**, fetches `rho:rchain:ertp`, calls `makeIssuerKit`, holds the returned
brand/mint/issuer as live remote objects, makes a purse from the issuer, and reads that purse's
balance off the chain — `[ true, 0n ]`. Each call becomes a signed deploy; the kit's members and the
purse cross as **descriptors** (unforgeable names never cross as data). The transcript and the node
configuration are in [`spec/audit/evidence/endo-spike/`](../../../spec/audit/evidence/endo-spike/README.md).
See [](ocapn.md) for what the bridge had to be taught: a returned capability has no source literal,
so the deploy that produces it registers it; and a tuple crosses Syrup as a list.

## One follow-up, and one that was closed

- **[AUDIT C219](../../../spec/AUDIT.md) — a minted channel keeps one continuation**, so
  `install_vault_handle`'s second install replaces the first and a vault handle's `balance` arm has
  never existed. Pre-existing, found here, measured both ways, and *not* worked around: the fix is to
  install the handle as one `arity: 1, remainder: true` continuation dispatching on the method — the
  shape `revVault` and the ERTP ledger already use — which is its own unit because it moves the
  continuation's `body_ref`.
- **`spec/conformance/protocol.tsv`'s `replyCatalog` did not carry the new urn — it does now.** The
  row needed more than an entry: the model could not express the `arity: 1, remainder: true`
  dispatch convention, and had no way to assert a *name* slot. `Rchain/Protocol.lean`'s `ReplyRow`
  gained the dispatch field and `SlotShape.name`, the emitter gained the column, and
  `rholang/src/system_processes.rs` now checks the row against both `Definition.arity` and
  `Definition.remainder` — by which the catalog finally describes `revVault`, `pos` and the ERTP
  ledger as the node installs them.

## Related

- [](ocapn.md) — the other half of issue #249, and the transport these capabilities cross.
- [`spec/RUST-FIRST.md`](../../../spec/RUST-FIRST.md) — the native-state design this ledger follows.
- [`spec/GENESIS.md`](../../../spec/GENESIS.md) — what a new blessed contract has to appear in.
