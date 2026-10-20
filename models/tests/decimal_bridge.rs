//! Issue #380: independent executable falsifiers for Rust Decimal / Lean scaled-int bridge.
//!
//! This checks a bounded but adversarial sample; it is NOT a Lean theorem.
//! The standalone value-algebra and deploy-version admission proofs remain separate.
use std::cmp::Ordering;

use num_bigint::BigInt;
use proptest::prelude::*;
use rchain_models::decimal::{Decimal, DecimalError, MAX_SCALE};

fn unit() -> BigInt {
    BigInt::from(10u8).pow(MAX_SCALE)
}

fn decimal(coefficient: BigInt, scale: u32) -> Decimal {
    Decimal::new(coefficient, scale).expect("test fixture must normalize inside scale limit")
}

fn examples() -> Vec<Decimal> {
    let base = BigInt::from(10u8).pow(70);
    let mut result = Vec::new();
    for coefficient in [
        BigInt::from(-1_001_i32),
        BigInt::from(-1_000_i32),
        BigInt::from(-15_i32),
        BigInt::from(-1_i32),
        BigInt::from(0_i32),
        BigInt::from(1_i32),
        BigInt::from(15_i32),
        BigInt::from(1_000_i32),
        BigInt::from(1_001_i32),
        -&base - 1,
        base + 1,
    ] {
        for scale in [0, 1, 2, 3, 17, 37, 38] {
            let d = decimal(coefficient.clone(), scale);
            if !result.contains(&d) {
                result.push(d);
            }
        }
    }
    result
}

#[test]
fn scaled_representation_is_injective_and_preserves_numeric_order() {
    let samples = examples();
    assert!(samples.len() > 50);
    for a in &samples {
        // This map is the payload chosen for Lean's edecimal constructor.
        let a_value = a.scaled_value();
        assert_eq!(
            a_value,
            a.coefficient() * BigInt::from(10u8).pow(MAX_SCALE - a.scale())
        );
        for b in &samples {
            let score_order = a_value.cmp(&b.scaled_value());
            assert_eq!(
                a.cmp_numeric(b),
                score_order,
                "numeric order vs scaled order"
            );
            assert_eq!(
                a == b,
                a_value == b.scaled_value(),
                "two canonical Rust values mapped to one Lean payload"
            );
            assert_eq!(score_order == Ordering::Equal, a == b);
        }
    }
}

#[test]
fn canonical_binary_round_trip_retains_the_lean_payload() {
    for value in examples() {
        let wire_coefficient = value.coefficient().to_signed_bytes_be();
        let decoded = Decimal::from_wire(&wire_coefficient, value.scale())
            .expect("a canonical coefficient must decode");
        assert_eq!(decoded, value);
        assert_eq!(decoded.scaled_value(), value.scaled_value());

        let mut with_extra_sign_extension = if value.coefficient() < &BigInt::from(0) {
            vec![0xff]
        } else {
            vec![0x00]
        };
        with_extra_sign_extension.extend_from_slice(&wire_coefficient);
        assert_eq!(
            Decimal::from_wire(&with_extra_sign_extension, value.scale()),
            Err(DecimalError::CoefficientNotMinimal),
            "non-minimal signed coefficient is an alternative block-hash encoding"
        );
    }
}

#[test]
fn exact_operations_match_independent_scaled_integer_arithmetic() {
    let samples = examples();
    let factor = unit();
    for a in &samples {
        for b in &samples {
            let a_scaled = a.scaled_value();
            let b_scaled = b.scaled_value();
            let sum = a
                .add(b)
                .expect("addition of bounded-scale decimals is representable");
            let difference = a.sub(b).expect("subtraction is representable");
            assert_eq!(sum.scaled_value(), &a_scaled + &b_scaled);
            assert_eq!(difference.scaled_value(), &a_scaled - &b_scaled);

            // The product is representable at scale <= 38 exactly iff it has
            // no sub-atomic remainder in units of 10^-38.
            let product = &a_scaled * &b_scaled;
            let representable = &product % &factor == BigInt::from(0);
            match a.mul(b) {
                Ok(value) => {
                    assert!(representable, "inexact product silently admitted");
                    assert_eq!(value.scaled_value() * &factor, product);
                }
                Err(DecimalError::ScaleOutOfRange { .. }) => {
                    assert!(!representable, "exact bounded-scale product refused");
                }
                Err(error) => panic!("unexpected multiplication error: {error}"),
            }
        }
    }
}

#[test]
fn division_returns_exact_scaled_value_or_a_named_refusal() {
    let samples = examples();
    let unit = unit();
    // Cross-scale divisors with factors 2/5, repeating denominators, and
    // boundary values; including negative signs and the canonical zero.
    let divisors = [
        decimal(BigInt::from(0), 0),
        decimal(BigInt::from(1), 0),
        decimal(BigInt::from(-1), 0),
        decimal(BigInt::from(2), 0),
        decimal(BigInt::from(5), 0),
        decimal(BigInt::from(3), 0),
        decimal(BigInt::from(25), 2),
        decimal(BigInt::from(-7), 3),
        decimal(BigInt::from(1), 38),
    ];
    for a in &samples {
        for b in &divisors {
            if b.is_zero() {
                assert_eq!(a.div(b), Err(DecimalError::DivisionByZero));
                continue;
            }
            let dividend = a.scaled_value() * &unit;
            let divisor = b.scaled_value();
            let exact = &dividend % &divisor == BigInt::from(0);
            match a.div(b) {
                Ok(quotient) => {
                    assert!(exact, "non-terminating quotient was rounded");
                    assert_eq!(quotient.scaled_value() * divisor, dividend);
                }
                Err(DecimalError::InexactDivision) => {
                    assert!(!exact, "representable quotient was refused");
                }
                Err(error) => panic!("unexpected division error: {error}"),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn randomized_canonical_bridge_is_consistent(
        raw_a in any::<i64>(),
        raw_b in any::<i64>(),
        scale_a in 0u32..=MAX_SCALE,
        scale_b in 0u32..=MAX_SCALE
    ) {
        let a = decimal(BigInt::from(raw_a), scale_a);
        let b = decimal(BigInt::from(raw_b), scale_b);
        prop_assert_eq!(a.cmp_numeric(&b), a.scaled_value().cmp(&b.scaled_value()));
        prop_assert_eq!(a == b, a.scaled_value() == b.scaled_value());
        prop_assert_eq!(
            a.add(&b).unwrap().scaled_value(),
            a.scaled_value() + b.scaled_value()
        );
        prop_assert_eq!(
            Decimal::from_wire(&a.coefficient().to_signed_bytes_be(), a.scale()).unwrap(),
            a
        );
    }
}
