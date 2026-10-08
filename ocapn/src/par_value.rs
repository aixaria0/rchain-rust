//! `Par` ↔ Syrup: the bridge's value translation.
//!
//! This is a **partial** map, and deliberately so. Its refusal rule is the same one that keeps a
//! capability a capability:
//!
//! > **An unforgeable name or a bundle may cross CapTP only as a descriptor in the export table,
//! > never as a Syrup value.** Mapping one to data would copy the authority — and this codebase
//! > enforces no copy-protection on names (possession *is* authority: `rho:rchain:revVault`'s
//! > `transfer` takes the caller's `deployerId` as the capability that authorises it). So a name
//! > is *refused*, not downgraded.
//!
//! Every refusal is a test, because a rule that is not tested is a rule that drifts.
//!
//! **What maps, and what does not.**
//!
//! | `Par` | Syrup | note |
//! |---|---|---|
//! | `GBool` | `Bool` | |
//! | `GInt`, `GBigInt` | `Int` | Syrup's integer is arbitrary-precision, so both are lossless |
//! | `GString` | `String` | |
//! | `GByteArray` | `Bytes` | |
//! | `GUri` | `Symbol` | a symbol comes back as a `GUri` |
//! | `EList` | `List` | a `remainder` pattern is refused |
//! | `ETuple` | `Record` | `<desc:tagged 'rho:tuple' [fields]>` — see the tuple note below |
//! | `ParMap` (string keys) | `Struct` | a non-string key is refused |
//! | `GUnforgeable`, `Bundle` | — | **refused: a capability is not data** |
//! | `ParSet` | — | refused: Syrup has no set, and a list would lose order-insensitivity silently |
//! | operators, `EMethod`, sends/receives/news/matches | — | refused: a process is not a value |
//! | any other `Record` **inbound** | — | refused: a labelled record that is not the tagged tuple |
//! | `Float64` **inbound** | — | refused: Rholang has no float |
//!
//! **The map round-trips its domain, and its refusals are named** (AUDIT C226). The law is
//! `spec/Rchain/Syrup.lean`, stated the way Law 42 states the JSON round trip: a value the decoder
//! answers encodes back and decodes again (`syrup_decode_encode`); two wireable values that decode
//! to one `Par` are one wire form (`syToPar_injective`); and the encoder never emits a shape outside
//! the domain (`parToSy_wireable`). The last is §1.6's no-silent-partiality on a wire: a value with
//! no counterpart is *refused*, never approximated by one that nearly fits.
//!
//! **A tuple crosses as OCapN's tagged value**, `<desc:tagged 'rho:tuple' [fields…]>`. Two earlier
//! shapes are refuted by the references, and both are worth not re-trying:
//!
//! - **a bare record.** Syrup records are *labelled*, and the label must be a string, selector or
//!   byte string (`@endo/ocapn`'s `decode.js`), so `(true, 0)` — the shape every `(ok, value)` reply
//!   in this codebase uses — would need the label `true`, which Endo refuses outright. A record is
//!   not even in Endo's CapTP passable union (`{list, struct, tagged}`), so it would not survive as
//!   an argument either.
//! - **a list.** A list is unlabelled, so a tuple sent as one comes back as a `List`, which no
//!   contract's `@(brand, value)` tuple pattern matches. Under the law that is worse than a bug: it
//!   makes one wire form stand for two values, which `syToPar_injective` forbids.
//!
//! The tagged form is the union's own extension point, whose `value` may be any passable, and both
//! the Python suite and Endo carry it. Outbound a URI is a `Symbol` and inbound a `Symbol` is a
//! `GUri` — the asymmetry that used to refuse a returning URI is gone, because it was a lossy map.
//!
//! **Two shapes are outside the domain, named rather than implied:** a `Float64` (Rholang has no
//! float), and a **small** `GBigInt` — Syrup's integer is one type where Rholang has two, so
//! `GBigInt 5` and `GInt 5` share a wire form and the encoder emits the narrow one (AUDIT C226 is
//! the tuple half of this row; the register holds the rest).

use num_bigint::BigInt;
use rchain_models::ast::{Expr, Par};

use crate::syrup::Value;

/// Why a `Par` or a Syrup value cannot cross.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// A capability — an unforgeable name or a bundle. It crosses CapTP as a descriptor, never as
    /// data; mapping it to a Syrup value would copy the authority.
    Capability(&'static str),
    /// A value or process with no counterpart on the other side.
    NoCounterpart(&'static str),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Capability(what) => {
                write!(
                    f,
                    "bridge: {what} is a capability, not data — pass it as a descriptor"
                )
            }
            BridgeError::NoCounterpart(what) => {
                write!(f, "bridge: {what} has no counterpart on the other side")
            }
        }
    }
}

impl std::error::Error for BridgeError {}

