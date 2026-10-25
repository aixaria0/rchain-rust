# What a joining node trusts

A node with no state restores a prefix of the chain from a peer: it downloads blocks by hash, imports
a tuple-space trie whose pages it checks, and adopts the bootstrap's `FinalizedFringe`. "Trustless"
is a word that hides which of those inputs are actually checked. This page states the model precisely,
and Law 83 (`spec/Rchain/Sync/Trust.lean`) is its machine form.

The headline is **content-trustless, not finality-trustless**: every state byte and every block body a
joiner reads is checked against a hash, so two joiners with the same verified inputs cannot read
differently — but the joiner does *not* verify which root is final, nor the signatures that finalised
it.

## The classification

| Input | Class | What the joiner does |
|---|---|---|
| The configured genesis block and state | **genesis-anchored** | Reads them from configuration; they are the root of trust |
| The genesis `pos:trusted` set (`bond keys ∪ pos-multi-sig`) | **genesis-anchored** | Installed at genesis (`casper/src/genesis/mod.rs`); no later input extends it except a chain-authorised `trust` |
| A downloaded block body | **verified** | Recomputes `Blake2b256` over the body and refuses a mismatch (`validate::block_hash`) |
| An imported trie page | **verified** | Recomputes the node hash against its key, and the walk against the walk listing (Law 70) |
| An ancestry row's `block_hash` | **verified** | Must be a block the walk downloaded; duplicates refused |
| An ancestry row's `fringe` | **verified** | Must equal `message_map::latest_fringe` of the block's justifications — the value a validating node derives |
| A singleton fringe's `state_hash` | **verified** | Must equal the named block's `post_state_hash` (genesis and one-validator chains) |
| Which root the bootstrap calls finalised | **residual** | Assumed; the joiner cannot see the quorum that finalised it |
| A multi-member fringe's per-block `fringe_state_hash` | **residual** | Assumed; the merged state is not recomputed on the join path (**C339**) |
| Liveness (the peer serves the blocks) | **residual** | Assumed; the walk's give-up rule bounds the wait, it does not produce an answer |
| Wire signatures and quorum finality proofs | **residual** | Not checked on the join path |

The residual is `spec/Rchain/Sync/Trust.lean`'s `the_residual_is_exactly_named`: of the nine input
kinds the join consumes, the four that are neither anchored nor verified are exactly the ones listed.
Adding a tenth without classifying it fails the Lean build.

## The proved claim

`join_is_content_trustless`: if two joiners obtain the same verified block map and the same trie, then
at the same claimed root they read the same value for every key and answer the same block lookup. The
proof composes Law 70 (`import_reads_the_rooted_trie`, the trie pages) with `reads_determined`; block
bodies are content-addressed, so a block map that two joiners agree on holds the same bodies.

**Not claimed:** finality. The joiner trusts the bootstrap's choice of root. A proof-carrying fringe
would have to carry the attestations that finalised it, and the join path does not read them; that is a
protocol change, not a join-path check.

## The derivable half and the residual (C338, C339)

The per-block `fringe` **is** derivable from the block's justifications — it is the same
`message_map::latest_fringe` a validating node computes — so the join path refuses a row that disagrees
(`check_derived_fringe`). The per-block `fringe_state_hash` is **not**: it is the merged state of the
fringe, and `close_block` folds it into the epoch seed, so a peer that chooses it can change what a
restored block replays to. The joiner pins it when the fringe names one block; when the fringe names
several, it keeps the claim and logs it. That is **C339**, and it stays open until the join path
recomputes the merge (or the wire carries a proof for it).

The same reasoning applies to the ancestry's shape: a row for a block the walk did not download, or two
rows for one block, is refused before anything is written. A peer that sends **no** ancestry rows is
older, not hostile: the join keeps the pre-#139 per-block WARN fallback rather than refusing to sync.

## The bonding trust axis

Admission and consensus membership are different lists (Law 82, `spec/Rchain/Casper/Lifecycle.lean`):

- `trust`/`untrust` change `pos:trusted`, and the caller must be a trusted key. The caller is bound to
  the unforgeable `deployerId`, so a program-authored key cannot act for another validator.
- `bond` is self-only and requires the bonder to be trusted; it moves the stake into the pool and
  **does not** activate — activation is the epoch boundary's re-selection.
- `untrust` **does not confiscate** (AUDIT C111): a bond is property, admission is not. An untrusted
  validator keeps its bond, stays in the active set until a boundary re-selects, and can withdraw.
- A slash needs evidence (`casper::validate::slashable_senders`); it is the only transition that
  confiscates, and it is graded by the offence.

The genesis trust set is therefore the anchor for admission, and the epoch boundary is the only place
membership moves. The Rust header at `rholang/src/native_state.rs` said the opposite for a long time
("`slash`/`untrust` remove the validator and confiscate the stake"); the comment was the defect.

## What this page does not say

- It does not certify a join. The in-process and devnet arms are finite witnesses; the live chain's
  replay remains the observation that counts.
- It does not close C339.
- It does not claim the bootstrap is honest about liveness: a peer that will not serve blocks fails
  the walk after the give-up rule, which is a bounded failure, not a restored state.
