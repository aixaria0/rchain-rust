# #293's close condition, measured — a four-validator genesis that includes a deploy and finalises past two epoch boundaries

**What this run is for.** Issue #293's own close condition, verbatim: *"A four-validator genesis on a
build containing #281 can include a deploy, produce blocks, and finalise — verified past **two** epoch
boundaries, not one."* This is a **one-shot outcome run**, not a regression suite: the tree it runs is
`435cf6a59` (the fix), and what it reports is the outcome the defect made impossible.

**The raw collection is [`nodes.txt`](nodes.txt)**, captured live from the running net.

## The configuration

| | |
|---|---|
| image commit | `435cf6a59b05bb958fd14c21c081de604b23c586` (the fix, read from the node's own `/api/v1/status`) |
| topology | `tools/devnet.sh up --validators 4 --epoch-length 10 --no-autopropose --fresh` |
| genesis | fresh (`--fresh`), created by the bootstrap from the standard ceremony |
| epoch boundaries | **10** and **20** |

## The outcome

| what #293 said could not happen | what the run did |
|---|---|
| *no block is produced* | `latestBlockNumber` **28** at the first reading, **35** at the second |
| *no deploy is ever included* | a deploy **included at block 28**, `preStateHash 79af9d56… ≠ postStateHash a4cef7bd…` — the deploy executed and moved the state |
| *finality never starts* | last finalized **24** (past both boundaries), then **31** — it is *advancing*, not frozen: a frozen chain reads the same finalized height every time |
| *`failed to regenerate mergeable channels` on every propose* | counted **0 on all four nodes** |

The error #293 is named for does not occur because the write it named is now attributed: the genesis
window is held across the deploy loop, so the post-deploy alias re-seeds belong to the genesis rather
than to nobody.

## Why this is evidence and not a green suite

The suite was green before the fix too. A test cannot witness an improvement. What makes this a
falsifier is that **the pre-fix tree produces a different outcome on the same rig** — and that half is
witnessed in-process, in the same commit, by `the_genesis_replays_to_itself`
(`casper/tests/genesis_registry.rs`), which on the pre-fix tree fails with that production error and its
exact count (`17 native write(s) outside any deploy's window`) and on the fixed tree passes. The live
run is the same claim at network scale.

**Limit.** One host, one attempt, one tree. `--no-autopropose` is the configuration #293 was reported
under, and it is the one used here; an autopropose run on the same image also produced blocks
(138) without the error, but its finality surface read a stall, so this file reports the
`--no-autopropose` arm only.
