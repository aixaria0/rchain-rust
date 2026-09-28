//! The governance arithmetic saturates rather than wrapping (AUDIT F-7).
//!
//! Every value these functions read comes straight off a deploy's arguments — `RhoNumber::unapply`
//! yields any `i64` and nothing between the deploy and the arithmetic range-checks it. With plain
//! `+`/`-`/`*`, a deploy could therefore pick the values that wrap: in debug the reduction panicked and
//! the deploy failed; in **release it wrapped silently**, and the wrapped results were wrong answers
//! that every node computed identically, so nothing detected them. These tests pin the release
//! behaviour, where the bug was invisible.
//!
//! Each test names the wrapping result it replaces, so a reader can see what the old code did rather
//! than having to reconstruct it.

use std::collections::BTreeMap;

use qucalc::gov;

fn m(s: &str) -> String {
    s.to_string()
}

/// `1 + trust` used to wrap: a maximum trust level became `i64::MIN`, which the `.max(0)` clamp then
/// turned into **zero** — the largest possible endorsement scoring as no endorsement at all.
#[test]
fn a_maximum_trust_level_scores_as_a_maximum_not_as_zero() {
    let direct = vec![m("A")];
    let trust = BTreeMap::from([(m("A"), i64::MAX)]);

    let w = gov::resolve_weights(&direct, &BTreeMap::new(), &trust);

    assert_eq!(
        w.get("A"),
        Some(&i64::MAX),
        "a maximum trust level must carry a maximum weight, not clamp to zero"
    );
}

/// The discredit pass subtracts a voucher's stake from its level. At the floor, `level - stake` used
/// to wrap to `i64::MAX` — the subtraction that exists to *lower* a level raised it to the ceiling.
#[test]
fn a_voucher_at_the_floor_is_not_promoted_to_the_ceiling() {
    // Three peers at level 5 censuring B, which is the quorum for a two-eligible subset:
    // `max((2*2 + 2)/3, 2) == 2`.
    let levels = BTreeMap::from([
        (m("A"), 5),
        (m("B"), 5),
        (m("C"), 5),
        // The voucher, already at the floor.
        (m("D"), i64::MIN),
    ]);
    let censures = vec![(m("A"), m("B")), (m("C"), m("B"))];
    let vouchers = vec![(m("D"), m("B"), 1)];

    let (discredited, level) = gov::censure(&censures, &levels, &vouchers);

    assert!(
        discredited.contains("B"),
        "the censure must fire for this test to mean anything"
    );
    assert_eq!(
        level.get("D"),
        Some(&0),
        "a voucher at the floor must stay at the floor; wrapping promoted it to i64::MAX"
    );
}

/// The ranked tally's `total` is a sum over caller-supplied weights. Two maximal weights used to wrap
/// it negative, and the `total <= 0` guard then refused a ballot that was beyond any majority.
#[test]
fn a_ranked_tally_of_maximal_weights_still_elects() {
    let ballots = BTreeMap::from([(m("alice"), vec![m("X")]), (m("bob"), vec![m("Y")])]);
    // Both weigh the maximum: the sum saturates at `i64::MAX` rather than wrapping to -2.
    let weights = BTreeMap::from([(m("alice"), i64::MAX), (m("bob"), i64::MAX)]);

    let winner = gov::tally_ranked(&ballots, &weights);

    assert_eq!(
        winner,
        Some(m("X")),
        "the tally must run; a wrapped total short-circuited to None, reporting no winner at all"
    );
}

/// And the approval tally accumulates per option, where the same wrap made a landslide **loser** win:
/// two maximal approvals summed to a negative count and lost to a single vote.
#[test]
fn an_approval_tally_of_maximal_weights_does_not_elect_the_loser() {
    let ballots = BTreeMap::from([
        (m("alice"), vec![m("X")]),
        (m("bob"), vec![m("X")]),
        (m("carol"), vec![m("Y")]),
    ]);
    let weights = BTreeMap::from([
        (m("alice"), i64::MAX),
        (m("bob"), i64::MAX),
        (m("carol"), 1),
    ]);

    let winner = gov::tally_approval(&ballots, &weights);

    assert_eq!(
        winner,
        Some(m("X")),
        "the option with two maximal approvals must win; wrapping made its count negative and \
         elected the single-vote option instead"
    );
}
