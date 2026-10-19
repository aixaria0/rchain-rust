# Contribution provenance, authorship and allocation-review evidence

Prepared by Aria Fani (aixaria0). Public metadata rechecked on 19 October 2026.
This ledger accompanies the [N=8 incident report](README.md) in the same PR.

## Meaning and scope

Contribution provenance traces a finding or requirement through implementation, integration and verification. Different people may perform each stage. A final PR author, assignee or squash author is therefore an incomplete record of the work.

This document supports recognition and any separately approved allocation review. It records no agreed payment, token percentage, recipient address or entitlement; it changes no genesis allocation. The developer-allocation mechanism in [#371](https://github.com/rchain-community/rchain-rust/pull/371) is separate: its destination remains to be confirmed and it does not divide contributor rewards using GitHub authorship or assignment fields.

| Role / record | What it establishes | What it does not establish |
| --- | --- | --- |
| Discovery / diagnosis | A published failure mechanism and its evidence. | Authorship of every later repair. |
| Requirements / review | The source of a missing invariant, objection or acceptance condition. | Authorship of every resulting line. |
| Git author / co-author | Authorship recorded on a specific revision. | Sole credit for discovery, integration or verification. |
| Git committer / merge actor | Who recorded or incorporated the revision. | Sole authorship of incorporated work. |
| PR author | Who opened a submission or integration proposal. | Sole authorship of everything it contains. |
| Assignee | Recorded responsibility for tracking or handling work. | Transfer of authorship, financial rights or an allocation percentage. |
| Verification | A reproducer, control or executed result pinned to a tested source. | Universal correctness or verification of a later untested revision. |
| Allocation approval | A separate explicit decision naming authority, scope, recipient and terms. | A decision inferred from a merge, assignment, check badge or PR count. |

Git author fields and commit dates are evidence, not independent identity signatures. Server-recorded publication times and explicit transfer acknowledgments provide additional anchors. The [dated metadata snapshot](contributions.json) keeps these fields separate.

## Principal contribution paths

| Source / destination | Aria's documented contribution | Integration and attribution |
| --- | --- | --- |
| [PoS research #19](https://github.com/aixaria0/RCHAIN-COMPLIER/pull/19) → [#78](https://github.com/rchain-community/rchain-rust/pull/78) | Published omitted pending-withdrawal persistence, the 9/9 stale-state witness, conditional repair and compute/replay checks before #78 opened. | Jim authored the production repair. #78 repeats the distinctive research context without naming Aria or linking #19. Discovery and verification credit should be named; this is not a Patrick-authored patch. |
| [#97](https://github.com/rchain-community/rchain-rust/pull/97) → [#104](https://github.com/rchain-community/rchain-rust/pull/104) | Exact bonded-sender coverage and the duplicate-sender regression. | Patrick integrated it with additional specification work. [86693daf7](https://github.com/rchain-community/rchain-rust/commit/86693daf7fd2623f0838ab537cd20b9e37f31b59) preserves Aria as co-author. |
| [#120](https://github.com/rchain-community/rchain-rust/pull/120) | Added the missing queue-depth observation that changed the OOM diagnosis. | [Patrick confirms](https://github.com/rchain-community/rchain-rust/pull/120#issuecomment-5903714498) the instrument ruled the measured queue out. It does not make the entire memory programme Aria's. |
| [#141](https://github.com/rchain-community/rchain-rust/pull/141) → [#147](https://github.com/rchain-community/rchain-rust/pull/147) | Directed rejected-set quotient and bounded block-index cache. | Patrick's later campaign, gates and qualifications build on Aria's rewrite; both roles deserve credit. |
| [#140](https://github.com/rchain-community/rchain-rust/pull/140) → [#147](https://github.com/rchain-community/rchain-rust/pull/147) | Diagnostics distinguishing rejected-deploy disagreement and replay-failure causes. | [The transfer account](https://github.com/rchain-community/rchain-rust/pull/140#issuecomment-5932514904) preserves Aria's authorship on [e8d4e691a](https://github.com/rchain-community/rchain-rust/commit/e8d4e691affd37582b87e15187f79d8c8d37cbc3); two parts were incorporated, not the whole proposal. |
| [#142](https://github.com/rchain-community/rchain-rust/pull/142) versus [#143](https://github.com/rchain-community/rchain-rust/pull/143) | Earlier public alternative for re-arming failed LFS sync. | The later landed design differs. The before-opening claim conflicts with publication times; literal copying or motive is not established. |
| [#282](https://github.com/rchain-community/rchain-rust/pull/282) → [#281](https://github.com/rchain-community/rchain-rust/pull/281) | Identified missing per-deploy write history and the I1 relation/direction problem; supplied correction and end-to-end acceptance condition. | Patrick supplied the earlier chain-level architecture and integration. Exact Aria commit [410af4cbf](https://github.com/rchain-community/rchain-rust/commit/410af4cbf320ecd3f710639bb882b75688474e11) was carried; [62c43e3b0](https://github.com/rchain-community/rchain-rust/commit/62c43e3b0c25cabda16a9d79df3e23a80e63768a) names Aria's acceptance condition. |
| [#283](https://github.com/rchain-community/rchain-rust/pull/283) / [#286](https://github.com/rchain-community/rchain-rust/pull/286) | Checkpoint-cost census and ablation with preserved controls. | Both are Aria-authored and merged. Their assignee fields differ from authorship. |
| [#305](https://github.com/rchain-community/rchain-rust/pull/305) | Fixed observer/stake double-counting and fail-closed handling; rejected unsupported quorum inference from endpoint reports. | [Patrick accepts the objection to his alternative](https://github.com/rchain-community/rchain-rust/pull/305#issuecomment-6089763209). Credit includes technical judgment and review, not only code. |
| [#309](https://github.com/rchain-community/rchain-rust/pull/309) → [#312](https://github.com/rchain-community/rchain-rust/pull/312) / [#333](https://github.com/rchain-community/rchain-rust/pull/333) | Unique-block reporting, checked recovery finality and conflicting-metadata refusal requirements. | [The closure account](https://github.com/rchain-community/rchain-rust/pull/309#issuecomment-6107904420) identifies later revised implementations. Requirement-level lineage is supported; the original commit was not transferred verbatim. |
| [#341](https://github.com/rchain-community/rchain-rust/pull/341) → [#379](https://github.com/rchain-community/rchain-rust/pull/379) / [fork #9](https://github.com/aixaria0/rchain-rust/pull/9) | N=8 reproducer, diagnostics, elapsed-only escape repair, subsequent recovery corrections and live campaigns. | Exact source responsibility, passing campaigns and the red no-helper control remain in the incident report. Upstream changes remain subject to review. |
| [#376](https://github.com/rchain-community/rchain-rust/pull/376) | Committed-bound enforcement and additional release-boundary corrections/tests. | Open and unmerged at this snapshot; proposed work is not counted as shipped. |

## Correctness gaps and accurate responsibility

- **Native-write history / #281:** a final-writer-only overlay erased an earlier accepted deploy's value when a later writer was rejected. I1 also used the wrong relation/direction to justify rejection. [Patrick acknowledged the gaps](https://github.com/rchain-community/rchain-rust/pull/281#issuecomment-6070568404) and [the applied Aria correction](https://github.com/rchain-community/rchain-rust/pull/281#issuecomment-6077742051).
- **Recovery / #305:** Patrick's earlier accounting multiplied proposer stake by observer count. Aria corrected it and showed why the suggested endpoint-weighted alternative lacked authenticated validator binding, signed votes and verified ancestry. The review acceptance is linked above.
- **N=8 escape:** Patrick's 2bbcb8de introduced the attempt-count escape; Jim's 59ee5b23 retained it alongside a timer and tested zero-time escape as acceptable. Aria's #379 removes that licence. The incident report preserves the exact authors, predicates, historical green checks and causal limits.
- **Bootstrap LFS:** the one-peer request originates in Patrick's 77e7a2a4 port. Aria's 1daa9474 targets bootstrap first and preserves retry/trigger eligibility. No historical bisect establishes that this one old commit caused every later N=8 failure.
- **Attribution / #142:** correct the published chronology independently of any unproved allegation about copying.

These findings neither assign every defect to Patrick nor erase his architecture, integration, formal work or existing acknowledgments.

## Publication chronology requiring correction

| Event | UTC, 1 October 2026 | Source |
| --- | --- | --- |
| Aria opens #142 | 04:29:13 | [#142](https://github.com/rchain-community/rchain-rust/pull/142) |
| Patrick opens #143 | 08:47:48 | [#143](https://github.com/rchain-community/rchain-rust/pull/143) |
| #143 merges | 11:20:44 | [#143](https://github.com/rchain-community/rchain-rust/pull/143) |
| Response says the named landed route preceded #142's opening | 11:49:06 | [Closure response](https://github.com/rchain-community/rchain-rust/pull/142#issuecomment-5930676087) |

The requested correction is the chronology, or a dated earlier public artifact supporting a narrower independence claim. Different designs and earlier registration of the underlying problem leave independent work possible; they do not reverse these timestamps.

## Complete reviewed submission index

This is the seventeen-submission index for 25 September–12 October, not a lifetime contribution census. Statuses were rechecked on 19 October.

| Original PR | Subject | Destination / disposition |
| --- | --- | --- |
| [#96](https://github.com/rchain-community/rchain-rust/pull/96) | Duplicate native writes | Closed; superseded by Jim's #101, not counted as a direct transfer. |
| [#97](https://github.com/rchain-community/rchain-rust/pull/97) | Exact sender coverage | Closed; incorporated through #104 with credit. |
| [#120](https://github.com/rchain-community/rchain-rust/pull/120) | Queue observation | Merged to dev. |
| [#136](https://github.com/rchain-community/rchain-rust/pull/136) | History-chain balances / preserved inputs | Merged to dev. |
| [#140](https://github.com/rchain-community/rchain-rust/pull/140) | Checkpoint diagnostics | Closed; two authored parts incorporated through #147. |
| [#141](https://github.com/rchain-community/rchain-rust/pull/141) | Directed quotient / cache bound | Merged to dev. |
| [#142](https://github.com/rchain-community/rchain-rust/pull/142) | LFS retry alternative | Closed; separate #143 route; chronology addressed above. |
| [#246](https://github.com/rchain-community/rchain-rust/pull/246) | Faucet budget / readiness | Merged to dev. |
| [#282](https://github.com/rchain-community/rchain-rust/pull/282) | Native provenance / I1 | Merged into #281's branch, then incorporated into dev. |
| [#283](https://github.com/rchain-community/rchain-rust/pull/283) | Checkpoint census | Merged to dev. |
| [#285](https://github.com/rchain-community/rchain-rust/pull/285) | Deploy expiry | Merged to dev. |
| [#286](https://github.com/rchain-community/rchain-rust/pull/286) | Checkpoint ablation | Merged to dev. |
| [#305](https://github.com/rchain-community/rchain-rust/pull/305) | Recovery accounting / errors | Merged to dev. |
| [#309](https://github.com/rchain-community/rchain-rust/pull/309) | Unique blocks / finality verification | Closed; requirements incorporated through #312/#333 with revised code. |
| [#324](https://github.com/rchain-community/rchain-rust/pull/324) | Random-seed overflow refusal | Merged to dev. |
| [#326](https://github.com/rchain-community/rchain-rust/pull/326) | Public-testnet evidence boundaries | Merged to dev. |
| [#336](https://github.com/rchain-community/rchain-rust/pull/336) | Validator HTTP readiness | Merged to dev. |

Twelve have merged status, including #282's feature-branch merge. Three additional closed submissions have explicit incorporation paths (#97, #140, #309): fifteen traceable merged/incorporated paths in this reviewed set. This is not fifteen independent discoveries, a percentage ownership of the project, or an allocation formula. #142 is not counted as proven incorporation.

## Assignment is not authorship

| PR | PR author | Recorded assignees on 19 October | Interpretation |
| --- | --- | --- | --- |
| [#283](https://github.com/rchain-community/rchain-rust/pull/283) | aixaria0 | PatrickMockridge, aixaria0 | Joint tracking; Aria's original authorship remains. |
| [#286](https://github.com/rchain-community/rchain-rust/pull/286) | aixaria0 | PatrickMockridge | Assignment does not transfer authorship of Aria's submission. |
| [#326](https://github.com/rchain-community/rchain-rust/pull/326) | aixaria0 | aixaria0 | Author and assignee coincide. |
| [#341](https://github.com/rchain-community/rchain-rust/pull/341) | aixaria0 | aixaria0 | Diagnostic responsibility is recorded; assignment proves no production repair. |

Most inspected PRs have no assignee; that is not evidence of absent work. Historical assignees have not been rewritten. Aria authors this report; assigning its PR identifies responsibility for maintaining it.

## Requested corrections and allocation-review handoff

1. Carry discovery, code, review and verification credit into the final destination, including a later integration or squash. Preserve existing co-author and exact-commit acknowledgments.
2. Name and link Aria's PoS research in #78's contribution record. Correct #142's before-opening statement or supply its supporting dated evidence.
3. Assess the distinct engineering contributions: a diagnosis, enabling invariant, unsafe alternative rejected in review or decisive negative control can matter beyond its line count.
4. Keep reported time separate from test proof. The approximately sixteen-hour incident effort includes the reported roughly ten hours of repeated testing; these are not added together.
5. For any allocation, separately record approving authority, recipient, scope, terms and approval. No award is invented here; neither integration authorship nor assignment substitutes for that decision.

These are requests documented for review. This PR has not edited historical comments, changed historical commit authors, assigned financial rights or modified #371. A separately supplied destination record will need its own source review.

## Verification boundary

Primary PR metadata, selected discussions and named commits were rechecked. The incident report supplies exact runtime evidence; this ledger adds no new network experiment, payment valuation or lifetime census. The JSON snapshot is a dated convenience copy of neutral GitHub fields, not a server signature or allocation approval.
