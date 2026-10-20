# Decimal bridge acceptance — issue #380

Source: `models/tests/decimal_bridge.rs` (Rust integration test, **not** a Lean proof).

## What the suite can falsify

The `#390` Lean model treats a Decimal as `edecimal : Int → Expr`; its integer
payload is `Decimal::scaled_value()`, measured in units of `10^-38`.
That bridge was explicitly outside the Lean theorem's scope.

The new test compares **independently calculated scaled integer quantities** with
the actual Rust Decimal type. It checks:

1. **Canonical injectivity (bounded adversarial corpus).** Two `Decimal` objects
   have equal scaled values iff they are the same canonical object, including
   negatives, zero, extremely large coefficients and scales at 0 and 38.
2. **Numeric order.** `Decimal::cmp_numeric` agrees with the exact integer order
   of `scaled_value` for all pairs in the corpus.
3. **Wire canonicality.** Encoding the canonical signed coefficient into minimal
   two's-complement bytes round-trips to the same Decimal and Lean payload.
   Additional sign-extension bytes are rejected, not interpreted as aliases.
4. **Arithmetic.** Exact addition and subtraction are checked against independent
   scaled-integer sum/difference. Multiplication is required to return the exact
   product when representable at scale ≤38 and to refuse unrepresentable products.
5. **Division.** A representable quotient must satisfy the scaled-integer cross
   product exactly; a non-representable quotient must fail with the named
   `InexactDivision` error, without rounding. Zero divisor must be refused.
6. **Seeded property cases.** Proptest generates signed `i64` coefficients
   and scales 0..=38 to challenge canonical order, equality, addition and wire
   round-trip beyond the hand-selected corpus.

## Reproduction

```bash
cargo test --locked -p rchain-models --test decimal_bridge -- --nocapture
cargo fmt --all --check
```

## What passing does *not* mean

The corpus and property checks are **finite executable witnesses**, not a formal
proof over arbitrarily large BigInt coefficients or every possible pair and scale.
The standalone `Rchain/Decimal.lean` proof, the map from Rust canonicalization
to `edecimal`, full reducer dispatch, gas cost and protocol-version admission
remain separate obligations. Specifically, these tests **do not** enable Decimal
on an existing live genesis and do not certify mixed-release consensus safety.

No live chain was scanned here; the new-genesis and uniform-validator release
condition in #391 and the unmerged #373 dependency remain in force. CI must
attribute results to its exact source SHA.
