# ERTP: tokens are capabilities, and a brand is what they are denominated in

> Requested alongside OCapN by **Dan Connolly** of Agoric; the provenance and the close condition are
> [issue #249](https://github.com/rchain-community/rchain-rust/issues/249). The OCapN half is
> [](ocapn.md); the two are one design, because ERTP's issuer/brand/purse/payment are exactly the
> capabilities that have to cross CapTP.

**Status.** The **native issuer ledger** is built (`PREFIX_ERTP = 0x0A`,
`rholang/src/native_state.rs`). The Rholang API over it, the genesis blessing, and REV as an ERTP
brand are **not** — this page records the design they will be built against, and says plainly which
parts are settled and which are still open.

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

The ledger holds two kinds of leaf, told apart by their key:

- a **brand** leaf: `brand → the bytes of the name that may mint it`;
- a **holding** leaf: `(brand, holder) → amount + live`.

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

## What is still open

- **Whether a purse is a native object or a contract over a native name.** The vault's handle is a
  native continuation installed on a minted name; a purse *could* be the same. The alternative is to
  hand the minted name to a Rholang `purse` contract that wraps it, keeping the object model in
  Rholang. The close condition asks for `makeIssuerKit` returning `(brand, mint, issuer)` and purses
  with `deposit`/`withdraw`/`getCurrentAmount` — an API shape, which either route can present. **Not
  decided here**; the difference is where the object's *behaviour* lives, not where its balance does.
- **amountMath.** Whether it is a contract per brand, or an operation the issuer exposes. It has to
  exist as *something*, because `amount` is a pair and comparing or adding amounts is how a caller
  reasons about holdings without asking the issuer every time.
- **REV as a brand with no mint arm.** REV's supply is fixed at genesis and its vault stays where it
  is (`PREFIX_VAULT`); the ERTP layer presents REV as a brand whose issuer cannot mint. Whether that
  issuer is a native process or a wrapper contract over `rho:rchain:revVault` is open, and the
  constraint is that `revVault` and `makeMint` stay byte-for-byte — their reply shapes are pinned by
  wallet and rgov vectors and `spec/GENESIS.md`.

## Related

- [](ocapn.md) — the other half of issue #249, and the transport these capabilities cross.
- [`spec/RUST-FIRST.md`](../../../spec/RUST-FIRST.md) — the native-state design this ledger follows.
- [`spec/GENESIS.md`](../../../spec/GENESIS.md) — what a new blessed contract has to appear in.
