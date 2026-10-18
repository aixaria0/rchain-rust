# #379 quiet-chain / silent-validator evidence (2026-10-18)

Evidence run: https://github.com/aixaria0/rchain-rust/actions/runs/39135722717
Exact tested source: `eb03fbd9468212639487f0c4eb5028b8c38f765b`
Production consensus correction: `2a0e902f34f1c3d9eb054efcf3a2931f8a1c9724` (upstream PR #379).
Harness: `test/n379-quiet-stall-gate` (fork, diagnostic only).

## Finite verdict

Two isolated Docker campaigns, each with one validator paused *before* a single signed marker deploy.
Autopropose was off, propose-on-deploy was on, no helper/Nil deploys were submitted,
and each active-validator group held strictly more than two-thirds of total bonded stake.
Both N=4 (3 live of 4) and N=8 (7 live of 8) **failed to finalize the marker in 240 seconds**.
This is a liveness counterexample for this exact quiet campaign, not a universal claim.

The elapsed escape repair did hold: 17 recorded N=4 escapes and 34 N=8 escapes,
all at or above 15,000 ms; smallest observed is 15,538 ms.
A correct escape predicate is **not** sufficient to establish quiet-chain finality.

At final observation:
- N=4: active nodes report next height 9, `finalityStall` says 0/400 supporting
  bonded weight for a 3-sender partition; undelivered-send counters are 7, 9, 10.
- N=8: active nodes report next height 8, `finalityStall` says 0/800 supporting
  bonded weight for a 7-sender partition; undelivered-send counters range 5–9.
- Self-validation failure count and panicked task count were zero on the active nodes.
- The last-finalized-block endpoint returned HTTP 400 during the observation window.
  This is absence of a finalized record, not a transport or API crash verdict.

## What is **not** established

The 15-second age check in `node/src/runtime/node_runtime.rs::attest_is_live` can reject
late remote blocks, and a previously isolated timing witness showed inputs outside
that window. Neither the log counts nor the final operator snapshots alone establish
that the age filter caused the observed failure. There is also nonzero undelivered-send
evidence, and the proposer logs show licensed attestations in portions of the run.
Do not change the quorum, disable the replay-age guard, or classify network topology
from the peer count without stronger causal evidence.

## Next discriminating gate

Capture the same remote block hash at each stage: sender timestamp, hash broadcast,
receiver arrival, retriever/admission outcome, `attest_is_live` age, per-sender
`attest_warranted` decision, enqueue success/failure, proposer decision and
candidate-fringe supporting stake. Use monotonic observation timing where possible;
record wall-clock skew separately. An event cannot be treated as absent solely
because it does not appear in a log. Compare the quiet run with a no-paused-validator
control using the same source and exactly one deploy.

PASS requires finalized identical marker values on all **active** validators within
a declared bound and retention in their current and original finalized snapshots,
with no sub-15-second escapes. FAIL if marker finalization is missing, an escape is
premature, or retention differs. Inconclusive when the necessary decision/propagation
records are absent. Follow separately with delay/reorder and paused-validator rejoin
campaigns. None of these are proved by CI green, or by the separate ordinary N=8 PASS.

This evidence is not merge authorization. No public network was changed.
