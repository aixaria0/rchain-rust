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
//! | `GUri` | `Symbol` | see the asymmetry below |
//! | `EList` | `List` | a `remainder` pattern is refused |
//! | `ETuple` | `List` | **not a `Record`** — see the tuple note below; `Record` is inbound-only |
//! | `ParMap` (string keys) | `Struct` | a non-string key is refused |
//! | `GUnforgeable`, `Bundle` | — | **refused: a capability is not data** |
//! | `ParSet` | — | refused: Syrup has no set, and a list would lose order-insensitivity silently |
//! | operators, `EMethod`, sends/receives/news/matches | — | refused: a process is not a value |
//! | `Symbol` **inbound** | — | refused: Rholang has no symbol, so it has nowhere to land |
//! | `Float64` **inbound** | — | refused: Rholang has no float |
//!
//! **The `GUri`/`Symbol` asymmetry is deliberate and worth a second opinion.** Outbound, a URI is
//! tagged as a Symbol so a peer sees a structured name rather than an opaque string. Inbound, an
//! untagged Symbol is refused, so a URI that leaves does not come back. The alternative — mapping a
//! Symbol to a `GUri` — would let a peer mint an arbitrary `rho:id:…` URI; that is *probably*
//! harmless (registry URIs are hashes of public keys, and knowing one grants nothing without a
//! `lookup!` that the registry answers anyway), but it is a decision about authority and this
//! module takes the refusing side until something needs otherwise.
//!
//! **A tuple crosses as a list, and that is a correction, not a preference.** The first cut mapped
//! `ETuple` to a Syrup `Record` "so a tuple survives a round trip", which is only *spellable* when
//! the tuple happens to start with a symbol, string or byte string: Syrup records are **labelled**,
//! and a label must be one of those three. `(true, 0)` — an ERTP reply, and the shape every
//! `(ok, value)` answer in this codebase uses — encoded to a record whose label was `true`, which
//! Endo refused outright (`Unexpected type "boolean", Syrup record labels must be strings, selectors
//! or bytestrings`) and which the Python suite would have refused too had anything ever sent one. A
//! Syrup `List` is the ordered, heterogeneous, unlabelled thing a Rholang tuple actually is, so that
//! is what it becomes; `Record` stays **inbound-only**, where the label is a peer's and the tuple is
//! the counterpart (a `desc:import-object N` the node hands to a contract). A tuple that crosses and
//! comes back is a list — the one loss, and it is the loss Syrup forces.

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
        // A tuple is a list on the wire: Syrup's `Record` is *labelled*, and a Rholang tuple has no
        // label — see the module note. A record leaves this module only where a caller built one by
        // hand with a label it chose (`Desc::to_syrup`'s `<desc:import-object N>`).
        Expr::ETuple(tuple) => Ok(Value::List(each(&tuple.ps)?)),
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
        Value::Record(xs) => ground(Expr::ETuple(rchain_models::ast::ETuple {
            ps: each_to_par(xs)?,
            ..Default::default()
        })),
        Value::Struct(m) => ground(Expr::EMap(rchain_models::ast::ParMap {
            kvs: m
                .iter()
                .map(|(k, v)| Ok((ground(Expr::GString(k.clone())), value_to_par(v)?)))
                .collect::<Result<Vec<_>, BridgeError>>()?,
            ..Default::default()
        })),
        Value::Symbol(_) => {
            return Err(BridgeError::NoCounterpart(
                "a Symbol — Rholang has no symbol for it to land in",
            ))
        }
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
    fn collections_round_trip_and_a_tuple_crosses_as_a_list() {
        let list = ground(Expr::EList(rchain_models::ast::EList {
            ps: vec![ground(Expr::GInt(1)), ground(Expr::GString("a".into()))],
            ..Default::default()
        }));
        round_trip(&list);
        // A tuple is a list on the wire, and comes back as one. The first cut sent it as a `Record`
        // and asserted the two stayed distinct; that only holds for a tuple whose head is a valid
        // Syrup *label*, and `(true, 0)` — every `(ok, value)` reply in this codebase — is not one.
        let tuple = ground(Expr::ETuple(rchain_models::ast::ETuple {
            ps: vec![ground(Expr::GBool(true)), ground(Expr::GInt(0))],
            ..Default::default()
        }));
        let crossed = par_to_value(&tuple).expect("a tuple crosses");
        assert_eq!(
            crossed,
            Value::List(vec![Value::Bool(true), Value::Int(0.into())])
        );
        let back = value_to_par(&crossed).expect("and comes back");
        assert_eq!(
            back,
            ground(Expr::EList(rchain_models::ast::EList {
                ps: vec![ground(Expr::GBool(true)), ground(Expr::GInt(0))],
                ..Default::default()
            })),
            "a crossed tuple arrives as the list it was sent as"
        );
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
    fn a_symbol_inbound_has_nowhere_to_land() {
        assert_eq!(
            value_to_par(&Value::Symbol("op:deliver".into())),
            Err(BridgeError::NoCounterpart(
                "a Symbol — Rholang has no symbol for it to land in"
            ))
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

    /// The asymmetry is deliberate: a URI leaves as a Symbol and does **not** come back.
    #[test]
    fn a_uri_leaves_as_a_symbol_and_is_not_accepted_back() {
        let uri = ground(Expr::GUri("rho:id:abc".into()));
        assert_eq!(par_to_value(&uri), Ok(Value::Symbol("rho:id:abc".into())));
        assert!(value_to_par(&Value::Symbol("rho:id:abc".into())).is_err());
    }
}
