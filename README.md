# Aria Fani — My RChain Work

**AixAria · [@aixaria0](https://github.com/aixaria0) · Systems Architect & RChain Contributor**

I started this work on **15 September 2026**. This fork is my record of what I have researched, built, tested, reviewed and contributed to the RChain ecosystem.

My work connects Casper CBC research, executable evidence, Rust node fixes, recovery and observability, and the application layer around Rholang and RSpace. I follow a problem from the first reproducible failure through its implementation boundary, regression test and network acceptance. A result that remains unresolved stays visible here.

**[Complete contribution ledger](ARIA_CONTRIBUTIONS.md)** · **[My upstream PRs](https://github.com/rchain-community/rchain-rust/pulls?q=is%3Apr+author%3Aaixaria0)** · **[Rholox](https://github.com/aixaria0/Rholox)** · **[Observer](https://github.com/rchain-community/rchain-observer)**

_Last checked: 18 October 2026. This page records my contributions; shared upstream implementation and existing formal specifications retain their own provenance. Authored patches, reviews, research candidates and deployed application work are identified separately._

## The path so far

| Period | What I built or investigated | Evidence |
| --- | --- | --- |
| 15–16 September | Sentinel observation, finalized-block evidence, cross-node checks and the verification dashboard; RLSenti's execution/provenance workbench. | [Sentinel's first commit](https://github.com/aixaria0/rchain-sentinel/commit/e7283846ab6ed11f511bdc020a582a40ef2e21ed), [RLSenti](https://github.com/aixaria0/rlsenti) |
| 17–21 September | Reality Evidence Plane foundations and deterministic Casper CBC stress, replay and pinned Rust integration. | [Compiler #9](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/9), [#14](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/14), [#16](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/16) |
| 22–25 September | Exact DAG replay across four Rust Finalizers, a downstream Lean conformance gate, native PoS persistence probes and repeated two-validator finality runs. | [Compiler #17](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/17), [#18](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/18), [#19](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/19) |
| Late September–early October | Protocol-neutral verification, signed evidence and deterministic replay, a real three-process recovery lifecycle, and paired C192/C171 experiments. | [Compiler #21](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/21), [#23–27](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/27), [#28–38](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/38) |
| 28 September–18 October | Native-state and merge correctness, queue observability, faucet reliability, reconciliation, seed error handling, devnet readiness, reset delivery and release-boundary work. | [My Rust PR record](#my-rust-node-contributions) |
| 10–15 October | Rholox's wallet, proof, explorer, testnet asset/exchange interfaces; multi-validator Observer and raw-block ancestry verification; cross-repository REV-transfer assurance work. | [Rholox](https://github.com/aixaria0/Rholox), [Observer](https://github.com/rchain-community/rchain-observer), [Compiler #42](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/42) |
| 18 October | Elapsed-only round escape, two successful all-live N=4/N=8 finality/retention campaigns, and a separate silent-validator failure witness. | [Rust #379](https://github.com/rchain-community/rchain-rust/pull/379), [quiet-chain run](https://github.com/aixaria0/rchain-rust/actions/runs/39135722717) |

## Casper CBC research: from controlled scenarios to executable evidence

### Deterministic stress and pinned upstream execution

I built a reproducible Casper CBC stress/evidence harness around partition, delayed-delivery, reordering and equivocation scenarios, with replay digests and machine-readable observations. The bridge into pinned upstream Rust reached the Finalizer and validation/ingress paths rather than stopping at a synthetic simulation.

The minimum-message investigation separated message count from distinct bonded-sender coverage. I also corrected the interpretation of the original M27 tuple: `a3,a3,c3,d3` has three unique message identities after Rust set deduplication, so it cannot be presented as a four-distinct-message Rust witness.

The separate finite model census found **105 four-distinct candidates**, including **81 with sender undercoverage** and **43 that were both undercovered and model-finalizing**. Those are model results. The source-level Rust witness has its own evidence.

Sources: [Compiler #14](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/14), [#16](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/16), [#17](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/17).

### Four Rust Finalizers and a Lean conformance checkpoint

I replayed the same content-hashed, sixteen-message DAG through four pinned Rust implementations. Each observed **70/100 supporting stake**, an initial fringe predicate of `true`, and new fringe `a1,b1,c1`.

I then built a downstream gate against the existing community `Rchain.Casper.Stake` specification: twelve concrete equalities, empty axiom audits and two intentionally corrupted controls that proof checking rejected. This checks a finite proof/code correspondence; it does not claim that I authored the upstream Casper specification or proved universal protocol safety.

Source: [Compiler #17 and its retained runs](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/17).

### Native PoS persistence and ordinary-network finality

I investigated staged-withdrawal and reward-claim persistence through source-pinned probes, negative controls and temporary repair candidates. The follow-up included native checkpoint/restore and actual Casper play/replay checks.

Separately, I produced two ordinary two-validator runs that exchanged six blocks, reached tip height 7 and agreed on finalized block 2. The research record distinguishes native-state and replay results from network-finalized slashing, which it did not establish.

Sources: [Compiler #18](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/18), [#19](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/19).

## My Rust node contributions

The upstream author record currently contains **21 PRs: 13 merged, 3 open and 5 closed without merge**. Closed research or patch proposals remain part of the record; they are not counted as merged code.

| PR | My change | Status on 18 October 2026 |
| --- | --- | --- |
| [#96](https://github.com/rchain-community/rchain-rust/pull/96) | Canonical native writes: collapse identical actions and reject conflicting actions. | Closed, unmerged |
| [#97](https://github.com/rchain-community/rchain-rust/pull/97) | Exact bonded-sender coverage at the minimum-message gate. | Closed, unmerged |
| [#120](https://github.com/rchain-community/rchain-rust/pull/120) | Validated-block queue depth, sampled peak and consumer activity metrics. | Merged |
| [#136](https://github.com/rchain-community/rchain-rust/pull/136) | History-chain balance queries and preservation of genesis/snapshot inputs. | Merged |
| [#140](https://github.com/rchain-community/rchain-rust/pull/140) | Claimed/computed checkpoint hashes and replay-failure evidence. | Closed, unmerged |
| [#141](https://github.com/rchain-community/rchain-rust/pull/141) | Exact rejected-state merge-search quotient and bounded BlockIndex memo residency. | Merged |
| [#142](https://github.com/rchain-community/rchain-rust/pull/142) | Reusable LFS worker and re-arming of failed sync attempts. | Closed, unmerged |
| [#246](https://github.com/rchain-community/rchain-rust/pull/246) | Durable faucet reservations, finalized-delivery eligibility and remaining budget. | Merged |
| [#282](https://github.com/rchain-community/rchain-rust/pull/282) | Per-deploy native-write provenance and rejection-cascade correctness. | Merged |
| [#283](https://github.com/rchain-community/rchain-rust/pull/283) | Pre-registered soft-checkpoint instrumentation and benchmark evidence. | Merged |
| [#285](https://github.com/rchain-community/rchain-rust/pull/285) | One expiry boundary shared by validation, proposer selection and pool cleanup. | Merged |
| [#286](https://github.com/rchain-community/rchain-rust/pull/286) | Remove the unreachable whole-unit fallback snapshot in Sequential/Gate modes. | Merged |
| [#305](https://github.com/rchain-community/rchain-rust/pull/305) | Conservative finalized-anchor reconciliation and fail-closed recovery operations. | Merged |
| [#309](https://github.com/rchain-community/rchain-rust/pull/309) | Unique-block accounting, contradictory metadata refusal and finalized recovery checks. | Closed, unmerged |
| [#324](https://github.com/rchain-community/rchain-rust/pull/324) | Typed block-seed length errors instead of panics; existing v1 bytes preserved. | Merged |
| [#326](https://github.com/rchain-community/rchain-rust/pull/326) | Application guidance separating submission, inclusion and finalized read-back. | Merged |
| [#336](https://github.com/rchain-community/rchain-rust/pull/336) | Wait for every devnet validator/observer HTTP listener before starting fuzz. | Merged |
| [#340](https://github.com/rchain-community/rchain-rust/pull/340) | Deliver pending reset requests when installing or replacing the driver waker. | Merged |
| [#341](https://github.com/rchain-community/rchain-rust/pull/341) | Real N=4/N=8 finalized-write, retention and causal-trace acceptance harness. | Open draft |
| [#376](https://github.com/rchain-community/rchain-rust/pull/376) | Enforce committed PoS bounds and harden activation, wire, restore and fundPot boundaries. | Open draft |
| [#379](https://github.com/rchain-community/rchain-rust/pull/379) | Require elapsed blocking time before round escape; remove attempt-count bypass. | Open |

### What the latest acceptance actually establishes

The #379 fix removes the independent attempt-count escape condition. Escape requires at least fifteen seconds of elapsed blocking; the existing retry remains.

I ran the fix with the test-only #341 harness on a real disposable Docker network:

| Campaign | N=4 | N=8 |
| --- | --- | --- |
| [First acceptance](https://github.com/aixaria0/rchain-rust/actions/runs/39101276768) | Marker finalized at height 2; retained through height 27. | Marker finalized at height 2; retained through height 24. |
| [Repeated acceptance](https://github.com/aixaria0/rchain-rust/actions/runs/39127847482) | Marker finalized at height 2; retained through height 25. | Marker finalized at height 2; retained through height 24. |

The original finalized snapshots also retained the expected marker value. These are bounded execution witnesses.

I then added a separate [silent-validator drill](https://github.com/aixaria0/rchain-rust/tree/test/n379-quiet-stall-gate): pause one validator before the marker deploy, disable autopropose and supply no helper deploys. **Both N=4 and N=8 failed to finalize within the 240-second window.** All 51 recorded escapes respected the fifteen-second bound.

Analysis of the checksum-verified active-node logs found matching re-advertised hash sets, but delayed delivery: up to about 35.6 seconds in N=4 and 140 seconds in N=8. The fifteen-second age filter on automatic attestation is a concrete investigation lead. Exact block timestamps and filter decisions were not captured, so the root cause remains unproven.

Evidence: [quiet-chain run 39135722717](https://github.com/aixaria0/rchain-rust/actions/runs/39135722717). This failure remains separate from the two successful all-live campaigns.

## The evidence and application systems I built

| Project | My work | Current boundary |
| --- | --- | --- |
| [RChain Sentinel](https://github.com/aixaria0/rchain-sentinel) | Rust observation/evidence APIs, finalized-block payload fingerprints, cross-node comparison, Casper-shaped evidence inspection and the verification dashboard. | Node observations and node-count agreement are not independent stake-weighted Casper finality. The public showcase is labeled synthetic. |
| [RLSenti](https://github.com/aixaria0/rlsenti) | Deterministic event → execution → block → observation → verification workbench; provenance envelopes, semantic/reality diffs, replay, counterfactual and adversarial scenarios. | The core workbench uses synthetic fixtures. Adapters and branch experiments retain their own status. |
| [RCHAIN-COMPLIER / Assurance Engine](https://github.com/aixaria0/RCHAIN-COMPLIER) | Pinned research workflows; verification-core adapters; signed event journals; TaskEnvelope provenance; independent reproduction receipts; real three-process recovery/replay; packaged SDK/CLI and short-lived signed process-health attestations. | Local process separation is not organizational independence. Evidence integrity does not independently authenticate an upstream execution claim. |
| [Rholox](https://github.com/aixaria0/Rholox) | Evidence-first browser interface: network console, session wallet, test REV, proof anchoring, block/deploy explorer, finalized transfer read-back, Asset Lab, testnet exchange custody checks, typography and degraded-state handling. | Public-testnet application work. Submission, execution, inclusion and finality remain distinct; exchange work is not a production-custody assurance claim. |
| [rchain-observer](https://github.com/rchain-community/rchain-observer) | Created the read-only multi-validator observation core, cached public evidence API, private-topology boundary, ancestry classification and cryptographic verification of raw block preimages/signatures. | Authenticated ancestry supports bounded comparison of observed snapshots; it is not a new consensus implementation. |
| [REV-transfer assurance](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/42) | Connected Compiler verification, [Sentinel observations](https://github.com/aixaria0/rchain-sentinel/pull/8) and [Rholox presentation](https://github.com/aixaria0/Rholox/pull/15) around exact task/claim/network identity, independent payload pins and finalized read-back. | These PRs remain open. The recorded live assurance result is BLOCKED; matching local digests cannot promote an untrusted package to independently pinned evidence. |

### Assurance Engine and C192/C171 experiments

I developed the assurance work from signed provenance and deterministic replay into a real coordinator/two-worker lifecycle with persistent identities, SQLite WAL, recovery after process termination and independently checked evidence packages. The later product layer exposes typed SDK and CLI interfaces and an installable package.

The C192/C171 integration work progressed through a pinned upstream witness, paired model, disposable Rust tests, actual production-path wiring, controlled devnet smoke and repeated campaigns. I kept negative controls and null-effect results: the cadence experiments were not promoted into a claim that both liveness and block-rate behavior had been solved.

Sources: [Compiler #21](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/21), [#23–27 lifecycle series](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/27), [#28](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/28), [#30–34 experiments](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/34), [#38 diagnostics](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/38), [#39 productization](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/39), [#40 liveness control plane](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/40).

## My review work

Review is part of my contribution record, with a different attribution from implementation. The complete ledger links the ten upstream PRs with a formal review from my account.

Examples with concrete findings:

- [Native merge review, #281](https://github.com/rchain-community/rchain-rust/pull/281#pullrequestreview-5463647197): identified the rejection-invariant/dependency mismatch and loss of an earlier same-block writer. My implementation follow-up is #282; the original hard-fork implementation is not attributed to me.
- [Recovery/catch-up review, #312](https://github.com/rchain-community/rchain-rust/pull/312#pullrequestreview-5480318026): documented shell interpolation, contradictory block-metadata handling, response-peer/range correlation and exact-head verification gaps.
- [Developer-allocation/genesis review, #371](https://github.com/rchain-community/rchain-rust/pull/371#pullrequestreview-5548408280): reproduced duplicate-holder accounting, examined the LFS restore boundary and corrected the severity when a later replay rejection path was identified. The calculation was also checked across 50,000 deterministic differential cases.

These links establish my review activity and findings; they do not transfer authorship of the reviewed PRs.

## Reading this fork

This is my contribution-facing fork of [the RChain Rust node](https://github.com/rchain-community/rchain-rust). The original project overview is preserved at [the README before this personal-page update](https://github.com/aixaria0/rchain-rust/blob/586ef01c537ce79699b89bb45b7196748bdd77ce/README.md).

- [Complete authored-PR and review ledger](ARIA_CONTRIBUTIONS.md)
- [Node operating guide](docs/src/node/operating.md)
- [Language documentation](docs/src/rholang/)
- [Upstream project overview](https://github.com/rchain-community/rchain-rust)
- [Existing license](LICENSE)

The inherited node, shared mathematical foundations and work by other contributors remain upstream work. This page is the record of my additions, experiments and reviews, with their current evidence boundaries.
