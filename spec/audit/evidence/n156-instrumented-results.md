# #156 instrumented run — the equivocation reproduces, and it is a race, not the failed record

Run 2026-10-01 on tree `b9c16b5da` (C190 repair + instrument), image `rnode:local` rebuilt from it.
The preregistration is `n156-instrumented-preregistration.md`; the node log is `n156-instrumented/validator-1.log.txt`.

## The run

`tools/devnet.sh up --validators 2` (restarted against a 289-round chain), then `reset 1`. Validator 1
wiped its store and LFS-synced a chain that already carried its own blocks (it had produced seq 1..292).

## The result — reproduced, and it is *not* the shape C190 fixed

The self-equivocation reproduced: **11** `equivocation detected: sender produced two blocks with the same
sequence number` lines, and the #157 halt fired once (`halted after 3 consecutive self-validation
failures`). The node recovered once synced (`proposed and added block #287` … `#288`).

**And the C190 instrument never fired** — `AUDIT C190`/`msg_map but not latest_msgs`: **0**;
`cleared this node`: **0**; `validation failed` (any): **0**. There was no failed record anywhere.

So the wedge is not the one C190 named (H-2 keeping a `validation_failed` record out of `latest_msgs`). The
colliding block is the node's *own old block, inserted validly by the sync*, and the proposer reads a stale
`latest_msgs` before that insert lands.

## The mechanism — a proposer/sync TOCTOU window

`proposer.rs::propose` reads `latest_msgs` → `next_seq` (`:263`), and `create_block` derives the parent set
from a snapshot of the same structures (`get_pre_state_for_new_block`, `:583`), then builds the block and
inserts it much later (`dag.insert`, `:432`). The sync (`populate_dag`) inserts the node's own old blocks
into that window, each taking its `(sender, seq_num)` in `msg_map` — so a proposal derived against a snapshot
that predates the sync's advance collides on insert. `insert_msg_mut` writes `msg_map` and `latest_msgs`
correctly; there is no failed record. The two structures are never actually *disagreeing* — the proposer is
just reading one that is already out of date by the time it writes.

## What this does and does not settle

- **The owed sentence is now observed, and it says a different thing than §43's addendum assumed.** §43
  named the reachable shape as *"a node that receives and records one of its own blocks as failed"*; this run
  shows no failed record is involved — the reachable shape is the sync/proposer race above.
- **C190's repair is not wrong, but it addresses an unreachable shape on this tree.** Its candidate scan
  looks for a `Divergence` record in `msg_map` above `latest_msgs`; the sync creates none, so it is never
  reached here. It remains correct defence for the shape it names (a genuine state-accounting failure that
  does write a `Divergence` record of the node's own block).
- **The actual defect — a stale-snapshot self-equivocation during re-sync — is unregistered.** It produces
  the #156 symptom and the #157 halt, and it is distinct from C190. It deserves its own row, and a fix is a
  decision (re-derive the seq against the DAG's current state at insert time, or refuse-and-retry on a
  caught equivocation rather than counting it as a self-validation failure), not a one-line edit.

## Not claimed

That the race is the *only* remaining path (a `Divergence` self-record may still be reachable by a genuine
divergence, and C190's repair covers it there); or that the node stays halted — it recovered once synced.
The instrument is now in the tree for the shape C190 names; the race needs its own instrument if it is to be
observed end-to-end.
