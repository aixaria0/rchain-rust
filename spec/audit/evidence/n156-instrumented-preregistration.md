# #156 instrumented run — the preregistration

**Tree `b9c16b5da`** (the C190 repair `84b87b773`/`7fd9eab3c` plus the instrument `b9c16b5da`), image
`rnode:local` rebuilt from it. This run discharges the sentence #156's body carried since the original
filing: the C190 mechanism was *shape-matched* but **no recorded line said which structure the arithmetic
read**. The instrument added in `b9c16b5da` is that line —

```
this node's own block {hash} (seq {n}) is in msg_map but not latest_msgs (latest seq {m}): …
(AUDIT C190)
```

— logged by `clear_own_failure_record` when the candidate scan finds a record the gate's `msg_map` holds
and the proposer's `latest_msgs` does not.

## The shape

`tools/devnet.sh up --validators 2`, wait for a mature chain, then `tools/devnet.sh reset 1` — validator 1
wipes its store and re-syncs a chain that already contains its own blocks. §43's addendum names this the
reachable shape: a node holding `validation_failed` records of *its own* blocks.

## What counts as each outcome (frozen before the run)

- **Reproduced** — the instrument line (or the repair's `cleared this node's own failure record`, which
  implies the candidate) appears in `devnet-validator-1`'s logs after the reset, with a `SeqNum` that the
  node's own arithmetic had already spent.
- **Unreproduced** — a bounded observation window (the reset node reaching a stable synced height and
  producing at least one post-reset block) passes with **no** such line.

**The prior, stated before the run:** the original C190 observation ("one attempt in five", #139's
`devnet.sh reset` reproduction) depended on #139's `InvalidStateHash` — the re-synced node re-validating its
own restored blocks against the wrong fringe. #139 is fixed (`dbaa5529d`, the fringe rides the sync), and
`populate_dag` inserts restored blocks as `BlockMetadata::from_block` (valid, no failed record), so the
trigger is expected to be gone. If the run is *unreproduced*, that is the finding: the shape is gone with
its cause, and the instrument is in place for when a genuine state-accounting failure re-creates it.

## Not claimed either way

That the mechanism is wrong — only whether the current tree still produces the shape. The falsifier
(`the_node_targets_only_its_own_spent_record`) already pins the mechanism in process against the real
`DagMessageState`; this run is the live observation, not the proof.