/// The record label a Rholang tuple crosses as: OCapN's **tagged** value, `<desc:tagged :tagName
/// value>` — the CapTP union's own extension point (`@endo/ocapn`'s `OcapnTaggedCodec`), whose
/// `value` may be any passable.
pub const TAGGED_LABEL: &str = "desc:tagged";

/// The tag that says the payload is a Rholang tuple.
pub const TUPLE_TAG: &str = "rho:tuple";

/// A one-expression `Par` — the shape every ground value takes.
fn ground(expr: Expr) -> Par {
    Par {
        exprs: vec![expr],
        ..Par::default()
    }
}

/// A `Par`, if it is a value, as a Syrup value.
pub fn par_to_value(par: &Par) -> Result<Value, BridgeError> {
    if !par.unforgeables.is_empty() {
        return Err(BridgeError::Capability("an unforgeable name"));
    }
    if !par.bundles.is_empty() {
        return Err(BridgeError::Capability("a bundle"));
    }
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.connectives.is_empty()
    {
        return Err(BridgeError::NoCounterpart("a process"));
    }
    match par.exprs.as_slice() {
        [] => Err(BridgeError::NoCounterpart("Nil")),
        [expr] => expr_to_value(expr),
        _ => Err(BridgeError::NoCounterpart("a parallel composition")),
    }
}

fn expr_to_value(expr: &Expr) -> Result<Value, BridgeError> {
    match expr {
        Expr::GBool(b) => Ok(Value::Bool(*b)),
        Expr::GInt(n) => Ok(Value::Int(BigInt::from(*n))),
        Expr::GBigInt(n) => Ok(Value::Int(n.clone())),
        Expr::GString(s) => Ok(Value::String(s.clone())),
        Expr::GUri(u) => Ok(Value::Symbol(u.clone())),
        Expr::GByteArray(b) => Ok(Value::Bytes(b.clone())),
        Expr::EList(list) => {
            if list.remainder.is_some() {
                return Err(BridgeError::NoCounterpart(
                    "a list with a remainder pattern",
                ));
            }
            Ok(Value::List(each(&list.ps)?))
        }
        // **A tuple crosses as OCapN's tagged value**, `<desc:tagged 'rho:tuple' [fields…]>` — a
        // Syrup record, so it is *labelled*, which is what a bare record cannot be for a tuple: the
        // label of `(true, 0)` would be `true`, and Endo refuses a non-selector label outright. The
        // tagged form is the union's own extension point and the only record shape its passable
        // union carries (`{list, struct, tagged}`), so this is the shape that comes back. AUDIT
        // C226; the law is `Rchain.syrup_decode_encode` in `spec/Rchain/Syrup.lean`.
        Expr::ETuple(tuple) => Ok(Value::Record(vec![
            Value::Symbol(TAGGED_LABEL.to_string()),
            Value::Symbol(TUPLE_TAG.to_string()),
            Value::List(each(&tuple.ps)?),
        ])),
        Expr::ESet(_) => Err(BridgeError::NoCounterpart(
            "a set (Syrup has no set, and a list would lose order-insensitivity)",
        )),
        Expr::EMap(map) => {
            if map.remainder.is_some() {
                return Err(BridgeError::NoCounterpart("a map with a remainder pattern"));
            }
            let mut entries = std::collections::BTreeMap::new();
            for (k, v) in &map.kvs {
                let key = match par_to_value(k)? {
                    Value::String(s) => s,
                    _ => return Err(BridgeError::NoCounterpart("a map with a non-string key")),
                };
                entries.insert(key, par_to_value(v)?);
            }
            Ok(Value::Struct(entries))
        }
        _ => Err(BridgeError::NoCounterpart(
            "an operator or method call — a process, not a value",
        )),
    }
}

fn each(ps: &[Par]) -> Result<Vec<Value>, BridgeError> {
    ps.iter().map(par_to_value).collect()
}

