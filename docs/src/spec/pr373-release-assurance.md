# PR #373 — release assurance and operator preflight

This document separates implementation regressions from the **operational preconditions** agreed in the PR #373 review. It is not an authorization to deploy.

**Integrated validation tree:** `dev@bdcc6f5871aee0a05f50d559d84dd36a6b69bb5b` plus PR #373 `e1dfcdef210d8d6fc4d757fdac5a3ff49c001d45`. The merge must be tested before any release decision. The merge itself is not a proof that historical blocks replay safely.

## Evidence obligations

| Review question | Existing or executable witness | Status / decision boundary |
| --- | --- | --- |
| Append-only, unambiguous PoS layout | `rholang::native_state::tests::params_wire_slots_are_append_only`; `params_round_trip` with distinct values | Testable; format 1 is ten little-endian fields after one byte |
| Wrong version, unknown format, malformed length | `models::block_version` schedule tests and `an_unknown_params_format_is_refused` | Testable; scheduled version is currently `(1,0)` |
| Old params must retain the old grace and default the new bounds | `a_short_params_record_reads_the_pre_change_rule_and_an_over_share_is_refused` | Testable; no claim about every historical stored root |
| Validity checks read committed state rather than untrusted local config | `casper/tests/committed_bounds.rs::committed_bounds_control_the_block_path_with_opposing_local_configs` | Testable; includes opposed local settings |
| Genesis, reconnect and LFS restore must reject incompatible configured bounds | `restore_guard_checks_committed_zero_legacy_and_bad_records_before_running` | Testable; does not replace a real multi-node restore acceptance |
| `fundPot` must conserve REV and refuse errors atomically | `funding_the_pot_adds_unclaimed_rev_and_bonding_does_not`, `pot_funding_overflow_moves_neither_balance`, native conservation sweep, dispatcher tests | Testable; concurrent funding still has a documented native-vault write conflict |
| #371 allocation must not silently collide with #373 fields | #373 format 1; #371 must use a deliberately reviewed next format and must be rebased after this PR | **External dependency** — do not merge or rewrite #371 here |
| Exact decimal type (#389/#390) needs no speculative schedule row on a fresh forced genesis | PR #373's genesis changes plus a uniformly upgraded validator set | **Release condition**, not established by compilation alone |
| Any validator operating on the **old genesis** must not have run a decimal-capable binary | Complete historical validator build inventory, correlated with old-genesis lifetime | **Unverified until actual operator evidence is inspected** |
| No old persisted native-state `Expr` carried wire field 35 | A **schema-aware** scan of all old native-state roots, with counted coverage and immutable source evidence | **Unverified until a real, independently reviewed scan** |
| No old block on the current chain carried a decimal source literal | Full persisted-block/deploy source export and source-language-aware scan; count all blocks and prove completeness | **Unverified until a real, independently reviewed scan** |
| Release begins from the newly produced genesis, with one decimal-aware binary across validators | Per-validator actual image digest, build commit and new genesis/post-state hashes | **Unverified until deployment/acceptance evidence is present** |
| State-preserving rollback or migration from old genesis | Independent rollback/migration and historical replay campaign | **Not proven by PR #373** |

The historical #373 CI status (tests, lints, formal and dependency audit) must always be attributed to its exact source SHA; an integrated `dev` tree needs its own acceptance run.

## Operator manifest gate

`python3 tools/pr373_release_gate.py manifest.json --repo /path/to/full/rchain-rust`

The tool checks the provided inventory *fail closed*. In particular it:

1. Rejects old-genesis in-place binary swap. The allowed activation value is `fresh-genesis-uniform-validator-upgrade`.
2. Decodes the supplied genesis `PosParams` record as **exactly** format 1 plus ten little-endian `i64` values; compares the committed fee floor and parent cap with the operator's values.
3. Requires a complete, unique validator inventory, identical binary digests and source commits, decimal support, and identical genesis block/state hashes. It uses Git ancestry to verify that every **new** binary includes merged #389.
4. For a network **with an old genesis**, requires three checksum-bound evidence files: complete schema-aware native-state-root/field-35 scan; complete block/deploy-source scan; and complete validator build history. Git ancestry must find **no** build containing #389 on the old genesis.
5. For a genuinely new network, requires checksum-bound independent evidence that no old genesis existed.
6. Rejects incomplete coverage, any observed decimal, inconsistent image provenance, missing evidence and corrupt evidence files.

The program does **not** generate those inventories and cannot certify that an operator captured the complete network history. A self-consistent or synthetic manifest is *not* a chain proof. Even a successful run returns `INPUT_CONSISTENCY_PASS_OPERATOR_REVIEW_REQUIRED` and still requires independent corroboration of the archive coverage, build provenance and validator census.

There is deliberately **no passing public-testnet manifest** committed in this branch. No absence of tag 35 or of earlier mixed-version operation has been inferred from source code, CI, or a green rig.

### Example input fields

```json
{
  "schema": 1,
  "activation": "fresh-genesis-uniform-validator-upgrade",
  "expected_validator_count": 2,
  "new_genesis": {
    "block_hash": "<64 lowercase hex>",
    "state_hash": "<64 lowercase hex>",
    "pos_params_hex": "<162 hex chars, format byte 01 + 10 i64 little-endian>",
    "expected_min_phlo_price": 1,
    "expected_max_number_of_parents": 255
  },
  "validators": [
    {
      "identity": "validator-1",
      "build_commit": "<40 lowercase hex>",
      "binary_sha256": "sha256:<64 lowercase hex>",
      "supports_decimal_tag35": true,
      "genesis_block_hash": "<same new genesis block hash>",
      "genesis_state_hash": "<same new genesis post-state hash>"
    }
  ],
  "pre_restart": {
    "old_genesis_present": true,
    "state_scan": {"file": "state-scan.json", "sha256": "<64 lowercase hex>"},
    "deploy_scan": {"file": "deploy-scan.json", "sha256": "<64 lowercase hex>"},
    "validator_build_audit": {"file": "legacy-builds.json", "sha256": "<64 lowercase hex>"}
  }
}
```

The example intentionally has only one validator entry against an expected count of two and placeholders; it **must fail** until replaced by real evidence.

### Correctly reporting outcomes

- **PASS (bounded source tests)**: exact source SHA and CI log for version/format/bounds/`fundPot` tests.
- **PASS (bounded integrated devnet)**: actual genesis block/state hashes across validators, recorded API observations, and real validator binary fingerprints.
- **BLOCKED (release preflight)**: any missing historical export, unverified legacy build census, mixed version/genesis, tag-35 evidence or missing operator confirmation.
- **UNRESOLVED**: a property not tested, including arbitrary history-preserving upgrades, general Byzantine safety and post-launch governance schedule changes.

The #51 fork tracker still owns the named release sequencing and the deferred shape-changing items. Nothing in this verification merges, approves, or reassigns those items.
