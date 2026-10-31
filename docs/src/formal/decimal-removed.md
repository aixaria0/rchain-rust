# The decimal type, withdrawn (issue #527)

Rholang has no built-in fractional type. It had one — an exact decimal, `GDecimal`, written as `1.5d` —
from 19 October 2026 until 31 October 2026, when it was **removed before mainnet** by issue
[#527](https://github.com/rhovision-org/rholo/issues/527). This page is the record: what was built, why
it went, and what a future attempt would have to do differently. It exists because the request will be
made again, and the tree should answer it before anyone re-derives it.

## The decision, and why

The type was a **38-digit fixed-point decimal**: `coefficient × 10^-scale`, canonical form, scale ≤ 38.
Three facts decided it:

- **It is isomorphic to a scale-38 integer.** Every value it can hold is representable exactly as an
  integer in a chosen base unit, so it adds no representational power. Every money path in this node is
  already integer base units or basis points — ERTP/REV amounts are `NonNegI64`, the PoS reward,
  participation, executor-share and pro-rata rules are integer basis points with floored dust, the
  development tax is integer drops, and the 2/3 finality rule is exact `i128` *because* `f64` rounds
  past 2^53.
- **It is not a rational.** `1d / 3d` was an `InexactDivision` error, so the one job a fractional
  primitive is actually needed for — a ratio — it could not do. Fixed-precision prices (`0.5`,
  `0.00012345`) it could represent, and those are exactly what integer base units already carry.
- **It was a one-way consensus commitment.** Wire tag 35 sat in the `Par` oneof that the state and
  channel-hash codec decodes, sort rank 14 ordered every map and set before hashing, and an *included*
  deploy's source is re-parsed at replay. Once mainnet state held a decimal, tag 35 would have to decode
  for ever. Adding a value type later costs a release but not a chain; removing one after a contract
  used it costs a new genesis.

No contract in the tree used it — not the genesis contracts, not the system processes, not `qucalc`.
The 2026-10-19 review had already recorded the motivation as authoring convenience rather than
correctness. Four independent reviews (mechanism, protocol, use-case, risk) reached **remove**; an
adversary could not overturn it, and the counterargument it preserved is stated below.

## What was removed

| surface | what went |
|---|---|
| Wire | `GDecimal g_decimal = 35` → `reserved 35; reserved "g_decimal";`. A sender now gets the existing `Malformed("expr_instance")` refusal |
| AST | `Expr::GDecimal`; `models/src/decimal.rs` left the module tree |
| Literal | the `d` suffix in the lexer and parser; `GroundDecimal` in the parse tree; the normalizer's `parse_decimal_literal` |
| Reducer | the promotion path for `+ - * /`, the decimal comparison arm, the `toString`/`toInt`/`toBigInt` arms, the interpolation arm |
| Ordering | sort rank 14 (`DECIMAL`) — retired and **never reused**; ranks are non-contiguous so nothing renumbers |
| Cost | the eight decimal cost functions and the register's decimal rows |
| Model | `Expr.edecimal` and 42 occurrences across 13 modules; the 42-case `decimal` corpus; the eight decimal rows of the `sort` corpus (the layer went from fifty to forty-two cases) |
| Legal | Law 87a–e, retired **in place** (same numbers; laws 88–91 do not renumber) |
| Bridges | the OCapN/Syrup refusal arm (there is nothing to refuse now); the JSON view's shared `GBigInt` gap |

## What was archived, and where a future attempt should look

Three pieces are kept as **commented code**, not deleted, because they are the semantics a new attempt
would otherwise re-derive:

- **`models/src/decimal.rs`** — the whole implementation, including its eleven tests (archived, not
  compiled, and *not* coverage), behind a dated header.
- **`spec/Rchain/Decimal.lean`** — the 902-line Law 87 model: canonicalisation, the scaled-value
  embedding, exact arithmetic and the division criterion. It stays on disk and imported because the
  conformance gate requires every model module in the root import list; an all-comment module is valid
  Lean.
- **`rholang/src/accounting.rs`** — the eight decimal cost formulas, with the reasoning that priced
  each one.

Every other site carries a short prose tombstone: the AST variant position, the wire arms,
`sorter.rs`'s rank, the parser token, normalizer, printer, matcher, substitute, and the two bridges.
Those arms are one-liners whose meaning is already in the two archived files.

The register keeps the decision, not just the tombstone: **Law 87a–e are `retired`**, each with a dated
note naming the code that was read and why the port has no rule of that shape. The Scala-deviation
register's decimal row is retained unedited as the record of the 2026-10-19 addition, with a dated
correction after the table.

## Consensus and replay

A chain that was decimal-free replays unchanged. The block hash is Blake2b over the
`BlockMessageProto` bytes; deleting one `oneof` arm and one message type cannot alter the encoding of a
term that does not use them, and no other field's number moves. The canonical order is the explicit
`node_score` tree, whose non-decimal tags are unchanged. No decimal cost function runs for a deploy
without a decimal, and the parser's handling of every other token is unchanged.

What changed is **refusal**: a state `Par` with tag 35 no longer decodes, and a `d`-suffixed literal no
longer parses. So a chain that ever carried either cannot be replayed, and the removal is licensed by
one of two things: a **fresh genesis** (this change is bundled with the pre-mainnet restart anyway), or
**preflight evidence** that no such value exists — a complete state-root export with zero tag-35
expressions and a complete block export with zero decimal source literals, with scanned counts equal.

## What a future attempt must do differently

1. **Model and corpus first.** Law 87 and its 42-case corpus were added *after* the type, which is how
   the type shipped ungated. Write the algebra and the cases before the wire.
2. **A versioned activation**, not an ungated addition. The block-version schedule exists
   (`models/src/block_version.rs`) and its height gate is still unwired; a new value type should be the
   change that wires it.
3. **A wire-tag decision.** Tag 35 is `reserved` and reusing it needs the same gate. A distinct tag,
   a rational `{num, den}`, or a metered `rho:lang` process over integer records are all viable.
4. **Prefer a rational, or nothing.** If the case is a *ratio*, a fixed-point decimal reproduces the
   limitation that made this one useless. A blessed `{coefficient, scale}` contract at genesis is the
   worst option: it copies the type's arithmetic into userland with worse metering and no canonical
   enforcement — any contract can mint a non-canonical record.
5. **State the use case in the issue's own terms**: a contract expected on mainnet where (a) integers
   in base units cannot represent the quantity and (b) the value crosses an independently authored
   contract boundary, so a record's canonical form cannot be trusted. Without both, the answer stands.

## What this does not claim

That a decimal type is never needed — it claims that none was needed *here, yet*, and that a
fixed-point decimal was the wrong shape for the need that would justify one. Nor does the removal fix
the two traps that were blamed on it and were inherited from `Int`/`BigInt`: **`1 == 1n` is true while
`1` and `1n` are distinct map keys and patterns**, and **canonical ordering is class-major** — a new
numeric class sorts after every integer whatever its value. Both survive this change and are documented
in [the rholang reference](../rholang/reference.md).

## How to verify this removal

```text
cargo test -p rchain-models --test expr_refusal          # tag 35 refused as `expr_instance`
cargo test -p rchain-models                              # the archived tests do not run
cargo test -p rchain-rholang                             # `2d` no longer parses
cargo test -p rchain-rholang --test lean_sort_corpus     # 42 rows, no decimal class
tools/check-lean-conformance.sh                          # the register and corpora agree
tools/emit-lean-laws.sh --check                          # the register is consistent
tools/check-cost-register.py                             # no decimal rows remain
```
