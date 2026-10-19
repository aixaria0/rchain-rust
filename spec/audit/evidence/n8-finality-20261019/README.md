# N=8 finality and bootstrap recovery: incident and contribution record

Report date: 19 October 2026. Prepared by Aria Fani (@aixaria0).

## Result and exact scope

The ordinary patched-engine N=4/N=8 acceptance campaign passed. Two later N=8 campaigns at `1daa9474144ea6240fb84103fcddd6e45c0865ca` also passed with one validator paused and signed state-bearing work carrying the chain across two further finalized epoch boundaries.

The no-helper N=8 legacy gate remains red on that same source. It finalizes the marker but does not cross its future-height requirement. This record preserves that failure. It does not claim universal liveness, a completed public-network recovery, or merge approval.

## Latest-source result count

At `1daa9474144ea6240fb84103fcddd6e45c0865ca`, there are **three successful workflow runs**, comprising **two live N=8 acceptance runs and one ordinary CI run**:

| Successful run | Workflow title | Actual test / scope |
| --- | --- | --- |
| [39183133040](https://github.com/aixaria0/rchain-rust/actions/runs/39183133040) | devnet fuzz (nightly) | `FUZZ_MODE=acceptance-epoch-work`; N=8, one silent validator; finalized 10 → 34. |
| [39184369430](https://github.com/aixaria0/rchain-rust/actions/runs/39184369430) | devnet fuzz (nightly) | `FUZZ_MODE=acceptance-epoch-work`; independent repeat; finalized 11 → 32. |
| [39184592817](https://github.com/aixaria0/rchain-rust/actions/runs/39184592817) | CI | All three jobs passed: Tests and the node's static gates; Dependency audit; Formatting and lints (including Clippy and benchmark-target compilation). |

The workflow title alone does not identify its executed mode: the two successful runs named “devnet fuzz” above executed the epoch-work acceptance mode, not an inferred random-input fuzz campaign. The three jobs within CI are components of one successful workflow run, not three additional workflow runs.

A **fourth** same-source workflow run, [39184423834](https://github.com/aixaria0/rchain-rust/actions/runs/39184423834), used `FUZZ_MODE=acceptance-quiet` and failed the N=8 no-helper future-epoch requirement while its N=4 arm passed. It is retained separately below.

## Source lineage and responsibility

Commit authorship identifies the origin of code; it does not by itself establish that one commit caused every observed failure.

| Source | Author / date (UTC) | What the diff establishes | What it does not establish |
| --- | --- | --- | --- |
| [77e7a2a46154](https://github.com/rchain-community/rchain-rust/commit/77e7a2a46154708bc32beba8bb84732674ae4ff8) | Patrick Mockridge, 18 August 2026 | The LFS port introduces `broadcast_request_for_block(h, Some(1))`: the request scope is one peer. | No controlled historical bisect has established this commit alone as the cause of all later N=8 failures. |
| [2403b33091c5](https://github.com/rchain-community/rchain-rust/commit/2403b33091c521def723c1cb82bd1bbd89ff7e5f) | Patrick Mockridge, 29 September 2026 | Adds termination after three consecutive idle resend rounds without a completed block. | This is a deliberate fail-rather-than-hang safeguard. It exposes a stalled walk; it is not evidence that the safeguard itself created missing delivery. |
| [6c37a23f5ed0](https://github.com/rchain-community/rchain-rust/commit/6c37a23f5ed0adc8af968a32e19142fe668d2bb0) | Patrick Mockridge, 30 September 2026 | Introduces attempt-count authorization for round escape. | This original change is not the full later timer history. Attribution of the later early-escape predicate must include the actual pre-fix code. |
| [2a0e902f34f1](https://github.com/rchain-community/rchain-rust/commit/2a0e902f34f1c3d9eb054efcf3a2931f8a1c9724), [upstream #379](https://github.com/rchain-community/rchain-rust/pull/379) | Aria Fani | Removes attempt count as an independent escape licence; elapsed blocked time must reach 15 seconds. | Does not alone resolve the subsequent silent-validator quiet-chain failures. |
| [1daa9474144e](https://github.com/aixaria0/rchain-rust/commit/1daa9474144ea6240fb84103fcddd6e45c0865ca) | Aria Fani, 19 October 2026 | Targets initial LFS block requests to bootstrap, widens retries to connected peers, retries a missing initial fringe, and preserves the bootstrap sync trigger against foreign or empty replies. | The passing combined campaign cannot attribute its entire improvement to any single changed line. |

The previously mentioned `a9cbbde9f5ce189cf242b1ecae0ea32a846e3b10` concerns parser guards. It is excluded from the sync-origin allegation. The October sync-anchor and send-observability changes are related history, not proven introductions of the one-peer line.

## Mechanisms separated during the investigation

1. **Premature escape.** The pre-fix predicate independently allowed escape after enough blocked attempts. #379 records 48 real N=8 escapes at 316–3,388 ms against a 15,000 ms threshold. A burst of attempts is not elapsed stall time. The correction retains attempt diagnostics and the elapsed boundary without reducing quorum.
2. **Bootstrap availability and trigger handling.** During startup, connected peers may still lack genesis. A one-peer request can therefore miss a block that bootstrap already has. The latest correction asks bootstrap first, uses wider retry when needed, and keeps the first-fringe request retryable. Bootstrap identity is checked by node ID. A foreign fringe returns before changing the sync target.
3. **Transport and proposal scheduling.** [Fork #9](https://github.com/aixaria0/rchain-rust/pull/9) records bounded multi-source retrieval, preserving a versioned recovery request through a busy proposer, and serializing the completion handoff. These are separately reviewable corrections, not grounds to change finalizer or parent rules.
4. **Acceptance scope.** Quiet retention and future epoch progression measure different behavior. Once state-bearing work is finalized, the idle guard can legitimately stop new proposals. The signed-work arm explicitly supplies new work. Its pass must never be relabelled as a no-traffic pass.

Private-mesh and fresh-parent experiments were investigated separately. Their results did not establish a complete repair and are not recommendations for production adoption.

## Executed evidence

| Campaign | Exact source | Observed verdict |
| --- | --- | --- |
| [39101276768](https://github.com/aixaria0/rchain-rust/actions/runs/39101276768) | `8bca39bec92ca9d8d6535b1dd80962375ea5fec7` | Ordinary N=4 and N=8 PASS; marker finalized at height 2 and retained through heights 27 and 24 respectively, as recorded in #379. |
| [39183133040](https://github.com/aixaria0/rchain-rust/actions/runs/39183133040) | `1daa9474144ea6240fb84103fcddd6e45c0865ca` | N=8 PASS: seven active validators, marker finalized at 10, retained at 34, target 30; 33 signed epoch-work deploys. |
| [39184369430](https://github.com/aixaria0/rchain-rust/actions/runs/39184369430) | Same source | N=8 PASS again: marker finalized at 11, retained at 32, target 30; 33 signed epoch-work deploys. |
| [39184423834](https://github.com/aixaria0/rchain-rust/actions/runs/39184423834) | Same source | Legacy no-helper N=8 FAIL: marker finalized at 10, but two further epoch boundaries were not reached. N=4 PASS under its quiet-retention baseline. |
| [39184592817](https://github.com/aixaria0/rchain-rust/actions/runs/39184592817) | Same source | Ordinary CI completed successfully. This status is separate from the live campaign verdicts. |

The three latest live verdicts above were read from the original job logs, including `QUIET_STALL_RESULT` and `ACCEPTANCE_OUTCOMES`, rather than inferred from a green badge.

First signed-work pass, agreed by all seven active validators:

- Marker block: `f273681bead9e931079ee14c8115171001d606a6af6050ca23ad9b56b73b42e5`.
- Marker post-state: `d312965c1c65feb780ac200fd397092a33bf1534b055d9d839b60f8b0c95e16f`.
- Retained finalized block at 34: `b6d4774675331d8d6c61321c456ece69f964edbbb9a7ebb4cb08b3d0196da968`.
- Retained post-state: `efe922138730f8527a600a75abf16b3b273070b9cdbd157ccdc948d136d0d914`.

The development record reports 30 targeted LFS/communication/sync tests for the latest correction. They were not rerun for this documentation-only report.

## Comparison with the repository's assurance claims

The [README at the upstream base](https://github.com/rchain-community/rchain-rust/blob/7b63cc29ce1a5d293fd875a93b46ddb7a307aefd/README.md) presents repository provenance, memory-safety guarantees, formal conformance, repeated adversarial review and a closed audit checklist.

These claims have distinct scopes:

| Assurance claim | What this incident requires alongside it |
| --- | --- |
| Repository provenance and an archived snapshot | Preserve later discovery, reproducer, repair and verification authorship too. A historical snapshot cannot describe all later contributions. |
| Memory safety and machine-checked laws | State the boundary between those properties and delayed-network delivery, proposer scheduling, startup recovery and eventual finality. A liveness failure does not itself disprove a memory-safety guarantee. |
| Audit completion | A closed checklist describes its reviewed scope. New live counterexamples must be recorded with their exact source and gate, even while older findings remain closed. |
| Tests and committed evidence | Keep ordinary CI, all-validator acceptance, silent-validator retention and signed-work durability as separate verdicts. Publish failed controls beside passing runs. |

This comparison does not allege fund loss, a chain split, remote execution, or deliberate misconduct. None is established by these runs. It establishes that the existing assurance apparatus did not prevent these operational defects, and that Aria's diagnosis, corrections and live verification are part of the engineering record.

## Contribution and review record

Aria Fani performed the investigation, produced and maintained the N=4/N=8 diagnostic and acceptance branches, implemented #379 and the subsequent recovery corrections, and carried the work through the two signed-work acceptance passes above. The original PRs and commits remain the authoritative attribution trail.

Aria reports approximately **16 hours** spent on this investigation and repair effort. This is contributor-reported effort, not an independently audited timesheet. Its technical value includes reproducing failures, distinguishing causes, retaining negative controls, implementing fixes, and validating the exact tested source.

Review should preserve that contribution in the destination change and any release or incident summary. Discovery, implementation and verification credit should travel with the work when it is incorporated elsewhere.

## Remaining review boundary

The legacy no-helper two-epoch gate is still red. Delayed/reordered delivery, paused-validator rejoin and broader rollback safety are not proved by these finite campaigns. The earlier fresh-parent experiment remains experimental. The bootstrap identity check and wider retry deserve their own security and resource-bound review.

This report changes documentation only. It introduces no new audit identifier, modifies no finding status, authorizes no merge and performs no public-network restart. Existing findings and test records remain their own sources of truth.
