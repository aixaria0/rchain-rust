//! A full rholang reduction on `wasm32-unknown-unknown` — parse → normalize → reduce → RSpace — with
//! **no tokio runtime**, driven by `wasm-bindgen-futures` (issue #98).
//!
//! The node runs the reducer on tokio; the target has no runtime, so `reduce.rs`'s `join_spawned`
//! hands each continuation-dispatch future to the host executor instead (`spawn_reduce_task`). This
//! file is the proof that the seam works: the second case matches a `for` against a `!`, which is
//! exactly the continuation dispatch that goes through it.
//!
//! Run: `cargo test -p rchain-rholang --test wasm_reduce --target wasm32-unknown-unknown`.

#![cfg(target_arch = "wasm32")]

mod common;

use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_models::ast::Expr;
use rchain_models::par_ops::from_expr;
use rchain_models::sorted::SortedProc;
use rchain_rholang::scheduler::EffectMode;
use wasm_bindgen_test::wasm_bindgen_test;

/// The term the scheduler cases share: a produce matched by a `for`, whose body produces on `done`.
/// The match is a continuation dispatch — the path `spawn_reduce` exists for.
const PRODUCE_AND_MATCH: &str = r#"@"chan"!(42) | for (_ <- @"chan") { @"done"!(42) }"#;

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::from_init(&[0u8; 32])
}

fn chan(name: &str) -> SortedProc {
    SortedProc::new(from_expr(Expr::GString(name.to_string())))
}

#[wasm_bindgen_test]
async fn a_produce_reduces_on_wasm() {
    let rt = common::build_runtime(false).await;
    let res = rt
        .evaluate(r#"@"chan"!(42)"#, &fixed_rand())
        .await
        .expect("evaluate");
    assert!(res.succeeded(), "unexpected errors: {:?}", res.errors);
    assert_eq!(
        rt.get_data_par(&chan("chan")).await.expect("get_data_par"),
        vec![from_expr(Expr::GInt(42))]
    );
}

/// The continuation path — the one `spawn_reduce_task` exists for. A `for` matching a `!` dispatches
/// a continuation through `join_spawned`; if the wasm arm did not hand the future to the host
/// executor, this hangs or panics rather than reaching the `done` channel.
#[wasm_bindgen_test]
async fn a_continuation_dispatches_on_wasm() {
    let rt = common::build_runtime(false).await;
    let res = rt
        .evaluate(PRODUCE_AND_MATCH, &fixed_rand())
        .await
        .expect("evaluate");
    assert!(res.succeeded(), "unexpected errors: {:?}", res.errors);
    assert_eq!(
        rt.get_data_par(&chan("done")).await.expect("get_data_par"),
        vec![from_expr(Expr::GInt(42))],
        "the continuation must have run and produced on `done`"
    );
}

/// Every effect scheduler, on the target. `reduce.rs` spawns in three more places than the
/// continuation dispatch — the `concurrent` fork-join, the Gate chain (Law 21), and the Relaxed
/// task set (Law 20) — and each takes the same `spawn_reduce` seam, so each is driven here rather
/// than asserted about.
async fn reduces_and_reports(rt: &rchain_rholang::runtime::RhoRuntime, what: &str) {
    let res = rt
        .evaluate(PRODUCE_AND_MATCH, &fixed_rand())
        .await
        .expect("evaluate");
    assert!(
        res.succeeded(),
        "{what}: unexpected errors: {:?}",
        res.errors
    );
    assert_eq!(
        rt.get_data_par(&chan("done")).await.expect("get_data_par"),
        vec![from_expr(Expr::GInt(42))],
        "{what}: the continuation must have run and produced on `done`"
    );
}

#[wasm_bindgen_test]
async fn the_concurrent_fork_join_reduces_on_wasm() {
    let rt = common::build_runtime_with_mode(true, EffectMode::Sequential).await;
    reduces_and_reports(&rt, "concurrent").await;
}

#[wasm_bindgen_test]
async fn the_gate_scheduler_reduces_on_wasm() {
    let rt = common::build_runtime_with_mode(true, EffectMode::Gate).await;
    reduces_and_reports(&rt, "gate").await;
}

#[wasm_bindgen_test]
async fn the_relaxed_scheduler_reduces_on_wasm() {
    let rt = common::build_runtime_with_mode(true, EffectMode::Relaxed).await;
    reduces_and_reports(&rt, "relaxed").await;
}
