# Is the `InvalidStateHash` divergence a fringe-derived one? — pre-registered

**Status: FROZEN before the run.**

Issue **#139**. Its close condition, which is the whole of its `owes`:

> A reproduction (devnet or in-process) that names the deploy and the two hashes, and then either a fix or a
> §6 row if the divergence is a registered departure. **Until there is a reproduction this issue should not
> be worked.**

Nothing below is a result. Every acceptance row was written before any arm ran, and the arms are reported as
they come out — including as null.

## Why this is the measurement

The observation, from the closed #105 thread (shape only — the artifacts are gone and no `658efb50` exists
anywhere in this tree): three validators A 100 / B 100 / C 50, `--epoch-length 10`, mid-storm. B and C
rejected the **same** block from A with `InvalidStateHash` in the same second, **just after the epoch
boundary at 40**; A recorded nothing and kept proposing. Second run, A 1000 / B 100: B logged five.

**The mechanism this preregisters a test of.** The replay's `fringe_state_hash` is not carried on the block
(`casper/src/interpreter_util.rs` says so) — each node derives it from its own DAG, and the replay feeds it
to the `CloseBlock` system deploy, which anchors **the next epoch's seed** to it
(`rholang/src/native_state.rs`'s `set_epoch_seed`). The design's own comments say the seed is "derived by
each node from its own DAG" and *"Nothing is published and nothing is verified"*. So two nodes whose fringes
differ replay the same block to **different post-states**, and the block's *declared* hash is the same
everywhere — only the recomputed one differs.

**And a second, sharper mechanism the exploration found in this tree.** `populate_dag`
(`casper/src/engine/node_syncing.rs`) inserts every **non-genesis** block a node restores as
`BlockMetadata::from_block`, which sets `fringe: ∅` and `fringe_state_hash: 0` — the real values are a local
recomputation and are not on the wire. `DagBlockStorage::insert` then caches
`fringe_states[fringe_hash_of(∅)] → 0`, so `get_pre_state_for_parents` begins from an empty fringe and reads
`prev_fringe_state = 0`. A node that LFS-syncs a **mature** chain therefore replays boundary blocks from a
zero seed. A joiner syncing at *genesis* restores block 0 alone, which is why ordinary devnet joins work and
this has stayed invisible.

**Scope correction, recorded because it shapes every rig below:** a **single-parent** block takes its
`pre_state_hash` from the parent and never consults the fringe, so the divergence is confined to (a) blocks
carrying a `CloseBlock` — epoch boundaries — and (b) multi-parent blocks whose merge base falls through to
`fringe_state`. Every arm triggers on a boundary block for that reason.

**What is already excluded.** The replay is deterministic in its inputs: `casper/tests/determinism.rs` has
seven play-vs-replay hash-equality tests including a close-block one, and all pass. This campaign is about
the *inputs* differing between nodes, not about the replay.

## The instrumentation this campaign reads

Landed before any run, because without it no run can name the two hashes:

- `BlockStatus::InvalidPreStateHash` — the pre-state and post-state mismatches were one status and are two
  different facts; an operator could not tell which had fired.
- `casper.interpreter.validate`'s disagreement line — which predicate, declared vs recomputed, the
  pre-state, the fringe state, `prev_fringe_lookup`, the fringe set, and the deploy identity with an
  explicit `close_block=yes/no`.

**The masking caveat, preregistered.** C173's restoring rule re-validates a `Divergence` record once the
view converges, so a transient refusal can leave **no trace in the final DAG**. Every arm therefore reads
**first occurrences from logs**, never the end state, and a final-state check that finds nothing is not a
null result.

## The arms and their acceptance rows, frozen

### Arm A — "the seed is the fringe" (in-process, hours)

| observation | verdict |
|---|---|
| the same deploy set, replayed under two different fringe states, produces **two different** post-state hashes, each equal to its own play | **mechanism proven** — the `CloseBlock` system deploy is named and the two hashes are the fixture |
| the two replays agree | **mechanism refuted**: the fringe cannot be the cause, and this arm is the evidence |

**The single number:** the pair of post-state hashes.

### Arm B — the restore shape (in-process; the decisive arm)

| observation | verdict |
|---|---|
| a DAG built as `populate_dag` builds it returns `InvalidStateHash` / `InvalidPreStateHash` for the next boundary block, naming two different post-state hashes | **reproduced**; the defect is the restore path and the fix is local to `populate_dag` |
| it returns `ValidateError::Internal` — "Fringe state not available in state cache" | a **different** defect (a missing entry, not a zero one): record it, re-aim, **do not close** |
| it accepts the block | this mechanism is **refuted in-process**; only the devnet arms remain |

### Arm C — two honest views (in-process)

| observation | verdict |
|---|---|
| two DAG views that differ only by extra messages derive different `fringe_state` for the same block, and the validator refuses it | the general mechanism is real; the disposition is a §6 row (nothing is on the wire to check) |
| they agree | the §6 row's premise is weakened and the residue is Arm B's restore path alone |

### Arm D' — devnet, mature join (confirms Arm B)

| observation | verdict |
|---|---|
| the reset node logs the mismatch on its first post-join blocks, in every attempt | Arm B confirmed end to end |
| it logs nothing, while Arm B is red | Arm B's double is **unfaithful** to the restore shape and must be rebuilt before any fix is written |

### Arm D — devnet, the historical shape (the only arm that can reproduce the incident)

Rig: `--validators 3 --stakes 100,100,50 --epoch-length 10 --fresh`, the devnet's defaults, ≥3 attempts,
unfiltered, one devnet at a time.

| observation | verdict |
|---|---|
| ≥2 nodes log the mismatch for the **same block hash** in one run | **reproduced on a live network**; that block hash, the named deploy and the two hashes are the artifact |
| exactly one node logs it | a local view divergence, not a consensus split — reported per node, not averaged |
| no mismatch line in any attempt | **not reproduced on the devnet**; the attempts are reported and the arms covered are named |

**VOID, not negative:** a run that stalls before reaching an epoch boundary gives no reading at all (a
boundary is required, so `--epoch-length 10` needs ≥10 blocks). The manifest records the maximum height
reached, and such a run is reported VOID.

**The single number to report:** per attempt, the block hash of the first disagreement and the two post-state
hashes, with the node that computed each.

## What this does not settle

- It does not measure whether the divergence is *harmful* at the network level — it measures whether it
  happens and what its inputs are.
- Three attempts of one configuration on one machine is the measurement the register asks for before a claim
  is written down; it is not a proof.
- It does not settle the general per-node-seed design (Arm C), which cannot be fixed without putting the
  fringe state hash on the wire — that is a hard fork and belongs on #51's tracker, not here.
