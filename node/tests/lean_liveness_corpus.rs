//! Law 58's `liveness` layer — the node's own live-weight filter on the same numbers.
//!
//! `spec/conformance/liveness.tsv` is emitted by `lake exe rchain-corpus --layer liveness` from
//! `Rchain/Corpus.lean`'s `livenessCases`, where each row's verdict is `decide`d against the model's
//! `liveOf` (`Rchain/Casper/Rounds.lean`) — the bonded senders whose latest message is within the window
//! of the tip, with their stakes. This file is the other party: it reads the same tip, window, bonds and
//! latest-message map and calls `rchain_block_storage::dag::liveness::live_weight_set`.
//!
//! **What the layer is about.** The two sides spell the window differently — the model's `h + w ≥ tip`
//! against the node's `heights_behind(tip, h) ≤ window` — and `Rounds.lean`'s `window_iff_heights_behind`
//! proves those are one test. What the cases exercise is the **filter**: the boundary (`h + w = tip` is
//! live, one past it is not), and the case C174 is about — a bonded sender with **no message at all** is
//! not live, because the node's lookup is an `is_some_and` and a default height would make silence read as
//! liveness, which is how a silent bonded validator made a partition unsatisfiable (#70, measured
//! 2026-09-29 at `100/100/50`).
//!
//! The two sides never compare representations: each computes its own live set on the same numbers, and
//! the numbers are what ties them. Both filter the bonds **in place**, so the corpus lists bonds in
//! ascending sender order and the rendering below needs no sort.

use std::collections::BTreeMap;

use rchain_block_storage::dag::liveness::live_weight_set;
use rchain_shared::refined::{BlockHeight, NonNegI64};

/// The corpus's declared size (`Rchain/Corpus.lean`'s `livenessCaseCount`).
const LIVENESS_CASES: usize = 11;

/// `s:stake,s:stake` → a bonds map. Ascending in the corpus, and a `BTreeMap` keeps it so.
fn bonds_of(field: &str) -> BTreeMap<u64, NonNegI64> {
    let mut out = BTreeMap::new();
    for pair in field.split(',').filter(|p| !p.is_empty()) {
        let (sender, stake) = pair.split_once(':').expect("a bond is `sender:stake`");
        out.insert(
            sender.parse().expect("the sender"),
            NonNegI64::try_from(stake.parse::<i64>().expect("the stake")).expect("non-negative"),
        );
    }
    out
}

/// `s:height,...` → a latest-message map. A sender **absent from this field has no message at all**,
/// which is the distinction the rule turns on — so the corpus spells absence by omission, not by a
/// sentinel height.
fn latest_of(field: &str) -> BTreeMap<u64, BlockHeight> {
    let mut out = BTreeMap::new();
    for pair in field.split(',').filter(|p| !p.is_empty()) {
        let (sender, height) = pair
            .split_once(':')
            .expect("a latest entry is `sender:height`");
        out.insert(
            sender.parse().expect("the sender"),
            BlockHeight::try_from(height.parse::<i64>().expect("the height"))
                .expect("non-negative"),
        );
    }
    out
}

/// The live weight set, rendered the way the corpus renders it: `s:stake` ascending, `,`-separated.
fn render(live: &BTreeMap<u64, NonNegI64>) -> String {
    live.iter()
        .map(|(sender, stake)| format!("{sender}:{}", i64::from(*stake)))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn the_node_agrees_with_the_model_on_every_live_weight_set() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../spec/conformance/liveness.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e}\n(run tools/emit-lean-corpus.sh)",
            path.display()
        )
    });

    let mut cases = 0usize;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            6,
            "corpus line {}: expected 6 columns, got {}",
            i + 1,
            columns.len()
        );
        assert_eq!(
            columns[0],
            "liveness",
            "corpus line {}: unexpected layer {:?}",
            i + 1,
            columns[0]
        );
        let tip = BlockHeight::try_from(columns[1].parse::<i64>().expect("the tip"))
            .expect("non-negative");
        let window: i64 = columns[2].parse().expect("the window");
        let bonds = bonds_of(columns[3]);
        let latest = latest_of(columns[4]);
        let expected = columns[5];

        let live = live_weight_set(&bonds, &latest, tip, window);
        assert_eq!(
            render(&live),
            expected,
            "line {}: tip {tip} window {window} bonds [{}] latest [{}] — the node says [{}], the Lean \
             model says [{expected}] (spec/conformance/liveness.tsv, law 58). The window is `h + w >= tip` \
             and the node spells it `heights_behind(tip, h) <= window`, so a disagreement here is a sender \
             kept or dropped at the boundary, or a silent one read as live.",
            i + 1,
            columns[3],
            columns[4],
            render(&live),
        );
        cases += 1;
    }

    assert_eq!(
        cases, LIVENESS_CASES,
        "the corpus carries {LIVENESS_CASES} cases (Rchain/Corpus.lean's livenessCaseCount); {cases} were read"
    );
}
