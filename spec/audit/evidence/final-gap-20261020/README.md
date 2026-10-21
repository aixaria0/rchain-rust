# Final release and consensus assurance — 2026-10-20

Release position: BLOCKED. #51 must remain open. These results support review of bounded corrections; they do not authorize a hard fork, public-network restart, or historical compatibility claim.

## Source and delivery

Upstream dev was rechecked at `bdcc6f5871aee0a05f50d559d84dd36a6b69bb5b`. #389/#390 are merged. #373/#391/#392/#393/#395 remain unmerged. Existing worktrees, dirty files, branches and prior evidence were preserved. All new distributed execution used disposable hosted-runner networks.

| Delivery | Exact source | Evidence | Acceptance boundary |
|---|---|---|---|
| [#402](https://github.com/rchain-community/rchain-rust/pull/402) initial LFS reply correction | `eae0a7993e9edc163028ee82401da24ea4e308ab`; production code introduced at `e574668b2c541baa27841c974c3aac7fa4c31bac` | [upstream CI](https://github.com/rchain-community/rchain-rust/actions/runs/39392031158), [production CI](https://github.com/rchain-community/rchain-rust/actions/runs/39388911701), [fault reintroduction](https://github.com/aixaria0/rchain-rust/actions/runs/39389941677) | Six regressions and full CI pass. No CBC/finality/stake rules changed. Draft, Jim review requested. |
| [#401](https://github.com/rchain-community/rchain-rust/pull/401) exact release inventory | `56038e6b54ba6e731e25b4cd5a833415d4297577` | [upstream CI](https://github.com/rchain-community/rchain-rust/actions/runs/39394739915) | 39 tracker obligations, all 85 upstream §6 rows, eight mutation controls plus 17 existing closeout controls. No approved named-release deferrals or independent sign-off. |
| [#399](https://github.com/rchain-community/rchain-rust/pull/399) combined acceptance | initial combined source `ba72459079e224a9b0add336b7fe30c58ddd375f`; Rust regression source `9f01ba1cb694aa50f4d715134f8f08ad69731679`; node image source `d3ccb4c03ecbf09f786c420e8dff9243f1809413` | [full Rust/static CI](https://github.com/rchain-community/rchain-rust/actions/runs/39389655344), [Lean/Coq/conformance CI](https://github.com/rchain-community/rchain-rust/actions/runs/39389655307), [targeted/fault-control CI](https://github.com/aixaria0/rchain-rust/actions/runs/39389941677), [real nodes](https://github.com/aixaria0/rchain-rust/actions/runs/39390752453) | CI and formal jobs pass. Real-node run has independently passing healthy N=4/N=8 and passive LFS arms, but overall run failed. Read the independent outcomes below. |

The production Rust source is unchanged between the combined, regression, image and recovery-fixture revisions; fixture/workflow changes are recorded separately. The retained image archive was checked by SHA256, embedded source SHA and a source diff before reuse. A wrapper/fixture commit is not represented as the binary's source commit.

## Closed technical gaps

1. **Initial LFS reply delivery (C335, #402).** Transport ACK is not reply admission. Startup now re-solicits the identical anchored request until an authenticated nonempty reply is admitted. Ten attempts each bound send and reply waits at ten seconds; exhaustion is terminal, through the existing fatal-startup path. Restore completion remains independently required. Tests cover ACK without reply, second-response recovery with unchanged request flags/anchor, a pre-poll notification, absent bootstrap, hung transport, and unauthorized/empty admission. The deliberately restored send-only completion behavior makes the missing-reply regression fail. This is fault reintroduction, not a claim that an unchanged historical checkout ran the new test.
2. **Inventory omission detection (#401).** Every one of the 85 rows in upstream §6 has its original text and digest, classification, existing evidence, missing obligation, next action and acceptance test. Forty-one are conservatively A candidates, not 41 newly proven defects. The checker rejects missing/new/drifted rows, omitted tracker IDs, invented verification/deferral and self-awarded sign-off. The inventory is pinned before unmerged C335 adds a new node-local row; re-inventory a future merged register.
3. **Integrated source verification (#399).** Existing 27 Decimal preflight, 18 #373 evidence-gate, 17 tracker and 10 consensus harness controls pass. Targeted tests pass for block-version schedule boundaries (six), Decimal representation bridge (five, including 128 property cases), initial reply handling (six), NodeSyncing (12), elapsed escape (two), monotonic reset (one), committed bounds/restore guard (two), PosParams and malformed/unknown formats. Full formatting, Clippy/static, Rust and Lean/Coq/conformance jobs also pass on the linked source. These are finite model/test claims, not a complete Decimal arithmetic equivalence theorem.

## Proof matrix

| Dimension | Reproducible positive evidence | Remaining obligation |
|---|---|---|
| Correctness | Schedule boundary/mismatch and malformed schedule tests; committed PoS bounds with opposing local config; unknown native format refusal; bounded authenticated reply tests | Review and land the separate production PRs; wider governed shard-parameter transitions are not implemented by the partial bounds work |
| Safety/finality | Healthy N=4 and N=8 finalize the same marker on every validator at height 2 and retain it through height 24; original finalized snapshots are checked | Universal absence of conflicting finalized blocks, arbitrary partitions and rotation is not established by a marker witness |
| Liveness | Quiet initial marker finality passes for N=4/N=8; 68 real escape observations are 16,000–38,234 ms, none below the 15-second predicate | Partition/rejoin outcomes are separate; retention after initial quiet finality uses declared supporting Nil deploys |
| Replay/state | Five of five named-anchor justification ancestors have matching full pre/post-state fingerprints; anchor locally finalized before and after persisted restart | Complete historical block, deploy-source and native-state exports plus independent reconciliation/replay are unavailable |
| Recovery | Reply-path network control records ACKed re-solicitation; passive anchored restoration and restart without a second LFS pass | Active key participation, repeated partition healing and post-recovery progression require their own acceptance below |
| Compatibility | Tag-35/source bridge, legacy signature vectors and fail-closed preflight controls pass | Actual historical tag-35/native-state scan, complete deploy source and uniform live validator/client binary digests remain missing |
| Formal verification | Linked formal job succeeds for the repository's Lean, Coq and conformance scope | Comparator/value representation models and bounded conformance do not prove exact Decimal algebra or distributed operational safety |
| Operations | Disposable containers are capped at 2 GiB; tests have explicit finite windows and preserve logs/outcomes | A configured cap is not a measured worst-case bound; production resources, topology and operator authorization are unverified |

## Release blockers and decisions

The [39-row matrix and 85-row reverse inventory](https://github.com/rchain-community/rchain-rust/pull/401) are the item-level source of truth. Every row includes the exact invariant, related work, available evidence, missing proof, smallest action and acceptance test. No outstanding item is marked formally deferred without a named-release approval.

- **Decimal on old genesis:** #373's height schedule does not retroactively gate Decimal. Neither the 27-test preflight nor the bridge supplies historical archive completeness or a binary-level old-genesis prohibition. Require authenticated complete block/deploy/native-state exports and actual validator binary provenance; freeze the intended refusal/migration policy before adding a consensus rule. Do not deploy a Decimal-capable binary to the old genesis on this evidence.
- **State and supply preservation:** operators must provide a finalized authenticated native/RSpace export and complete history. Independently reconcile vaults, bonded stake, reward state, supply and deploy replay. Rollback after a consensus-changing block must follow an approved checkpoint/migration plan; binary downgrade is not demonstrated safe.
- **Crypto-agility (#274):** the seed-length panic is already fixed at `886869780974ba6824706f99d35b5fb12d1595e6`; typed errors and unchanged V1 bytes pass existing tests. Fixed 65-byte validator identities and two legacy secp schemes remain. Jim/security must freeze canonical registry/wire, activation authority, pricing and sunset rules before implementing enabled schemes and cross-version migration vectors.
- **Governance/economics:** double mint, supply migration, genesis contract upgrades, merge semantics, reward/rotation policy, unbond throttling, phlo exchange, storage rent, prepaid execution, assetStore, decentralized RSpace, staking pools and receipt/translator interfaces have no approved final policy or named-release deferral in this evidence. Maintainers must approve each exact interface/conservation invariant and activation scope; no economic allocation was selected here.
- **Completed prerequisites:** current Law 53 records both C173 inverses as shipped; do not implement them again because #51 has stale conditional wording. #247 selected an already-funded faucet key and corrected outcome reporting; remaining acceptance is a finalized drip and operator/key attestation, not a new genesis allocation. RCHIP #43/#40 must not be confused with unrelated local issues sharing those numbers.
- **Release sequencing:** Jim must approve merge order, canonical genesis/parameter hashes, uniform validator/client rollout, activation authority/height and recovery/rollback limits. Independent §6 classification and signed release disposition are still required. #51's closeout command must continue to refuse closure.

## Reproduction

Use the exact linked commits rather than current dev. Read `.github/workflows/devnet-fuzz.yml` for all arguments and resource limits.

```sh
python3 -m unittest discover -s tests -p test_decimal_release_preflight.py -v
python3 -m unittest discover -s tests -p test_pr373_release_gate.py -v
cargo test --locked -p rchain-casper --lib initial_fringe
cargo test --locked -p rchain-casper --lib node_syncing
cargo test --locked -p rchain-casper --lib round_escape
cargo test --locked -p rchain-casper --lib monotonic
cargo test --locked -p rchain-casper --test committed_bounds
```

On the #401 branch, run `python3 tools/check-section6-inventory.py`, its eight `test_section6_inventory.py` mutation tests, and `python3 tools/check-hardfork-51-closeout.py --assert-closeable`. The last command must exit 3 (`TRACKER_MUST_REMAIN_OPEN`). Do not copy its upstream-pinned inventory onto a changed §6 source without re-inventorying.

The opt-in `final-gap-rust` campaign runs targeted Rust/control tests and send-only fault reintroduction. `final-gap` builds one integrated image and runs independent disposable node arms. `final-gap-recovery` reuses the source-pinned retained image, verifies ZIP and image digests plus a production-source diff, then runs corrected recovery/partition fixtures. The original image artifact expires after three days; its digest and compact outcomes are retained here, while full logs have the workflow's 14-day retention. A later reproduction must rebuild the exact source if the artifact has expired.

## Independent recovery and partition outcomes

[Run 39393626848](https://github.com/aixaria0/rchain-rust/actions/runs/39393626848), fixture `6e883ceb8d48f45c470a9a2cac74529f80f79b5d` on unchanged d3ccb4 node image, passes anchored LFS/restart, same-volume original-key reactivation and N4's two partition/heal cycles. The new accepted marker finalizes at height 9 on source and joiner while the original height-2 anchor and all five ancestry/state fingerprints remain unchanged. The reactivated validator logs successful local proposals at heights 6–12, sequence 5–11; this establishes observed active participation, not merely a configured key. No fresh LFS, reported equivocation or bad-prestate error appears in the activation scope. Six harness controls now refuse passive-only acceptance, sequence reuse/regression and wrong/missing durable volume identity. The stricter LFS-only [39395992605](https://github.com/aixaria0/rchain-rust/actions/runs/39395992605), fixture `0c9ccc6018252ef4b330570328cbf395c1dd884e`, passes against the same source-pinned image.

N4 retains its original finalized snapshots through two disconnect/rejoin cycles on the same data volume. Its final rejoined observations are heights 50/50/50/48 with the marker retained; equal latest-finalized heights on all validators are not claimed. One resource sample is 46.25–59.3 MiB per container against 2-GiB caps; it is not a peak-memory bound. This is an explicit supporting-deploy workload, not quiet partition liveness.

N8 fails the 300-second online-progress criterion in that same run: height 24 reaches only 25 instead of 27. Healing separately restores all eight validators to the same finalized marker at height 25 and preserves old snapshots, but the overall result remains FAILURE and the second partition cycle is not executed. [Causal rerun 39395723981](https://github.com/aixaria0/rchain-rust/actions/runs/39395723981), fixture `141f37fb64813cbe85ee68da50e04fd69fb6c47b`, independently fails: finalized 27 reaches 29 instead of the required 30 over 146 observations. Raw pre-healing DAGs show user-deploy traffic through height 75, surviving cadence-due peers withholding attestations, no proposer timer halt and a correct 100/800 finalizer refusal for its observed support. C336 tests a proposer-only correction; its acceptance must be recorded separately before this blocker can close.

The passed formal job includes the repository's existing Coq compilation/axiom checks. It does not turn the axiomatized Coq track into completed proofs.
