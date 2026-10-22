# N=8 partition: source-backed progress height (experimental consensus correction)

Status: **Draft only / not release acceptance**. This patch changes deterministic
consensus inputs. It must not merge until the canonical, unmodified N=8
partition/rejoin acceptance and necessary replay/state-restore regressions pass
at a source-pinned integrated commit.

## Reproducible prior failure and contrast

The two disposable hosted-runner campaigns used exactly commit
`edf588b87cfc76bf45cf7d95c82d42a3332689b0`:

- [Canonical concentrated workload, FAIL](https://github.com/aixaria0/rchain-rust/actions/runs/39632248185):
  baseline N=8 passes, but partition cycle 1 only advances finalized height
  57 -> 58 in 300 seconds; required >=60. The isolated validator remained at
  accepted-block height 60; one signer reached 104, six others clustered
  around 90 (one near 100). Finalizer logged support 100 of 800 in a
  one-sender live partition.
- [Rotating online workload, PASS](https://github.com/aixaria0/rchain-rust/actions/runs/39632255754):
  with the same source, total bonds, two partition/rejoin cycles and
  threshold, the partitioned phase advances 57 -> 60 and 60 -> 66.
  Signer-tip heights remained close together: around 67-68 and 81-82.

A successful rotating *diagnostic* is not equivalent to canonical acceptance.
Neither run isolates every confounder, but their difference is a falsifier
for any claim that simply having seven online nodes guarantees a fresh
seven-sender partition.

## Specific correction hypothesis

The existing window is five **block heights**. Measuring staleness against
the maximum height seen in justifications allows one deployment-heavy sender
to advance height well beyond six still-online validators, making 100/800
appear live even when seven bonded signers have produced accepted blocks.

The patch introduces `quorum_supported_height`: the greatest height at
which the most recent message heights of **strictly more than 2/3 of all
bonded stake** have reached or exceeded that height. When no such level
exists, it returns zero. It is deterministic from bonds and accepted
justification senders, independent of clocks or observed-only denominators.

It uses this height only for:
- deciding which justification senders are live at the finalizer gate;
- retiring quiet senders and numbering rounds in the proposer DAG snapshot;
- measuring moving stake in the proposer's attestation licence.

The actual maximum block height remains the input to the timestamp-free
per-proposer cadence (used to catch lagging senders up), deploy-age guard,
and the consensus-defined next block number. The strict full-bonded
supermajority denominator is **not** changed anywhere.

## Non-negotiable acceptance

1. `cargo test --locked -p rchain-block-storage --lib` and relevant
   Casper proposer/finalization tests: PASS with new adversarial cases.
2. Build the exact rnode image from the pinned source; run *canonical*
   `final-gap-recovery / n8` without modifying workload, topology, gates or
   300-second window. Require both partition/rejoin cycles +3 finalized heights.
3. Compare finality continuity, block hashes and marker preservation,
   including persistence on the original data volumes.
4. Validate restoring from the same accepted DAG, signature/parent validity,
   and eventual rejoin; review the amended deterministic protocol behaviour
   (including the Scala/Lean correspondence).
5. A negative canonical result is a negative result. Do not relabel as PASS,
   reduce stake denominator, raise timeout, or substitute rotating mode.

This is a candidate explanation and implementation, **not** a safety theorem,
a completed release gate or evidence that the defect is closed.

## Review disposition: #407 at `940165cd`

The original #407 head is **negative acceptance evidence**, not a passing
standalone N=8 implementation:

- [Run 39639570219](https://github.com/aixaria0/rchain-rust/actions/runs/39639570219),
  source `b9324947f`: canonical `final-gap-recovery` **FAIL**.
- [Run 39640491759](https://github.com/aixaria0/rchain-rust/actions/runs/39640491759),
  source `940165cd0`: canonical `final-gap-recovery` **FAIL**,
  partition finality 57 -> 59 against the required >= 60 in 300 seconds.
- The review at `940165cd0` also identified two failing full-Casper tests:
  `the_chain_the_nodes_own_proposer_builds_reaches_a_fringe` and
  `a_second_proposal_in_a_round_keeps_its_sequence`.
  A passing block-storage-only regression does not override these failures.

The later sender-local boundary correction makes round closure depend on each
sender's sequence relative to that sender's boundary message, not solely on a
scalar quorum height. This corrects the old-message reuse condition. A separate
regression pins the case in which fewer than a strict supermajority have spoken:
the boundary stays frozen, then progresses when sufficient bonded senders return.
These tests do not, by themselves, establish network-level finality or replay safety.

## Integrated acceptance, distinct from this standalone proposal

[#408](https://github.com/rchain-community/rchain-rust/pull/408) is the
**combined N=8 integration candidate**, containing #407 plus changes to
block-hash admission, transport retry, and missing-parent recovery. Its source
tree `187211e43a14d06a6be2434ede93b7023008292a` was tested twice:

- Source `818cbbe3818e`: [full Rust CI PASS](https://github.com/aixaria0/rchain-rust/actions/runs/39665533963);
  [canonical N=8 PASS](https://github.com/aixaria0/rchain-rust/actions/runs/39665533457).
  Online finality: cycle 1, 57 -> 60 in 135.31 s; cycle 2, 60 -> 81 in 42.16 s.
- Source `55842ad76c10` (identical source tree):
  [full Rust CI PASS](https://github.com/aixaria0/rchain-rust/actions/runs/39669077973);
  [canonical N=8 PASS](https://github.com/aixaria0/rchain-rust/actions/runs/39669077968).
  Online finality: cycle 1, 56 -> 59 in 106.37 s; cycle 2, 59 -> 62 in 33.02 s.

In all four partition/rejoin cycles, the seven online validators advanced at
least three finalized heights within 300 s, and all eight converged on identical
finalized height/hash after the isolated validator rejoined. The original
persistent volume, markers and snapshots were retained. Both strict checkers
reported `passed=true`, `errors=[]`, `rounds_checked=2`.

These are **integration results**, not evidence that standalone #407 passed.
They establish bounded acceptance under the recorded workload, not a formal CBC
safety proof, network-independent liveness, or production readiness.

## Activation and integration boundary — blocking

The quorum-supported progress and round-boundary rules alter deterministic
consensus/replay behavior. **Do not deploy them as an ungated update against
an existing historical chain.** The proposed rollout is the next
**new-genesis fork**, with all validators aligned on the same rules from
genesis; this is a release-engineering decision requiring maintainer approval.
If existing-chain height activation is chosen instead, first implement and
verify a deterministic activation boundary using the schedule from #373
(including proposer, validator and restored-state/replay behavior).

Until the release owner confirms a fork/activation decision and the related
replay/state-restore compatibility is verified, this remains **Draft,
merge-blocked, and not deployable**.

Integration tracks: #407 is the quorum-clock prerequisite; **#408 is the
canonical combined N=8 acceptance head**. #403 addresses the separate C336
bounded proposer-attestation policy and did not itself pass canonical N=8;
do not merge competing proposer changes without explicit compatibility review.
