# #293's close condition, measured — a four-validator genesis that includes a deploy and finalises past two epoch boundaries

**What this run is for.** Issue #293's own close condition, verbatim: *"A four-validator genesis on a
build containing #281 can include a deploy, produce blocks, and finalise — verified past **two** epoch
boundaries, not one."* This is a **one-shot outcome run**, not a regression suite: the tree it runs is
`435cf6a59` (the fix), and what it reports is the outcome the defect made impossible.

**The raw collection is [`nodes.txt`](nodes.txt)**, captured live from the running net.

**There are two runs here, and the second one is the one that counts for a merge.** This file's tables
are run 1, at `435cf6a59`. [`agreement.txt`](agreement.txt) is run 2 — **the same run repeated at the
head of PR #297**, which is what a reviewer asks of evidence that describes a tree the PR has since
moved past, and it adds the capture the review of #296 asked for: the four validators shown to agree on
one finalized height, one block hash and one state hash, with the genesis inputs they were started with.
A result measured on a tree that is not the merge candidate is a result about a different tree; the row
that says so is the reason the second run exists rather than an argument about whether the fix's own
lines changed.

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

---

## Run 2 — repeated at PR #297's head, with the per-validator agreement capture

| | |
|---|---|
| image commit | `4ff5732133738a77fb78a3d3fb39600a502376c7` — **this PR's head**, read from each node's own `/api/v1/status` |
| topology | `tools/devnet.sh up --validators 4 --epoch-length 10 --no-autopropose --fresh` |
| epoch boundaries | **10** and **20** |

**The outcome is the same as run 1, on the tree that is actually being merged** — blocks produced, a
deploy included and the state moved (block 158: `deployCount 1`, `preStateHash 77d7f251…` →
`postStateHash 997d820c…`), finality at **161** i.e. far past both boundaries, and the #293 error counted
**0 on all four nodes**.

**And the agreement the review asked for.** Polling all four until the same observation reported the same
number *and* the same hash:

| node | finalized height | block hash | post-state hash |
|---|---|---|---|
| devnet-bootstrap | 161 | `b4f0490b…b8e91` | `03813e66…b90e` |
| devnet-validator-1 | 161 | `b4f0490b…b8e91` | `03813e66…b90e` |
| devnet-validator-2 | 161 | `b4f0490b…b8e91` | `03813e66…b90e` |
| devnet-validator-3 | 161 | `b4f0490b…b8e91` | `03813e66…b90e` |

**Distinct finalized block hashes across the four: 1.** The genesis block (`blockNumber 0`,
`c01a6091…2face`) is byte-identical across them too, and the full bytes, the genesis inputs each
validator was mounted with (`bonds.txt`, `wallets.txt`), and the per-node status lines are in
[`agreement.txt`](agreement.txt).

**What this does and does not say.** It says the four nodes agree about the chain they built, at a
common finalized height, on this PR's tree — which run 1 did not show and which is the property a
testnet deployment turns on. It does not say the *agreement* is a consequence of this PR: agreement is
what a healthy four-validator run does, and the fix's own claim is the one run 1's falsifier carries
(`the_genesis_replays_to_itself`, red on the pre-fix tree with the production error and its count). Two
different claims, two different instruments, and neither is evidence for the other.

**Limit.** One host, one attempt, one tree, `--no-autopropose` (the configuration #293 was reported
under). Autopropose on the same image produces blocks too, but its finality surface reads a stall, which
is the separate finding in `run 1`'s note.
