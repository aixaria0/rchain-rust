# Issue #51 — Hard-fork closeout ledger

This is the reviewable closeout packet for [Issue #51](https://github.com/rchain-community/rchain-rust/issues/51), **not** a declaration that the issue can close. It is tied to upstream `dev@bdcc6f5871aee0a05f50d559d84dd36a6b69bb5b` as observed on 20 October 2026. This source commit is pinned so later merges cannot be silently interpreted as having been reviewed here.

## Outcome

**TRACKER_MUST_REMAIN_OPEN.** The authored machine-readable record at [hardfork-51-closeout.json](hardfork-51-closeout.json) contains 39 audit entries: 4 entries with verified merged-PR references, 35 unresolved entries, and **zero recorded owner-and-reason deferrals**. These entries represent evidence obligations and include deliberate cross-references; **35 does not mean 35 distinct defects.**

The tracker closes by exhaustive classification: every §A/§B row and every relevant §6 category-A deviation must be either *merged* or formally *deferred with a named decision owner and one-line reason*. Unimplemented proposals and release-stage dependencies are **not** closed by green CI, historical prose, or an unmerged PR. The §6 reverse inventory is itself currently an unresolved ledger entry; therefore the list of 39 entries **must not be described as a complete independent §6 audit**.

## Verified merged changes that the original §A/§B table understates

| Status | Change | Merge commit | Scope |
|---|---|---|---|
| Merged | [PR #44](https://github.com/rchain-community/rchain-rust/pull/44), timestamp | `228b66742f` | Code merged; protocol compatibility still requires rollout discipline |
| Merged | [PR #47](https://github.com/rchain-community/rchain-rust/pull/47), string functions | `f07a2628a7` | §B string-function implementation |
| Merged | [PR #53](https://github.com/rchain-community/rchain-rust/pull/53), RCHIP #39 attachments | `9a04285a6b` | **Correction:** §A/§B “no PR” is stale; code merged, synchronized validator/client upgrade still needed |

GitHub merge ancestry was compared to pinned `dev`, not inferred from a PR title. The ledger counts RCHIP #39 twice because it has both §A and §B rows; that is one implementation, not two separate fixes.

Other already merged work whose release implications remain open includes [#49](https://github.com/rchain-community/rchain-rust/pull/49), [#389](https://github.com/rchain-community/rchain-rust/pull/389) and [#390](https://github.com/rchain-community/rchain-rust/pull/390) (BigInt/Decimal), [#376](https://github.com/rchain-community/rchain-rust/pull/376) integrated into **unmerged** [#373](https://github.com/rchain-community/rchain-rust/pull/373), and merged [#379](https://github.com/rchain-community/rchain-rust/pull/379) (a coordinated proposer timing correction, **not category A**). These are not grounds for closing their larger unreleased rows.

## Blocking decisions for Jim and release maintainers

**Must settle before the planned first-release fork:**

- **#373** — activate block versions/committed bounds/format changes and `fundPot` on one reviewed integrated release source; keep its Draft status and review dependency visible until accepted.
- **#274** — explicitly deliver or disposition crypto-agility shape changes: validator key IDs, algorithm negotiation/table, deploy content hashes, PQ address and fee/sunset semantics. A height schedule by itself does not make a wire/identity-shape change compatible.
- **RCHIP #43** — implement or explicitly disposition a governed blessed-contract upgrade mechanism; a new alias/state structure after launch is class A.
- **State preservation and transition** — authenticate historic block/deploy/native-state snapshots, audit old and new binary provenance for every validator, verify the shared new genesis block/state hash, and independently sign off the new-genesis procedure. [#391](https://github.com/rchain-community/rchain-rust/pull/391)'s evidence preflight is deliberately not authorization.
- **#339** — integrated validator finality/write-retention acceptance must run on the intended release binary, not only an earlier devnet build.

**Explicit governance decisions (no implied outcomes):**

- **#371 / #388 versus RFC #372:** fixed allocation versus rank-weighted capped reserve are different proposals. The author-open comment interval on #371 extends through **at least 23 October 2026**; this ledger does not select a recipient, formula, or winner.
- **RCHIP #40**, faucet-genesis [#247](https://github.com/rchain-community/rchain-rust/issues/247), RCHIP #30 and RSpace #53: land or record a defensible, explicitly approved design disposition.
- **Deferred economics and protocol proposals**, including #4, #7/#8, #31, #32, #38, #46, #47 and C173: each deferral must identify one accountable person by GitHub login and give a specific one-line reason. A release name is no longer required under Jim's 2026-10-21 decision; neither a vague role nor a placeholder reason counts.
- **§6 category-A reverse inventory:** independently reconcile every consensus-changing row in `spec/audit/passes.md` with landed code or a recorded owner-and-reason deferral. A label or one green test cannot substitute for the inventory.

## Reproduce the closure refusal

```sh
python3 -m unittest discover -s tests -p test_hardfork_51_closeout.py -v
python3 tools/check-hardfork-51-closeout.py
python3 tools/check-hardfork-51-closeout.py --assert-closeable
# expected exit 3: TRACKER_MUST_REMAIN_OPEN
```

The normal command checks schema, duplicate IDs and exact 39-record membership, merged evidence metadata, outstanding owners and deferral requirements. The `--assert-closeable` mode refuses closure until there are **zero unresolved entries**, all declared deferrals have a named person and a substantive one-line reason, the §6 inventory has been signed off, and provenance has been checked. The checker **does not itself close the GitHub issue or approve a network release**. `--verify-local-git` can add ancestry checks when a sufficiently complete local repository is available; shallow clones do not prove ancestry.

The tests include deliberate attempts to drop an unresolved item, invent a deferral, omit a decision owner, duplicate records and make `closure_requested: true` override real blockers. No public network or original proposal is modified by this ledger.

## Requested decision

@jimscarver — please identify which of the remaining class-2 shape changes **must land in the first release**, and assign named target releases and decision owners to items deliberately deferred. Then request an independently reviewed §6 reverse inventory and release provenance packet. Only after those conditions are met should a closeout reviewer change the ledger's status and close #51.

## Complete source-row inventory added

The companion final-gap-matrix.json enumerates every 39 tracker obligation with its missing proof, concrete next action and required acceptance. section6-gap-inventory.json covers all 85 §6 rows and conservatively flags 41 category-A candidates. Eight mutation controls detect omitted/changed rows, source drift, fabricated execution and self-awarded sign-off. This closes the enumeration gap only; independent classification and historical acceptance remain open. No release deferral is approved.