/// A Syrup value, if it has a `Par` shape, as one.
pub fn value_to_par(value: &Value) -> Result<Par, BridgeError> {
    Ok(match value {
        Value::Bool(b) => ground(Expr::GBool(*b)),
        // Syrup's integer is arbitrary-precision; Rholang has a machine word and a bignum, so pick
        // the narrow one when it fits rather than widening everything to `GBigInt`.
        Value::Int(n) => ground(match i64::try_from(n.clone()) {
            Ok(small) => Expr::GInt(small),
            Err(_) => Expr::GBigInt(n.clone()),
        }),
        Value::String(s) => ground(Expr::GString(s.clone())),
        Value::Bytes(b) => ground(Expr::GByteArray(b.clone())),
        Value::List(xs) => ground(Expr::EList(rchain_models::ast::EList {
            ps: each_to_par(xs)?,
            ..Default::default()
        })),
        // A record is what a tuple crosses as, and only in the one tagged shape: `<desc:tagged
        // 'rho:tuple' [fields]>`. Any *other* labelled record is a peer's, has no Rholang
        // counterpart, and is refused with its own reason rather than read as a tuple — the reason
        // the encoder never emits one either (C226, `Rchain.parToSy_wireable` in `Syrup.lean`).
        Value::Record(xs) => match xs.as_slice() {
            [Value::Symbol(label), Value::Symbol(tag), Value::List(fields)]
                if label == TAGGED_LABEL && tag == TUPLE_TAG =>
            {
                ground(Expr::ETuple(rchain_models::ast::ETuple {
                    ps: each_to_par(fields)?,
                    ..Default::default()
                }))
            }
            _ => {
                return Err(BridgeError::NoCounterpart(
                    "a labelled record that is not the tagged tuple",
                ))
            }
        },
        Value::Struct(m) => ground(Expr::EMap(rchain_models::ast::ParMap {
            kvs: m
                .iter()
                .map(|(k, v)| Ok((ground(Expr::GString(k.clone())), value_to_par(v)?)))
                .collect::<Result<Vec<_>, BridgeError>>()?,
            ..Default::default()
        })),
        // A symbol is a URI's wire form: `GUri → Symbol` outbound, so this is the leg that makes the
        // round trip hold. Refusing it here is what made a URI leave and never come back — a lossy
        // map, which is what `Rchain.parToSy_wireable` forbids (AUDIT C226).
        Value::Symbol(s) => ground(Expr::GUri(s.clone())),
        Value::Float64(_) => {
            return Err(BridgeError::NoCounterpart(
                "a Float64 — Rholang has no float",
            ))
        }
    })
}

