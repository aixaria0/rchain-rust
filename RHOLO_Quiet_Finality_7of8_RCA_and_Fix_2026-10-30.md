# Quiet finality 7/8 — investigation and candidate correction

Date: 2026-10-30. Fork-only isolated verification. Current decision: **NO-GO pending mandatory acceptance evidence**.

## Reproduced failure

Original source: 53256c5682b8d4b0e63444771d17d06bd502de7c. [Original failed run](https://github.com/aixaria0/rchain-rust/actions/runs/40980671027), [artifact](https://github.com/aixaria0/rchain-rust/actions/runs/40980671027/artifacts/12945405729).

Eight equal bonded stakes total 800. The 8/8 quiet arm passed. After isolating validator 7, the seven survivors stopped at finalized height 10, below required height 11, with supporting stake 500/800. The original finalized marker remained absent at the last observation; every surviving validator failed the healthy gate after the proposal safety latch engaged. The proposal lag bound remained 32.

The verified recovery baseline 20ef2505ffed761cdf1d2b4330f1ec1f326f0eec and quiet harness were integrated without conflict at 05ef306a5ce453a6d94a42f03157322475c0a4f7. [Integrated unchanged-behavior control](https://github.com/aixaria0/rchain-rust/actions/runs/40986341277) also failed. This control is not acceptance of the candidate correction.

## Proven scheduling defect and remaining causal qualification

The blocked-round clock starts once. Previously, every attempt after its first 15 seconds could escape while that round remained open. Successful escapes did not consume that permission. Remote-block-triggered attempts therefore amplified production instead of respecting a fresh stall interval.

The original logs show minimum consecutive escape intervals of 164 ms on validator 2 and 176 ms on validator 6. They performed 25 and 21 escapes respectively within their first 15 seconds of escape activity. Validator 2 latched at 00:43:05.098Z and validator 6 at 00:43:06.581Z, each refusing height 42 against finalized height 9. Other survivors subsequently advanced their supporting layers, but only five full partitions were credited. The finalizer correctly refused 500/800: six equal bonded supporters are required.

The scheduling defect and the two early latches are established by source and archived logs. The original artifact does not contain the complete causal DAG, so exact sender-by-sender support-map attribution for its final 500/800 observation is not established by those logs alone. A separate [causal control](https://github.com/aixaria0/rchain-rust/actions/runs/40987234808) archives DAG summaries and hash preimages for replay. Do not treat a plausible attribution as a proven protocol defect.

## Candidate code changes

Candidate source: b26bda1a1aa8a1adac9fc9146a712b9d9c8dc3a9.

- The proposer consumes one stalled-round escape permission and starts a fresh monotonic interval before permitting another escape.
- Regression cases exercise duplicate ticks, bursts, delayed attempts, unused-permission banking and round reset.
- The finalizer regression requires six distinct complete bonded supporters out of eight; five supporters, missing seers and unbonded substitution remain rejected. Finalizer decision logic is unchanged.
- Harness controls reject conflicting finalized hashes, missing validators, missing markers, unhealthy observations, OOM counters and stalled finalized height.
- Quiet acceptance additionally requires rejoin and retention of both signed markers at an exactly agreed finalized hash.
- The combined workflow builds one pinned binary and supplies that same image to two repetitions of both recovery and quiet-finality acceptance. Rust regressions precede these acceptance jobs.

## Safety reasoning

No quorum threshold, bonded stake, genesis parameter, liveness window, readiness requirement or finality-lag limit is weakened. The correction changes local proposal scheduling; finality remains deterministic from block/DAG inputs. A counterfactual regression deliberately removes permission consumption inside a disposable test container and must fail the scheduling test. This is explicitly negative test evidence, not a candidate acceptance run.

## Regression and before/after evidence

[Candidate verification](https://github.com/aixaria0/rchain-rust/actions/runs/40987327326) is the source of authoritative candidate results. At document preparation, final gates and complete causal replay remain pending. The six offline quiet harness controls passed locally. Compilation or successful creation of a commit does not establish finality recovery.

The complete acceptance matrix, binary hash and remaining blockers belong in [the readiness handoff](RHOLO_Testnet_Readiness_Handoff_2026-10-30.md). No live action or PR merge is authorized by this document.
