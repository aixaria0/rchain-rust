# #280 — the merge lost a write every parent's state carried

**Taken live from `https://testnet.rhobot.net` (node A) while the net was still wedged**, so the
reading is of a chain that is doing it, not of one that used to. The instrument is
`n280-live-capture.py`; everything under `n280-merge-loses-a-write/raw/` is the node's own reply,
verbatim, and `blocks-summary.tsv` is derived from those replies only. Nothing in this account is a
note taken while reading a log that no longer exists.

## The finding in one line

**A merge produced a state that had lost a write present in every one of its parents' states — the same
state on all four validators, with no node refusing a peer's block — and the block that carried the
write was rejected *as a whole* because a sibling chain in it lost a native-writer contest against an
equal-valued sibling.** It is the residue of AUDIT **C207** (`spec/findings.tsv:259`), whose own row
named this escape: *"a system deploy escaped, and that asymmetry is why nothing looked wrong: `close_block`
writes no `pos:vault`, and two sibling boundaries compute identical values from the same pre-state, so
rejecting one leaves the other's equal write in place."*

It is **not** C215. C215 is *two nodes* merging the same justifications to different pre-states
(`InvalidPreStateHash` between peers). Here every node agreed, at every height, and no block was refused
by a peer.

## The chain, as the node reports it (`blocks-summary.tsv`)

Four equal validators — **A** `0410b8c5`, **B** `041ed2a2`, **C** `04d7707c`, **D** `04dce59b` — one
block each per height through 103, then A alone at 104 and 105.

| h | block | by | preStateHash | postStateHash | deploys | rejectedDeploys |
|---|---|---|---|---|---|---|
| 99 | ×4 | A B C D | b3168385 | b3168385 | 0 | 0 |
| **100** | `357b06f8` | A | b3168385 | **ff6308f1** | **1** | 0 |
| **100** | `3cc759f6` / `4b934ceb` / `c2b97730` | C B D | b3168385 | **3bf78f23** | 0 | 0 |
| 101 | ×4 — `e3a8e5fc` is C's and is the finalized block | A B C D | **e1296029** | **e1296029** | 0 | 0 |
| 102 | ×4 | A B C D | e1296029 | e1296029 | 0 | 0 |
| **103** | `1dc55474` | A | **3bf78f23** | 44518c03 | 1 | **2** |
| 103 | ×3 | B C D | **3bf78f23** | 3bf78f23 | 0 | 2 |
| 104 | `0723f71f` | A only | 3bf78f23 | 3bf78f23 | 0 | 5 |
| **105** | `ae973c3c` | A only | **4bb663bc** | eedb159b | 1 | 5 |

Height 100 is an epoch boundary (the net runs `epoch_length = 10`), so **every one of the four blocks**
ran the `CloseBlock` system deploy and every one of them changed state: the three empty blocks all moved
`b3168385 → 3bf78f23` (`preStateHash ≠ postStateHash` with `deployCount: 0` is the signature), and A's
additionally applied the user deploy, `→ ff6308f1`.

## The write, read back at six states (`explore-issues-*.json`)

The Issue contract's `issues()` facet, read through `explore-deploy-by-block-hash` at each block's
**post**-state — one term, six anchors, so the column is comparable:

| 100-A | 101-D (finalized) | 102-A | **103-A** | 104-A | **105-A (tip)** |
|---|---|---|---|---|---|
| `["does-it-work-0v2p6"]` | `["does-it-work-0v2p6"]` | `["does-it-work-0v2p6"]` | **`[]`** | `[]` | `[]` |

Three things follow, each from the tables alone:

1. **The write was real and was merged.** It is in the state at 100 (A's own block), at **101 (the
   finalised block)** and at 102 — so it is not a local artefact of A's proposal, and it survived into
   finality. The `#103` `cast` deploy's answer, `("gov-error", "no such issue", "does-it-work-0v2p6")`
   (`deploy-status-103-cast.json`), is therefore *correct against the state it was handed*; the deploy
   did nothing wrong.
2. **A merge lost it, and it lost it unanimously.** Every height-103 block — A's, B's, C's and D's —
   carries pre-state `3bf78f23`, which is exactly the post-state of the three **empty** height-100
   blocks. So four independent nodes re-derived the round-100…102 scope and agreed on a state that
   three rounds earlier had contained the deploy. There is no fork here, and no `InvalidPreStateHash`
   between peers: the disagreement is between the chain and its own past.
3. **The merge is rejecting the epoch-boundary blocks themselves.** See the decode below. And the
   regression is not a one-off: at height 105 A's pre-state `4bb663bc` is *not* the state of any of its
   four parents (all four end at `3bf78f23`), so the merge's answer keeps moving as the scope moves.

## Decoding `rejectedDeploys` — the epoch boundary is in the rejected set

An entry of the form `X ‖ 0x02` is `sys_deploy_id(block_hash, 2)` — the **`CloseBlock`** system deploy
of block X (`0x01` Slash, `0x03` Empty; `casper/src/merging.rs`, in the `ProcessedSystemDeploy::Succeeded`
match). A block's `rejectedDeploys` field is not its own judgement: it is `pre_state.fringe_rejected_deploys`
(`casper/src/blocks/proposer/block_creator.rs`), i.e. what the **merge** rejected, and validation compares
it against the same set (`casper/src/interpreter_util.rs`).

So, decoded:

| h | rejectedDeploys | what it is |
|---|---|---|
| 103 | `3cc759f6…02`, `4b934ceb…02` | `CloseBlock` of **C#100** and **B#100** |
| 105 | the #100 deploy's sig, `357b06f8…02`, `3cc759f6…02`, `4b934ceb…02`, `c2b97730…02` | the user deploy of height 100, **and the `CloseBlock` of all four height-100 blocks** |

The first entry of height 105's set is the deploy signature itself
(`304402200f7c03fd5c…`, `deploy-status-100-open.json`), which is what tells us the user deploy's chain
was rejected *beside* the four boundary chains — i.e. A's whole height-100 block went down, and the
deploy went with it.

At 103 two boundary chains are rejected and four rounds later all four are, from the same four blocks.
The rejection set is not stable across merges of the same finalized height — which is the property the
fix has to restore.

## What this does not establish

- **Why the node then stopped proposing.** `/api/status` gives the count
  (`consecutiveSelfValidationFailures: 3`) and not the reason; the reason is in node A's log
  (`proposer.rs`'s `Self-created block #N … failed validation: <status>` plus the matching
  `interpreter_util.rs` warn line, which carries both hash pairs). The shapes that would fit are
  `InvalidPreStateHash` — the same non-monotonicity seen from inside one node, a pre-state computed at
  block creation and a different one recomputed at validation — and `InvalidRejectedDeploy`, where the
  *accumulating* fringe rejection set is the thing that moved. **Both are hypotheses here.** The
  capture cannot distinguish them, and the log can.
- **Whether B, C and D report the same.** Only A is reachable through the public endpoint. That A's
  chain shows the identical height-103 pre-state as B's, C's and D's blocks is strong evidence they
  agree, and it is not the same as having read them.
- **That this needs the epoch boundary and not merely concurrency.** The boundary is where four blocks
  write the *same* native slots with the *same* values; that is the shape C207 exonerated by argument
  rather than by test, and this incident is the argument failing. The reproduction (Stage 1) is what
  settles whether a boundary is required or only convenient.
