//! Gas-amplification probes for the October 2026 security re-audit (report-only instrumentation).
//!
//! The question: is an operation reachable from a deploy's term superlinear in the input it is handed
//! while its phlo charge is flat? These probes measure the **charged cost** and the **wall time** of
//! the collection conversions and methods in `reduce.rs` at several input sizes.
//!
//! They are env-gated (`RNODE_GAS_PROBE=1`) so the ordinary suite stays fast; run them release:
//!
//! ```text
//! RNODE_GAS_PROBE=1 cargo test -p rchain-rholang --test gas_amplification --release -- --nocapture
//! ```
//!
//! The committed evidence in `spec/audit/evidence/` carries the numbers.

mod common;

use std::time::Instant;

use common::build_runtime;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;

fn probe_enabled() -> bool {
    std::env::var("RNODE_GAS_PROBE").ok().as_deref() == Some("1")
}

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::from_init(&[0u8; 32])
}

fn list_of_ints(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    format!("[{}]", items.join(","))
}

fn list_of_pairs(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| format!("({i},{i})")).collect();
    format!("[{}]", items.join(","))
}

/// One evaluation: `(charged phlo, wall millis)`.
async fn measure(term: &str) -> (i64, u128) {
    let rt = build_runtime(false).await;
    let start = Instant::now();
    let res = rt
        .evaluate(term, &fixed_rand())
        .await
        .expect("no node fault");
    assert!(
        res.errors.is_empty(),
        "the probe term must evaluate cleanly; got {:?}",
        res.errors
    );
    (res.cost.value, start.elapsed().as_millis())
}

/// **`toSet` on a list of N ints** — the conversion sorts N elements. The baseline is the same
/// literal with no conversion, so the parse charge is common to both and the delta is the method's.
///
/// A charge proportional to the work would make the delta grow with N. A flat charge keeps it
/// constant while the wall time grows; that is the finding, and it is printed rather than asserted.
#[tokio::test]
async fn a_conversion_charges_nothing_and_sorts() {
    if !probe_enabled() {
        return;
    }
    println!("case,n,plain_phlo,converted_phlo,delta_phlo,plain_ms,converted_ms");
    for n in [500usize, 1_000, 2_000, 4_000, 10_000, 20_000, 40_000] {
        let list = list_of_ints(n);
        let (plain, plain_ms) = measure(&format!(r#"@"out"!({list})"#)).await;
        let (converted, ms) = measure(&format!(r#"@"out"!({list}.toSet())"#)).await;
        println!(
            "toSet,{n},{plain},{converted},{},{plain_ms},{ms}",
            converted - plain
        );
    }
}

/// **`toMap` on a list of N pairs** — same shape as `toSet`, through `par_map`.
#[tokio::test]
async fn a_map_conversion_charges_nothing_and_sorts() {
    if !probe_enabled() {
        return;
    }
    println!("case,n,plain_phlo,converted_phlo,delta_phlo,plain_ms,converted_ms");
    for n in [500usize, 1_000, 2_000, 4_000, 10_000] {
        let pairs = list_of_pairs(n);
        let (plain, plain_ms) = measure(&format!(r#"@"out"!({pairs})"#)).await;
        let (converted, ms) = measure(&format!(r#"@"out"!({pairs}.toMap())"#)).await;
        println!(
            "toMap,{n},{plain},{converted},{},{plain_ms},{ms}",
            converted - plain
        );
    }
}

/// **`keys` on a map of N pairs** — builds and sorts N keys. The baseline is `toMap()` of the same
/// list, so the conversion's own charge is held fixed and the delta is `keys`'.
#[tokio::test]
async fn keys_is_charged_flat() {
    if !probe_enabled() {
        return;
    }
    println!("case,n,baseline_phlo,keys_phlo,delta_phlo,baseline_ms,keys_ms");
    for n in [500usize, 1_000, 2_000, 4_000, 10_000] {
        let pairs = list_of_pairs(n);
        let (base, base_ms) = measure(&format!(r#"@"out"!({pairs}.toMap())"#)).await;
        let (keys, ms) = measure(&format!(r#"@"out"!({pairs}.toMap().keys())"#)).await;
        println!("keys,{n},{base},{keys},{},{base_ms},{ms}", keys - base);
    }
}

/// **`set` on a map of N pairs** — clones the map, scans it, re-sorts it.
#[tokio::test]
async fn set_is_charged_flat() {
    if !probe_enabled() {
        return;
    }
    println!("case,n,baseline_phlo,set_phlo,delta_phlo,baseline_ms,set_ms");
    for n in [500usize, 1_000, 2_000, 4_000, 10_000] {
        let pairs = list_of_pairs(n);
        let (base, base_ms) = measure(&format!(r#"@"out"!({pairs}.toMap())"#)).await;
        let (set, ms) = measure(&format!(r#"@"out"!({pairs}.toMap().set(0,0))"#)).await;
        println!("set,{n},{base},{set},{},{base_ms},{ms}", set - base);
    }
}

/// **The September union amplification, re-measured**: the charge must track its work now.
#[tokio::test]
async fn the_union_charge_tracks_its_work() {
    if !probe_enabled() {
        return;
    }
    println!("case,n,plain_phlo,union_phlo,delta_phlo,plain_ms,union_ms");
    for n in [1_000usize, 5_000, 10_000] {
        let list = list_of_ints(n);
        let (plain, plain_ms) = measure(&format!(r#"@"out"!({list})"#)).await;
        let term = format!(r#"@"out"!(({list}).toSet().union(({list}).toSet()))"#);
        let (union, ms) = measure(&term).await;
        let delta = union - plain;
        println!("union,{n},{plain},{union},{delta},{plain_ms},{ms}");
        assert!(
            delta >= 3 * n as i64,
            "a union of two N-element sets must charge for its elements (>= 3N); {n}: {delta}"
        );
    }
}
