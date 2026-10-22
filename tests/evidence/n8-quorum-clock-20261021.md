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
