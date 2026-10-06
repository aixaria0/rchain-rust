//! Law 59 — the CapTP value bridge, checked 1:1 against the Lean model.
//!
//! `spec/conformance/syrup.tsv` is emitted by `lake exe rchain-corpus --layer syrup` from
//! `Rchain/Corpus.lean`'s `syrupCases`, where each case's verdict is `decide`d against
//! `Rchain/Syrup.lean` — the model's `parToSy` (the encode) and `renderSy` (the shape it produces).
//! A case's shape column is therefore the model's own rendering, not a hand-written expectation, and
//! this consumer is the second party: it runs the same value through the node's bridge and compares
//! the node's shape to the model's text.
//!
//! Each case additionally round-trips: `syToPar` of what `parToSy` produced, then encoded again,
//! which is `Rchain/Syrup.lean`'s `syrup_decode_encode` stated at the wire level. **The tuple row is
//! the point** (AUDIT C226): `(true, 0)` must cross as the tagged record and come back a tuple, which
//! is what lets a peer hand an ERTP amount to a contract that matches `@(brand, value)`. A shape that
//! drifted here is that class: a contract's pattern stops matching a value the peer sent, and
//! **nothing errors**.
//!
//! Each case runs on its own runtime and its term carries a control datum, so "the value produced
//! nothing" can be told from "the term never ran".

mod common;

use common::rho_runtime;
use rchain_ocapn::par_value::{par_to_value, value_to_par};
use rchain_ocapn::syrup::Value;

/// The corpus's declared size (`Rchain/Corpus.lean`'s `syrupCaseCount`).
const SYRUP_CASES: usize = 12;

/// The model's `renderSy`, mirrored — the compact wire text both sides agree on. A tuple renders as
/// the record it crosses as, so the tuple's row shows the shape rather than a name for it.
fn render(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "t" } else { "f" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::String(s) => format!("\"{s}\""),
        Value::Symbol(s) => format!("'{s}"),
        Value::Bytes(b) => format!("b{}", b.len()),
        Value::Float64(_) => "D".to_string(),
        Value::List(xs) => format!("[{}]", join(xs.iter().map(render).collect())),
        Value::Struct(entries) => format!(
            "{{{}}}",
            join(
                entries
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", render(v)))
                    .collect()
            )
        ),
        Value::Record(xs) => {
            format!("<{}>", xs.iter().map(render).collect::<Vec<_>>().join(" "))
        }
    }
}

fn join(parts: Vec<String>) -> String {
    parts.join(", ")
}

#[tokio::test]
async fn the_bridge_shape_is_the_lean_models_and_the_tuple_round_trips() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spec/conformance/syrup.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read {}: {e}\n(run tools/emit-lean-corpus.sh)", path.display())
    });

    let mut cases = 0usize;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut columns = line.split('\t');
        let layer = columns.next().unwrap_or_default();
        assert_eq!(layer, "syrup", "corpus line {}: unexpected layer {layer:?}", i + 1);
        let source = columns.next().expect("the value column");
        let expected = columns.next().expect("the shape column");
        assert!(columns.next().is_none(), "corpus line {}: trailing columns", i + 1);

        // The value as a datum, and a control datum that proves the term ran.
        let term = format!("@\"out\"!({source}) | @\"ctl\"!(\"ran\")");
        let rt = rho_runtime().await;
        let res = rt
            .evaluate_with_env(&term, &Default::default(), &fixed_rand())
            .await
            .expect("evaluate returns Ok");
        assert!(res.succeeded(), "{source}: the term failed to run: {:?}", res.errors);
        let ctl = rt.get_data_par(&chan("ctl")).await.expect("read ctl");
        assert_eq!(
            ctl.len(),
            1,
            "{source}: the control datum is missing, so the term never ran and the case proves nothing"
        );
        let data = rt.get_data_par(&chan("out")).await.expect("read out");
        assert_eq!(
            data.len(),
            1,
            "{source}: the datum must be on `out` exactly once; got {data:?}"
        );

        // The node's own encode, and the model's text.
        let value = par_to_value(&data[0]).unwrap_or_else(|e| {
            panic!("{source}: the bridge refuses a value the model maps (`parToSy`): {e}")
        });
        let got = render(&value);
        assert_eq!(
            got, expected,
            "{source}: the bridge produces {got}, the Lean model says {expected} \
             (Rchain/Syrup.lean's `parToSy` and `renderSy`; the model renders a tuple as the tagged \
             record it crosses as). A shape that drifted here is AUDIT C226's class — a contract's \
             pattern stops matching a value the peer sent, and nothing errors."
        );

        // The bridge's own round trip: decode what it encoded, encode again.
        let back = value_to_par(&value)
            .unwrap_or_else(|e| panic!("{source}: the bridge will not read back what it wrote: {e}"));
        let again = par_to_value(&back)
            .unwrap_or_else(|e| panic!("{source}: the decoded par does not re-encode: {e}"));
        assert_eq!(
            render(&again),
            got,
            "{source}: `syToPar` of what `parToSy` produced must encode back to the same shape \
             (Rchain/Syrup.lean's `syrup_decode_encode`). The tuple row is the one that used to fail \
             here — it crossed as a list and came back an `EList`, so a peer could hold an ERTP purse \
             and not fund it."
        );
        cases += 1;
    }

    assert_eq!(
        cases, SYRUP_CASES,
        "the corpus carries {SYRUP_CASES} cases (Rchain/Corpus.lean's `syrupCaseCount`); {cases} \
         were read"
    );
}

/// A fixed, deterministic random seed so fresh-name allocation is reproducible.
fn fixed_rand() -> rchain_crypto::hash::blake2b512_random::Blake2b512Random {
    rchain_crypto::hash::blake2b512_random::Blake2b512Random::from_init(&[0u8; 32])
}

/// The `SortedProc` for a string channel.
fn chan(name: &str) -> rchain_models::sorted::SortedProc {
    rchain_models::sorted::SortedProc::new(rchain_models::par_ops::from_expr(
        rchain_models::ast::Expr::GString(name.to_string()),
    ))
}
