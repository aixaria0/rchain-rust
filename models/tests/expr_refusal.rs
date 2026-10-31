use rchain_models::ast::{Expr, Par};
use rchain_shared::serialize::Serialize;

#[test]
fn an_empty_expression_is_not_a_false_value() {
    // An empty Expr inside Par.exprs is different from an empty Par (Nil).
    let error = <Par as Serialize<Par>>::decode(&[0x2a, 0x00])
        .expect_err("an empty expression must not become false");
    assert!(error.contains("expr_instance"), "{error}");
}

#[test]
fn an_unknown_only_expression_is_refused_after_real_wire_decode() {
    // Length-delimited Expr tag 36 with an empty payload, inside Par.exprs. Tag 36 is unknown.
    // Tag 35, this fixture's original choice, was `g_decimal` and is `reserved` since issue #527.
    let raw = [0x2a, 0x03, 0xa2, 0x02, 0x00];
    let error = <Par as Serialize<Par>>::decode(&raw)
        .expect_err("an unsupported expression must not become false");
    assert!(error.contains("expr_instance"), "{error}");
}

#[test]
fn an_unknown_expression_nested_in_a_collection_is_refused() {
    // Par -> Expr.EList -> EList.ps -> Par -> unknown-only Expr (tag 36).
    let raw = [
        0x2a, 0x0a, 0xa2, 0x01, 0x07, 0x0a, 0x05, 0x2a, 0x03, 0xa2, 0x02, 0x00,
    ];
    let error = <Par as Serialize<Par>>::decode(&raw)
        .expect_err("the recursive bridge must propagate the malformed expression");
    assert!(error.contains("expr_instance"), "{error}");
}

#[test]
fn explicit_false_and_nil_keep_their_existing_wire_values() {
    let nil = Par::default();
    assert_eq!(<Par as Serialize<Par>>::encode(&nil), Vec::<u8>::new());
    assert_eq!(<Par as Serialize<Par>>::decode(&[]), Ok(nil));

    let false_par = Par {
        exprs: vec![Expr::GBool(false)],
        ..Par::default()
    };
    let raw_false = [0x2a, 0x02, 0x08, 0x00];
    assert_eq!(<Par as Serialize<Par>>::encode(&false_par), raw_false);
    assert_eq!(<Par as Serialize<Par>>::decode(&raw_false), Ok(false_par));
}

#[test]
fn the_reserved_decimal_tag_is_refused_as_a_missing_variant() {
    // Par.exprs -> Expr whose only oneof field is 35 (length-delimited, tag 35 = 0x9a 0x02), with
    // an empty payload. Tag 35 was `g_decimal` (#380, removed by issue #527); the field is now
    // `reserved`, so prost leaves `expr_instance` empty and the bridge answers the same refusal it
    // gives for any unknown expression — rather than decoding a term the node cannot represent.
    let raw = [0x2a, 0x03, 0x9a, 0x02, 0x00];
    let error = <Par as Serialize<Par>>::decode(&raw)
        .expect_err("a reserved decimal tag must not decode as a term");
    assert!(error.contains("expr_instance"), "{error}");
}
