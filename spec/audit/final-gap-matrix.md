# Release gap matrix — 2026-10-20

Pinned upstream dev: bdcc6f5871aee0a05f50d559d84dd36a6b69bb5b. Upstream remains unmerged for #373/#391/#392/#393/#395. #389/#390 are merged.

The machine-readable final-gap-matrix.json enumerates all 39 existing §A/§B/batch obligations with an exact invariant, source evidence, missing proof, smallest action and acceptance test. Overlapping rows are intentional.

The section6-gap-inventory.json reverse inventory covers every 85 row of §6, not just text containing the words hard fork. 41 rows are conservatively category-A candidates. Every row retains its exact source-row digest and full authored evidence. These classifications await independent sign-off; no historical replay or universal consensus proof is inferred.

No named-release deferral is approved here. Economic/governance choices remain owned by maintainers. #51 cannot close while any required row lacks landed-and-accepted evidence or an approved named release.

| Obligation | Classification | Exact invariant / path | Acceptance |
|---|---|---|---|
| A:block-timestamp | IMPLEMENTED / EVIDENCE INCOMPLETE | models block-header timestamp / hash_block | Attachment-free and timestamped block wire/hash fixtures; cross-version replay |
| A:binary-attachments | IMPLEMENTED / EVIDENCE INCOMPLETE | DeployData attachments and deploy-content hash (#53) | Signed attachment deploy round-trip and old/new client refusal; uniform validator/client rollout |
| B:binary-attachments | IMPLEMENTED / EVIDENCE INCOMPLETE | DeployData attachment-bearing client signing/hash contract (#53) | Old/new deploy clients produce the declared hash and explicit refusal behavior |
| B:string-functions | IMPLEMENTED / EVIDENCE INCOMPLETE | rholang string length Unicode scalar semantics (#47) | Non-ASCII S.length corpus and historical replay of affected deploys |
| A:bigint-decimal | RELEASE BLOCKER | models Decimal::scaled_value, wire tag 35; rholang exact arithmetic | Existing decimal_bridge, lean_sort_corpus, lean_parse_corpus; #391 source/history/validator preflight |
| A:double-mint | REQUIRES MAINTAINER DECISION | RCHIP #44 bonded REV double-mint correction — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:hardfork1-migration | REQUIRES MAINTAINER DECISION | RCHIP #42 Hard Fork 1 supply migration — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:vault-export | RELEASE BLOCKER | RCHIP #41 complete REV-vault supply/export invariant | Independently reconcile exported vaults, staking, rewards and supply at finalized checkpoint |
| A:genesis-contract-upgrade | REQUIRES MAINTAINER DECISION | RCHIP #43 governed replacement of genesis contracts — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:shard-config | INTEGRATION GAP | casper/src/validate.rs committed parent-poststate bounds; node_launch restore guard | casper/tests/committed_bounds.rs plus three-node new-genesis/restart/LFS on integrated source |
| A:block-merge | REQUIRES MAINTAINER DECISION | RCHIP #23 merge/event-log validity semantics; committed parent cap is partial | Approve exact semantics or named-release deferral before adding a merge fixture |
| A:rev-economics | REQUIRES MAINTAINER DECISION | RCHIP #47 REV stabilization/mint and vesting — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:validator-rewards | REQUIRES MAINTAINER DECISION | RCHIP #46 rotation/longevity/open enrollment — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:unbond-throttle | REQUIRES MAINTAINER DECISION | RCHIP #4 unbonding rate throttle — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:phlo-exchange | REQUIRES MAINTAINER DECISION | RCHIP #7/#8 phlogiston↔REV exchange and per-block limits — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:storage-rent | REQUIRES MAINTAINER DECISION | RCHIP #40 data retention/rent — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:prepaid-rholang | REQUIRES MAINTAINER DECISION | RCHIP #38 prepaid execution — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:asset-store | REQUIRES MAINTAINER DECISION | RCHIP #30 blessed assetStore if chosen — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| A:decentralized-rspace | REQUIRES MAINTAINER DECISION | RCHIP #53 RSpace decentralization, if consensus-affecting — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:staking-pools | REQUIRES MAINTAINER DECISION | RCHIP #32 staking pools and REV addresses — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:rev-receipt | REQUIRES MAINTAINER DECISION | RCHIP #31 execute-on-REV-receipt — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:genesis-contract-upgrade | REQUIRES MAINTAINER DECISION | RCHIP #43 client-visible blessed-contract upgrade — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:babel | REQUIRES MAINTAINER DECISION | RCHIP #48 Rholang compatibility translator — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:breaking-release | REQUIRES MAINTAINER DECISION | RCHIP #10 coordinated breaking-release runbook — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| B:staking-pools-interface | REQUIRES MAINTAINER DECISION | RCHIP #32 beneficiary/reward channel compatibility — no accepted completed implementation is established by the tracker | Approve the exact policy/interface and named release, then exercise both allowed and refused transitions, conservation and old/new replay |
| Batch:activation-schedule | INTEGRATION GAP | models/src/block_version.rs version_at; casper/src/validate.rs block_version | Height boundary and mismatch rejection; startup/restart/replay on #373 integrated source |
| Batch:crypto-agility | REQUIRES MAINTAINER DECISION | #274 §14 key IDs, algorithm table, deploy IDs, address version and pricing | After Jim/security freezes canonical registry/wire, activation, pricing and sunset rules, run legacy/new signature, address, deploy-hash and migration/replay vectors; preserve current V1 bytes and typed length errors. |
| Batch:governance-schedule | REQUIRES MAINTAINER DECISION | Plan H2 authenticated governance activation and parameter state transition | Approve authority/quorum/activation rules; test unauthorized update and activation/replay boundary |
| Batch:state-preservation | RELEASE BLOCKER | Plan §14.3 S1/S2 complete native/RSpace state and deploy-history preservation | Operators export authenticated finalized state/history; independently reconcile all entries and replay |
| Batch:faucet-genesis | IMPLEMENTED / EVIDENCE INCOMPLETE | #247 genesis-wallet/faucet allocation identity and supply | Using the already-funded signing key, attest operator/key identity and observe a finalized faucet drip with exact balance/supply delta and truthful failure reporting; no new allocation is implied. |
| Batch:attestation-format | IMPLEMENTED / EVIDENCE INCOMPLETE | Equivocation evidence and slash signature/header compatibility | Existing signature/slash fixtures and versioned historical evidence replay |
| Audit:section6-categoryA | IMPLEMENTED / EVIDENCE INCOMPLETE | spec/audit/section6-gap-inventory.json exact reverse coverage | check-section6-inventory.py plus independent reviewer sign-off on every A candidate |
| Audit:law52-partition | IMPLEMENTED / EVIDENCE INCOMPLETE | block-storage/src/dag/finalizer.rs live partition vs quorum; C174/C208 | Existing containment/minimum-message falsifiers plus distributed partition/heal finality witness |
| Audit:law53-estrangement | IMPLEMENTED / EVIDENCE INCOMPLETE | casper/src/validate.rs failed-justification height maximum; C173 | Pin the merged C173 implementation ancestry, run current restoration/failed-height falsifiers, then attach distributed rejoin and historical validity replay witnesses before release disposition. |
| Audit:active-set-draw | IMPLEMENTED / EVIDENCE INCOMPLETE | PoS select_active stake-weighted draw without replacement, epoch-ahead seed | Existing active-set determinism vectors, epoch transition replay and validator rotation acceptance |
| Audit:parent-limit | IMPLEMENTED / EVIDENCE INCOMPLETE | casper/src/validate.rs justification_count; C191; #373 committed bound | Committed bound with opposing local config and over-limit refusal fixture |
| Audit:sidecar-merge | IMPLEMENTED / EVIDENCE INCOMPLETE | casper/src/merging.rs per-accepted-deploy accounting/native conflict relation; C207 | Existing sibling native-write/cost composition regressions and historical merge replay |
| Release:attachment-upgrade | RELEASE BLOCKER | Uniform attachment-aware validator and client release sequencing (#53) | Inventory actual validator/client binary digests; attachment-bearing deploy acceptance and old-peer refusal |
| Release:decimal-genesis | RELEASE BLOCKER | tools/decimal_release_preflight.py; #373 genesis and validator digest inventory | Reuse 27 preflight controls; real archive/native tag-35/source scan and complete live binary inventory |

## Reproduction

python3 tools/check-section6-inventory.py
python3 -m unittest discover -s tests -p test_section6_inventory.py -v
python3 tools/check-hardfork-51-closeout.py --assert-closeable

The final command must still return exit 3. Independent §6 classification, archive completeness, live validator provenance and release authorization are separate gates.

## Highest-value implementation follow-up

#402 addresses missing initial LFS reply delivery without changing CBC finality. #399 carries the combined code for separate Rust, formal and real-node acceptance. The results of those campaigns must be attached before any completed-acceptance claim.

## Source rechecks

#274 still lists a seed-length panic, but source commit 886869780974ba6824706f99d35b5fb12d1595e6 already returns a typed error. Existing oversize/V1-byte and legacy-signature regressions pass in CI 39388911701; this is partial crypto-agility evidence, not PQ support. Current Law 53 records both C173 inverses as shipped. #247 documents an already-funded faucet key and outcome reporting; it is not an unselected new allocation. These corrections avoid reopening completed code.

The reverse census is pinned to upstream dev, before unmerged #402 C335 adds its node-local startup row. A future merged register must be re-inventoried, not silently treated as this snapshot.
