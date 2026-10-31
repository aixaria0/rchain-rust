// **The exact decimal value type, archived (issue #527).**
//
// This file was `models::decimal` — a 38-digit fixed-point decimal (`coefficient × 10^-scale`,
// canonical form, `MAX_SCALE = 38`). It was added 2026-10-19 (issue #380 option B, merged in
// #389/#390) and removed before mainnet on 2026-10-31 by issue #527, option R.
//
// **Why it went.** A fixed-point decimal is isomorphic to a scale-38 integer, so it adds no
// representational power over integers in a chosen base unit; every money path in this node is
// already integer base units or basis points; it cannot carry a non-terminating ratio — `1d / 3d`
// is `InexactDivision` — which is the one job a fractional primitive would be needed for; and its
// wire tag (35) sits in the state and channel-hash codec, so keeping the type was the one-way door.
// The full decision, with the evidence, is `docs/src/formal/decimal-removed.md`.
//
// **What a future attempt must do differently.** Model and corpus first; a versioned activation (a
// hard fork) rather than an ungated addition; and a wire-tag decision — tag 35 is `reserved`, and
// reusing it needs the same gate. If a use case is ever named, prefer a rational (`{num, den}`)
// contract or a metered `rho:lang` process over integer records; a blessed `{coefficient, scale}`
// contract would reproduce the exact limitation that made this type useless.
//
// **This code is not compiled and its tests are not coverage.** `models::decimal` is no longer a
// module (`models/src/lib.rs`), so nothing below is in the tree's test count or its gates.
//
// --- archived 2026-10-31 ---
//
// //! The exact decimal value type (issue #380, option B).
// //!
// //! A [`Decimal`] is `coefficient × 10^-scale` with a signed, arbitrary-precision coefficient and a
// //! non-negative scale. There is no NaN, no infinity and no negative zero.
// //!
// //! **Canonical form.** Factors of ten are stripped from the coefficient while the scale is
// //! positive, and every zero is `(0, 0)`. Two numerically equal decimals therefore have exactly one
// //! representation, which is what lets the derived `Eq`/`Ord` on `Expr` behave as numeric equality
// //! for canonical values — the property `par_set`/`par_map` rely on for deduplication.
// //!
// //! **Three ingress policies, deliberately different.** The wire refuses any noncanonical raw form
// //! ([`Decimal::from_wire`]); a source literal and an arithmetic result canonicalize first and then
// //! check the scale bound ([`Decimal::new`]). The wire stores both fields, so a noncanonical pair is
// //! an alternative encoding of one value and must be refused; a literal or a result is a value
// //! request, so its canonical form is what has to fit.
//
// use std::cmp::Ordering;
// use std::fmt;
//
// use num_bigint::BigInt;
// use num_traits::ToPrimitive;
// use serde::{Deserialize, Serialize};
//
// /// The largest number of fractional decimal digits a value may have. This is a protocol constant:
// /// changing it changes the canonical form, the sort score and the Lean model's scaled
// /// representation (`spec/Rchain/Par.lean`).
// pub const MAX_SCALE: u32 = 38;
//
// /// A decimal value error.
// #[derive(Clone, Debug, PartialEq, Eq)]
// pub enum DecimalError {
//     /// The scale is outside `0..=MAX_SCALE`.
//     ScaleOutOfRange { scale: u32 },
//     /// The wire coefficient field was empty (`BigInt::from_signed_bytes_be` would read it as zero).
//     CoefficientEmpty,
//     /// The wire coefficient was not the minimal signed two's-complement encoding.
//     CoefficientNotMinimal,
//     /// A zero coefficient carried a nonzero scale; canonical zero is `(0, 0)`.
//     ZeroWithScale { scale: u32 },
//     /// The coefficient had a factor of ten at a positive scale; canonical form strips it.
//     TrailingZero { scale: u32 },
//     /// Division by zero.
//     DivisionByZero,
//     /// Division did not terminate within [`MAX_SCALE`] digits.
//     InexactDivision,
//     /// The sum of two scales exceeded `u32` (unreachable with `MAX_SCALE = 38`, refused rather
//     /// than wrapped).
//     ScaleOverflow { scale: u32 },
// }
//
// impl fmt::Display for DecimalError {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             DecimalError::ScaleOutOfRange { scale } => {
//                 write!(f, "decimal scale {scale} is out of range (0..={MAX_SCALE})")
//             }
//             DecimalError::CoefficientEmpty => write!(f, "decimal coefficient bytes are empty"),
//             DecimalError::CoefficientNotMinimal => {
//                 write!(f, "decimal coefficient is not minimally encoded")
//             }
//             DecimalError::ZeroWithScale { scale } => {
//                 write!(f, "decimal zero must have scale 0, got {scale}")
//             }
//             DecimalError::TrailingZero { scale } => {
//                 write!(
//                     f,
//                     "decimal coefficient has a trailing zero at scale {scale}"
//                 )
//             }
//             DecimalError::DivisionByZero => write!(f, "decimal division by zero"),
//             DecimalError::InexactDivision => {
//                 write!(f, "decimal division is not exact within scale {MAX_SCALE}")
//             }
//             DecimalError::ScaleOverflow { scale } => {
//                 write!(f, "decimal scale overflow at {scale}")
//             }
//         }
//     }
// }
//
// impl std::error::Error for DecimalError {}
//
// /// A canonical exact decimal: `coefficient × 10^-scale`.
// #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// #[serde(try_from = "DecimalRepr")]
// pub struct Decimal {
//     coefficient: BigInt,
//     scale: u32,
// }
//
// /// The serialized shape. Deserialization is routed through [`Decimal::from_parts`] so a
// /// noncanonical pair cannot enter the AST from JSON either.
// #[derive(Clone, Debug, Serialize, Deserialize)]
// struct DecimalRepr {
//     coefficient: BigInt,
//     scale: u32,
// }
//
// impl TryFrom<DecimalRepr> for Decimal {
//     type Error = DecimalError;
//
//     fn try_from(repr: DecimalRepr) -> Result<Self, Self::Error> {
//         Decimal::from_parts(repr.coefficient, repr.scale)
//     }
// }
//
// impl Decimal {
//     /// Build a canonical value from a coefficient and a scale. Canonicalization runs first, so an
//     /// arithmetic result whose raw scale is large but whose value fits is accepted (for example
//     /// `(10^20, 20) * (10^20, 20)` canonicalizes to `(1, 0)`).
//     pub fn new(mut coefficient: BigInt, mut scale: u32) -> Result<Self, DecimalError> {
//         let zero = BigInt::from(0);
//         if coefficient == zero {
//             return Ok(Decimal {
//                 coefficient: zero,
//                 scale: 0,
//             });
//         }
//         let ten = BigInt::from(10);
//         while scale > 0 && &coefficient % &ten == zero {
//             coefficient /= &ten;
//             scale -= 1;
//         }
//         if scale > MAX_SCALE {
//             return Err(DecimalError::ScaleOutOfRange { scale });
//         }
//         Ok(Decimal { coefficient, scale })
//     }
//
//     /// The strict constructor: the pair must already be canonical. Used by the wire and by serde.
//     pub fn from_parts(coefficient: BigInt, scale: u32) -> Result<Self, DecimalError> {
//         if scale > MAX_SCALE {
//             return Err(DecimalError::ScaleOutOfRange { scale });
//         }
//         let canonical = Self::new(coefficient.clone(), scale)?;
//         if canonical.coefficient != coefficient || canonical.scale != scale {
//             return Err(if coefficient == BigInt::from(0) {
//                 DecimalError::ZeroWithScale { scale }
//             } else {
//                 DecimalError::TrailingZero { scale }
//             });
//         }
//         Ok(canonical)
//     }
//
//     /// Parse a source literal's parts. Source canonicalizes first, then bounds — the literal's
//     /// canonical value is what has to fit, exactly like an arithmetic result.
//     pub fn from_literal_parts(coefficient: BigInt, scale: u32) -> Result<Self, DecimalError> {
//         Self::new(coefficient, scale)
//     }
//
//     /// Decode the wire form: a minimal signed two's-complement big-endian coefficient and a scale.
//     pub fn from_wire(bytes: &[u8], scale: u32) -> Result<Self, DecimalError> {
//         if scale > MAX_SCALE {
//             return Err(DecimalError::ScaleOutOfRange { scale });
//         }
//         if bytes.is_empty() {
//             return Err(DecimalError::CoefficientEmpty);
//         }
//         let coefficient = BigInt::from_signed_bytes_be(bytes);
//         if coefficient.to_signed_bytes_be() != bytes {
//             return Err(DecimalError::CoefficientNotMinimal);
//         }
//         Self::from_parts(coefficient, scale)
//     }
//
//     /// The signed coefficient.
//     pub fn coefficient(&self) -> &BigInt {
//         &self.coefficient
//     }
//
//     /// The number of fractional decimal digits.
//     pub fn scale(&self) -> u32 {
//         self.scale
//     }
//
//     /// Whether the value is zero. Canonical zero is `(0, 0)`.
//     pub fn is_zero(&self) -> bool {
//         self.coefficient == BigInt::from(0)
//     }
//
//     /// The value in units of `10^-MAX_SCALE`: `coefficient × 10^(MAX_SCALE - scale)`. This is the
//     /// sorter's score, and the Lean model's `edecimal` payload, so the two orders agree.
//     pub fn scaled_value(&self) -> BigInt {
//         &self.coefficient * pow10(MAX_SCALE.saturating_sub(self.scale))
//     }
//
//     /// Compare two decimals by exact numeric value.
//     pub fn cmp_numeric(&self, other: &Self) -> Ordering {
//         match self.scale.cmp(&other.scale) {
//             Ordering::Equal => self.coefficient.cmp(&other.coefficient),
//             Ordering::Less => {
//                 (&self.coefficient * pow10(other.scale - self.scale)).cmp(&other.coefficient)
//             }
//             Ordering::Greater => self
//                 .coefficient
//                 .cmp(&(&other.coefficient * pow10(self.scale - other.scale))),
//         }
//     }
//
//     /// Exact addition.
//     pub fn add(&self, other: &Self) -> Result<Self, DecimalError> {
//         let scale = self.scale.max(other.scale);
//         let left = &self.coefficient * pow10(scale - self.scale);
//         let right = &other.coefficient * pow10(scale - other.scale);
//         Self::new(left + right, scale)
//     }
//
//     /// Exact subtraction.
//     pub fn sub(&self, other: &Self) -> Result<Self, DecimalError> {
//         let scale = self.scale.max(other.scale);
//         let left = &self.coefficient * pow10(scale - self.scale);
//         let right = &other.coefficient * pow10(scale - other.scale);
//         Self::new(left - right, scale)
//     }
//
//     /// Exact multiplication.
//     pub fn mul(&self, other: &Self) -> Result<Self, DecimalError> {
//         let scale = self
//             .scale
//             .checked_add(other.scale)
//             .ok_or(DecimalError::ScaleOverflow { scale: u32::MAX })?;
//         Self::new(&self.coefficient * &other.coefficient, scale)
//     }
//
//     /// Negation (canonical form is preserved: negation cannot add a trailing zero).
//     pub fn neg(&self) -> Self {
//         Decimal {
//             coefficient: -&self.coefficient,
//             scale: self.scale,
//         }
//     }
//
//     /// Exact division, or an explicit error. The quotient is built one decimal digit at a time; a
//     /// nonzero remainder after [`MAX_SCALE`] digits means no representable result exists.
//     pub fn div(&self, other: &Self) -> Result<Self, DecimalError> {
//         if other.is_zero() {
//             return Err(DecimalError::DivisionByZero);
//         }
//         let zero = BigInt::from(0);
//         let a = if self.coefficient < zero {
//             -&self.coefficient
//         } else {
//             self.coefficient.clone()
//         };
//         let b = if other.coefficient < zero {
//             -&other.coefficient
//         } else {
//             other.coefficient.clone()
//         };
//         let numerator = a * pow10(other.scale);
//         let denominator = b * pow10(self.scale);
//         let negative = (self.coefficient < zero) != (other.coefficient < zero);
//
//         let mut quotient = &numerator / &denominator;
//         let mut remainder = &numerator % &denominator;
//         if remainder != zero {
//             for digit_count in 1..=MAX_SCALE {
//                 remainder *= &BigInt::from(10);
//                 let digit = &remainder / &denominator;
//                 remainder %= &denominator;
//                 quotient = quotient * &BigInt::from(10) + digit;
//                 if remainder == zero {
//                     let signed = if negative { -quotient } else { quotient };
//                     return Self::new(signed, digit_count);
//                 }
//             }
//             return Err(DecimalError::InexactDivision);
//         }
//         let signed = if negative { -quotient } else { quotient };
//         Self::new(signed, 0)
//     }
//
//     /// The i64 value, only when the value is an integer in range.
//     pub fn to_i64(&self) -> Option<i64> {
//         if self.scale == 0 {
//             self.coefficient.to_i64()
//         } else {
//             None
//         }
//     }
//
//     /// The exact BigInt value, only when the value is an integer.
//     pub fn to_bigint_exact(&self) -> Option<BigInt> {
//         if self.scale == 0 {
//             Some(self.coefficient.clone())
//         } else {
//             None
//         }
//     }
//
//     /// The plain decimal spelling, without the Rholang `d` suffix: `1.5`, `0`, `-0.05`.
//     pub fn to_plain_string(&self) -> String {
//         let negative = self.coefficient < BigInt::from(0);
//         let magnitude = if negative {
//             -&self.coefficient
//         } else {
//             self.coefficient.clone()
//         };
//         let digits = magnitude.to_string();
//         let body = if self.scale == 0 {
//             digits
//         } else {
//             let scale = self.scale as usize;
//             let mut padded = digits;
//             while padded.len() <= scale {
//                 padded.insert(0, '0');
//             }
//             let split = padded.len() - scale;
//             format!("{}.{}", &padded[..split], &padded[split..])
//         };
//         if negative {
//             format!("-{body}")
//         } else {
//             body
//         }
//     }
//
//     /// The canonical Rholang literal: the plain spelling plus the `d` suffix (`1.5d`, `0d`).
//     pub fn to_literal_string(&self) -> String {
//         format!("{}d", self.to_plain_string())
//     }
// }
//
// impl fmt::Display for Decimal {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.write_str(&self.to_plain_string())
//     }
// }
//
// impl PartialOrd for Decimal {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }
//
// impl Ord for Decimal {
//     fn cmp(&self, other: &Self) -> Ordering {
//         self.cmp_numeric(other)
//     }
// }
//
// fn pow10(n: u32) -> BigInt {
//     let mut value = BigInt::from(1);
//     for _ in 0..n {
//         value *= 10u8;
//     }
//     value
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     fn d(coefficient: i64, scale: u32) -> Decimal {
//         Decimal::new(BigInt::from(coefficient), scale).expect("canonical value")
//     }
//
//     #[test]
//     fn canonicalisation_strips_trailing_zeros_and_normalises_zero() {
//         assert_eq!(d(110, 2), d(11, 1));
//         assert_eq!(d(1000, 3), d(1, 0));
//         assert_eq!(d(0, 5), d(0, 0));
//         assert_eq!(d(5, 0), d(5, 0));
//     }
//
//     #[test]
//     fn the_scale_bound_is_inclusive() {
//         assert!(Decimal::new(BigInt::from(1), MAX_SCALE).is_ok());
//         assert_eq!(
//             Decimal::new(BigInt::from(1), MAX_SCALE + 1),
//             Err(DecimalError::ScaleOutOfRange {
//                 scale: MAX_SCALE + 1
//             })
//         );
//     }
//
//     #[test]
//     fn strict_construction_refuses_a_noncanonical_pair() {
//         assert_eq!(
//             Decimal::from_parts(BigInt::from(150), 2),
//             Err(DecimalError::TrailingZero { scale: 2 })
//         );
//         assert_eq!(
//             Decimal::from_parts(BigInt::from(0), 3),
//             Err(DecimalError::ZeroWithScale { scale: 3 })
//         );
//         assert!(Decimal::from_parts(BigInt::from(15), 1).is_ok());
//     }
//
//     #[test]
//     fn the_wire_form_is_minimal_and_canonical() {
//         assert_eq!(
//             Decimal::from_wire(&[], 0),
//             Err(DecimalError::CoefficientEmpty)
//         );
//         assert_eq!(
//             Decimal::from_wire(&[0x00, 0x0f], 1),
//             Err(DecimalError::CoefficientNotMinimal)
//         );
//         assert_eq!(
//             Decimal::from_wire(&[0xff, 0xf1], 1),
//             Err(DecimalError::CoefficientNotMinimal)
//         );
//         assert_eq!(
//             Decimal::from_wire(&[0x00, 0x96], 2),
//             Err(DecimalError::TrailingZero { scale: 2 })
//         );
//         assert_eq!(Decimal::from_wire(&[0x00], 0), Ok(d(0, 0)));
//         assert_eq!(Decimal::from_wire(&[0x0f], 1), Ok(d(15, 1)));
//         assert_eq!(Decimal::from_wire(&[0xf1], 1), Ok(d(-15, 1)));
//         assert_eq!(Decimal::from_wire(&[0x80], 0), Ok(d(-128, 0)));
//     }
//
//     #[test]
//     fn comparison_is_numeric_across_scales() {
//         assert_eq!(d(15, 1).cmp_numeric(&d(2, 0)), Ordering::Less);
//         assert_eq!(d(2, 0).cmp_numeric(&d(15, 1)), Ordering::Greater);
//         assert_eq!(d(15, 1).cmp_numeric(&d(150, 2)), Ordering::Equal);
//         assert_eq!(d(0, 0).cmp_numeric(&d(1, MAX_SCALE)), Ordering::Less);
//         assert_eq!(d(-15, 1).cmp_numeric(&d(-1, 0)), Ordering::Less);
//     }
//
//     #[test]
//     fn ord_and_eq_agree_for_canonical_values() {
//         let values = [
//             d(0, 0),
//             d(1, 0),
//             d(-1, 0),
//             d(15, 1),
//             d(-15, 1),
//             d(1, MAX_SCALE),
//         ];
//         for a in &values {
//             for b in &values {
//                 assert_eq!(a.cmp(b) == Ordering::Equal, a == b);
//             }
//         }
//     }
//
//     #[test]
//     fn scaled_value_is_unique_for_canonical_values() {
//         assert_eq!(d(15, 1).scaled_value(), d(15, 1).scaled_value());
//         assert_ne!(d(15, 1).scaled_value(), d(2, 0).scaled_value());
//         assert_eq!(d(1, 0).scaled_value(), pow10(MAX_SCALE));
//     }
//
//     #[test]
//     fn arithmetic_is_exact() {
//         assert_eq!(d(11, 1).add(&d(22, 1)).unwrap(), d(33, 1));
//         assert_eq!(d(11, 1).add(&d(2, 0)).unwrap(), d(31, 1));
//         assert_eq!(d(15, 1).sub(&d(5, 1)).unwrap(), d(1, 0));
//         assert_eq!(d(1, 1).mul(&d(2, 1)).unwrap(), d(2, 2));
//         assert_eq!(d(1, 1).mul(&d(10, 0)).unwrap(), d(1, 0));
//         assert_eq!(d(15, 1).neg(), d(-15, 1));
//     }
//
//     #[test]
//     fn division_is_exact_or_refused() {
//         assert_eq!(d(1, 0).div(&d(4, 0)).unwrap(), d(25, 2));
//         assert_eq!(d(10, 0).div(&d(4, 0)).unwrap(), d(25, 1));
//         assert_eq!(d(1, 0).div(&d(3, 0)), Err(DecimalError::InexactDivision));
//         assert_eq!(d(1, 0).div(&d(0, 0)), Err(DecimalError::DivisionByZero));
//         let ten_pow_39 = pow10(39);
//         assert_eq!(
//             d(1, 0).div(&Decimal::new(ten_pow_39, 0).unwrap()),
//             Err(DecimalError::InexactDivision)
//         );
//     }
//
//     #[test]
//     fn printing_is_canonical() {
//         assert_eq!(d(15, 1).to_plain_string(), "1.5");
//         assert_eq!(d(15, 1).to_literal_string(), "1.5d");
//         assert_eq!(d(5, 2).to_plain_string(), "0.05");
//         assert_eq!(d(-5, 2).to_literal_string(), "-0.05d");
//         assert_eq!(d(0, 0).to_literal_string(), "0d");
//         assert_eq!(d(2, 0).to_literal_string(), "2d");
//     }
//
//     #[test]
//     fn serde_refuses_a_noncanonical_pair() {
//         // `num-bigint`'s serde form is `(sign, digits)`, so build the value rather than guessing
//         // its JSON spelling.
//         let coefficient = serde_json::to_value(BigInt::from(150)).unwrap();
//         let noncanonical = serde_json::json!({ "coefficient": coefficient, "scale": 2 });
//         assert!(serde_json::from_value::<Decimal>(noncanonical).is_err());
//
//         let coefficient = serde_json::to_value(BigInt::from(15)).unwrap();
//         let canonical = serde_json::json!({ "coefficient": coefficient, "scale": 1 });
//         assert!(serde_json::from_value::<Decimal>(canonical).is_ok());
//     }
// }
