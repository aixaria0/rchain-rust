# A staging instrument for the reset arm — design

**Why this is needed.** §108 records the attempt at F-U5-01's devnet arm — *a diverged running node
re-enters sync* — and its negative result: the route §101 named (`up --validators 4 --fresh`, the §92
self-reject) stages nothing on this tree, because the divergence §92 measured came from the fringe-record
store's `min` tie-break, retired by law 66/67/68. The arm needs an **instrument**, not a longer run. This
is the design.

## What the reset actually needs (read from the code, not assumed)

1. A **running** node. The act lives in `node_launch::apply`'s drive loop, as the third exit of its
   `select!` (`casper/src/engine/node_launch.rs`). Every boot-time path — `NodeSyncing` and
   `sync_anchor`, which `node_launch.rs:231` says is *"only consulted when this node's DAG is empty"* —
   runs *before* that loop and never reaches it.
2. A **spent record**: `restore_budget_is_spent` (`casper/src/multi_parent_casper.rs:652`) is
   `validation_failed && failure_cause == FailureCause::Divergence && restore_attempts >=
   RESTORE_ATTEMPT_LIMIT` (3).
3. A **detector that reaches it** while running: `clear_own_failure_record` on the proposer timer, or
   `restore_divergent_justifications` on an incoming block (`:796`, `:1202`).
4. An **anchor with a height**: `rewind_plan` (`block-storage/src/dag/representation.rs`) takes
   `last_finalized_block_hash()` *and* `block_height_of` it, and `reset_decision` acts only on the arm
   where both exist — the other two arms are refusals.

`FailureCause::Divergence` has exactly five sources (`casper/src/block_status.rs:138`): `InvalidStateHash`,
`InvalidPreStateHash`, `InvalidRejectedDeploy`, `InvalidBondsCache`, `UnjustifiedSlash` — all "decided by
this node's state or replay". So the instrument must make one node's **state** differ from its peers'.

## The options, and why four of them fail

| option | why it does not stage the arm |
|---|---|
| `--sync-anchor <bogus>` | **Requirement 1.** Consulted only when the DAG is empty (`node_launch.rs:231`) — a boot path, before the drive loop exists |
| a genesis/bonds/wallets mismatch (`DEVNET_EXTRA_FLAGS_*`, #150's A1 arm) | the failing block is **#0**, and a node whose DAG is empty is in `NodeSyncing`, not the drive loop: it stalls at height 0 (§92's *"two joiners sitting at height 0 and never answered"*) instead of resetting — the same shape as `C46`'s empty-fringe constraint |
| the C215 injection (perturb block content) | **self-defeating**: an injected node's blocks are un-validatable by a *clean* node, so the injected node validates its own blocks fine and never records a `Divergence`. Retired for exactly this (§100) |
| the §92 natural self-reject | does not reproduce: its cause was retired by law 66/67/68 (§108, `run.md`) |
| **a synthetic spent record, written while the node is stopped** | **Recommended.** Reaches all four requirements with one store write, and nothing about the reset's behaviour depends on *why* the record is spent beyond its cause field |

A two-root divergence (two nodes with different geneses) fails for the mismatch row's reason, and the
older-image route (`RNODE_IMAGE_devnet_validator_N`) for a related one: a node that disagrees with the
*majority* is a fork, and its own boot path — not its running loop — is where the disagreement first appears.

## The recommended instrument: `tools/devnet.sh drill <node>`

1. `stop <node>`;
2. a **dev-only helper** opens the node's `dagstorage` LMDB, in its `block-metadata` DB, and rewrites one
   record — for a block **above** the node's last-finalised block — to
   `validation_failed = true`, `failure_cause = Some(Divergence)`, `restore_attempts = RESTORE_ATTEMPT_LIMIT`;
3. `start <node>` with `--self-reset` already in its flags;
4. observe, in order: the detector's `cannot restore block …` line (the window), then `reset requested`,
   then the engine's `rewound N block(s) above height …` and the catch-up walk restarting from the anchor,
   then the node's height and finality recovering.

**Two choices in the design, and why:**

- The helper **reuses the tree's own `BlockMetadataCodec`** rather than re-encoding the record, so the
  instrument cannot drift from the store's format — the failure Class this repository pays for most often is
  a hand-maintained second implementation of an on-disk shape.
- It is a **dev-only binary (an example/crate), not an `rnode` subcommand**. A store-rewriting subcommand on
  the shipped binary is a product seam (`build_dag`'s own note in `casper/tests/reset.rs` records the same
  objection for the test fixture), and `tools/audit-type-system.sh`'s classes are the wrong place to discover
  one.

## What it measures, and what it does not

**Measures** — the **engine re-entry**, which is the half no test reaches: the drive loop's third exit,
`act_on_reset_request`, `perform_reset`'s three durable effects under one completion record, and the
catch-up walk restarting from the anchor. §106 and §107 both record that wiring as *read rather than run*;
this is the instrument that would run it.

**Does not measure** — a node whose **state** genuinely diverged *healing*. The record is synthetic, so the
node re-fetches and re-validates the very blocks it dropped, on a chain it agrees with, and the re-validation
succeeds. A genuine state divergence would fail the re-validation again, and the node would loop between
its anchor and its peers' tip rather than heal.

**So the arm's falsifier — *a diverged running node re-enters sync* — is exercised in part**: the re-entry
yes, the healing no. That boundary is the reason this document exists rather than a claim that the arm is
run: an instrument that reaches the code path is not the same as a measurement of the property, and F-U5-01
stays **owed** for the difference.

## Cost

A helper of ~150 lines (open the LMDB through the tree's store manager, decode and re-encode one
`BlockMetadata`), a `drill` subcommand in `tools/devnet.sh` of ~40 lines, and one devnet run of ~10 minutes
with `--self-reset` armed. The run is scripted exactly as §108's was, so the observation loop already exists
(`observe.log`'s sampler).