fn each_to_par(xs: &[Value]) -> Result<Vec<Par>, BridgeError> {
    xs.iter().map(value_to_par).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_models::ast::{Bundle, GDeployId, GDeployerId, GPrivate};

    fn round_trip(par: &Par) {
        let value = par_to_value(par).expect("maps out");
        let back = value_to_par(&value).expect("maps in");
        assert_eq!(&back, par, "round trip changed the value");
    }

    #[test]
    fn ground_values_round_trip() {
        round_trip(&ground(Expr::GBool(true)));
        round_trip(&ground(Expr::GInt(-7)));
        round_trip(&ground(Expr::GBigInt(
            BigInt::parse_bytes(b"340282366920938463463374607431768211456", 10).unwrap(),
        )));
        round_trip(&ground(Expr::GString("hi".into())));
        round_trip(&ground(Expr::GByteArray(vec![0, 1, 255])));
    }

    #[test]
    fn a_machine_int_stays_a_machine_int() {
        // The narrow type is chosen when it fits, so a round trip does not widen `GInt` to `GBigInt`.
        assert_eq!(
            par_to_value(&ground(Expr::GInt(7))),
            Ok(Value::Int(7.into()))
        );
        assert_eq!(
            value_to_par(&Value::Int(7.into())),
            Ok(ground(Expr::GInt(7)))
        );
    }

    #[test]
    fn collections_round_trip_and_a_tuple_stays_a_tuple() {
        let list = ground(Expr::EList(rchain_models::ast::EList {
            ps: vec![ground(Expr::GInt(1)), ground(Expr::GString("a".into()))],
            ..Default::default()
        }));
        round_trip(&list);
        // **The falsifier for C226.** A tuple crosses as `<desc:tagged 'rho:tuple' [fields]>` and
        // comes back a *tuple* — which is what makes an ERTP amount `(brand, value)` reach an arm
        // that matches `@(brand, value)`. Before this, a tuple crossed as a `List` and came back an
        // `EList`, so a peer could hold a purse and not fund it. `(true, 0)` is the shape every
        // `(ok, value)` reply uses, so it is the one that has to work.
        let tuple = ground(Expr::ETuple(rchain_models::ast::ETuple {
            ps: vec![ground(Expr::GBool(true)), ground(Expr::GInt(0))],
            ..Default::default()
        }));
        let crossed = par_to_value(&tuple).expect("a tuple crosses");
        assert_eq!(
            crossed,
            Value::Record(vec![
                Value::Symbol(TAGGED_LABEL.to_string()),
                Value::Symbol(TUPLE_TAG.to_string()),
                Value::List(vec![Value::Bool(true), Value::Int(0.into())]),
            ]),
            "a tuple crosses as the tagged record, not as a bare list"
        );
        assert_eq!(
            value_to_par(&crossed).expect("and comes back"),
            tuple,
            "the tuple survives the round trip it used to lose"
        );
        round_trip(&tuple);
        // A list is still a list: the two are not interchangeable, which is the injectivity the law
        // states (`Rchain.syToPar_injective`).
        assert_ne!(
            par_to_value(&list).unwrap(),
            par_to_value(&tuple).unwrap(),
            "a list and a tuple must not share a wire form"
        );
    }

    #[test]
    fn a_labelled_record_that_is_not_the_tuple_is_refused() {
        // A peer's own record has no Rholang counterpart, so it is refused with a reason rather than
        // read as a tuple — the other half of the law's domain clause.
        let peer_record = Value::Record(vec![
            Value::Symbol("desc:something".to_string()),
            Value::Int(1.into()),
        ]);
        assert!(matches!(
            value_to_par(&peer_record),
            Err(BridgeError::NoCounterpart(_))
        ));
    }

    #[test]
    fn a_string_keyed_map_round_trips() {
        round_trip(&ground(Expr::EMap(rchain_models::ast::ParMap {
            kvs: vec![(ground(Expr::GString("k".into())), ground(Expr::GInt(1)))],
            ..Default::default()
        })));
    }

    // --- the refusals, one test each -------------------------------------------------------

    #[test]
    fn an_unforgeable_name_is_refused_not_copied() {
        for name in [
            rchain_models::ast::GUnforgeable::GPrivate(GPrivate { id: vec![1, 2, 3] }),
            rchain_models::ast::GUnforgeable::GDeployId(GDeployId { sig: vec![4, 5] }),
            rchain_models::ast::GUnforgeable::GDeployerId(GDeployerId {
                public_key: vec![6],
            }),
            rchain_models::ast::GUnforgeable::GSysAuthToken,
        ] {
            let par = Par {
                unforgeables: vec![name.clone()],
                ..Par::default()
            };
            assert_eq!(
                par_to_value(&par),
                Err(BridgeError::Capability("an unforgeable name")),
                "{name:?} must not be copied as data"
            );
        }
    }

    #[test]
    fn a_bundle_is_refused_not_copied() {
        let par = Par {
            bundles: vec![Bundle::default()],
            ..Par::default()
        };
        assert_eq!(par_to_value(&par), Err(BridgeError::Capability("a bundle")));
    }

    #[test]
    fn a_process_is_refused() {
        // A send makes the `Par` a process, not a value.
        let par = Par {
            sends: vec![rchain_models::ast::Send::default()],
            ..Par::default()
        };
        assert_eq!(
            par_to_value(&par),
            Err(BridgeError::NoCounterpart("a process"))
        );
        assert_eq!(
            par_to_value(&Par::default()),
            Err(BridgeError::NoCounterpart("Nil"))
        );
    }

    #[test]
    fn a_symbol_inbound_lands_as_the_uri_it_came_from() {
        // A symbol is a URI's wire form. Refusing it here is what made a URI leave and never come
        // back — the lossy map C226's law (`Rchain.parToSy_wireable`) forbids.
        assert_eq!(
            value_to_par(&Value::Symbol("rho:rchain:ertp".into())),
            Ok(ground(Expr::GUri("rho:rchain:ertp".into())))
        );
    }

    #[test]
    fn a_float_inbound_has_no_rholang_source() {
        assert_eq!(
            value_to_par(&Value::Float64(1.5)),
            Err(BridgeError::NoCounterpart(
                "a Float64 — Rholang has no float"
            ))
        );
    }

    #[test]
    fn a_set_is_refused() {
        let par = ground(Expr::ESet(rchain_models::ast::ParSet {
            ps: vec![ground(Expr::GInt(1))],
            ..Default::default()
        }));
        assert!(matches!(
            par_to_value(&par),
            Err(BridgeError::NoCounterpart(_))
        ));
    }

    #[test]
    fn a_non_string_map_key_is_refused() {
        let par = ground(Expr::EMap(rchain_models::ast::ParMap {
            kvs: vec![(ground(Expr::GInt(1)), ground(Expr::GInt(2)))],
            ..Default::default()
        }));
        assert_eq!(
            par_to_value(&par),
            Err(BridgeError::NoCounterpart("a map with a non-string key"))
        );
    }

    #[test]
    fn an_operator_is_refused() {
        let par = ground(Expr::EPlus(
            Box::new(ground(Expr::GInt(1))),
            Box::new(ground(Expr::GInt(2))),
        ));
        assert!(matches!(
            par_to_value(&par),
            Err(BridgeError::NoCounterpart(_))
        ));
    }

    /// A URI leaves as a Symbol and comes back: the asymmetry that used to refuse the return was a
    /// lossy map, which is what `Rchain.parToSy_wireable` names as a violation rather than a choice.
    #[test]
    fn a_uri_leaves_as_a_symbol_and_comes_back() {
        let uri = ground(Expr::GUri("rho:id:abc".into()));
        assert_eq!(par_to_value(&uri), Ok(Value::Symbol("rho:id:abc".into())));
        assert_eq!(value_to_par(&Value::Symbol("rho:id:abc".into())), Ok(uri));
    }
}
