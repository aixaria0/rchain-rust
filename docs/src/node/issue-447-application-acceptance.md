# Issue #447 — application-state acceptance boundary

This is a source-pinned verification recipe for [#447](https://github.com/rhovision-org/rholo/issues/447). It does not claim that a public network has been upgraded. #434/C336 changes the merged state hash and is a hard fork.

## What is and is not established

| Claim | Existing witness | Disposition |
| --- | --- | --- |
| Single-returner partition/rejoin | #444 exact-head N=4/N=8 canonical campaigns | Passed, but not a 2+2 divergent application merge |
| C336 affected-channel diff | #434 regression tests | In-process, not a public network |
| Native-state durability and admission | #446 independent checks | Scoped to their exercised cases |
| Independent writes across a 2+2 split | tools/n420-fork-point.py | Requires a live run with both sides internally connected |
| Native registry lookup returning Nil at a pinned root | #447 report | Requires same-block/same-URI reproduction |
| 17-step hot-cell burst and realistic memory | #447 matrix | Not covered by #444 canonical campaign |

## Two internally connected pairs

The prior fork-point script disconnected validators 2 and 3 from the only Docker network. Those validators were then disconnected **from each other** as well. This does not represent two communicating pairs, so a missing merge in that setup cannot establish a Casper failure.

tools/n447_partition.py attaches validators 2 and 3 to a second temporary bridge **before** removing their original network membership. Validators 0 and 1 remain on the main bridge. The helper verifies the complete memberships, rejects cross-side links, and restores the original topology even if the workload fails. Failure to restore is a failed test, not a pass.

tools/n420-fork-point.py now uses that helper directly and incrementally saves topology, fork hashes and merged-state readbacks to fork-point.json. An incomplete result is never a pass. It also probes actual peer HTTP reachability within and across both pairs. After rejoining, it checks that every node has both tip hashes before attempting the multi-parent merge. A gossip failure is recorded separately from a proposal/merge timeout with per-node status evidence. The offline tests cannot substitute for this live observation.

For a disposable Docker host with sufficient RAM:

    DEVNET_PREFIX=n420fork DEVNET_NODE_MEMORY=8g \
      tools/devnet.sh up --validators 4 --fresh --no-autopropose --epoch-length 10

    DEVNET_PREFIX=n420fork python3 tools/n420-fork-point.py \
      --out target/issue447/fork-point --timeout 600

    DEVNET_PREFIX=n420fork tools/devnet.sh down -v

An independent application harness may use the network wrapper:

    python3 tools/n447_partition.py --prefix n420fork \
      --out target/issue447/network.json -- python3 your-application-harness.py

The wrapper checks **Docker network isolation and restoration only**, not consensus correctness or successful peer rediscovery. Its JSON does not serialize command arguments or keys. A nonzero workload exit remains nonzero.

Do **not** execute these partition commands on the public testnet. The fresh disposable devnet is explicitly destructive to its own data. Do not start it with a prefix that collides with an existing test network.

In a four-validator equal-stake 2+2 split, each side has 50% of stake; neither side should be required to finalize before reconnecting. Distinct unfinalized tips may exist. Acceptance requires a later block justifying both tips, correct merge and rejection decisions, and converged finalized state after healing.

## Historical lookup: required falsifier

POST /api/v1/explore-deploy-by-block-hash reads a **block hash** (not a state hash). When usePreStateHash is false, casper/src/api/block_api_impl.rs chooses that block's post-state and starts an isolated exploratory runtime there. Native rho:registry:lookup is not equivalent to querying ordinary tuple-space data-at-name.

To diagnose the Nil observation, preserve the exact URI, query term, named block hash and number, that block's pre/post-state hashes, the registration deploy's inclusion and finality, and complete JSON replies (no credentials). Compare the signed lookup with an exploratory lookup at the **same committed state**, not an unpinned tip. A Nil result at a block preceding registration is not a defect. A mismatch at an identical post-state is a valid regression candidate.

## Six separate application acceptance arms

1. **Distinct channels:** both divergent tips explicitly justified; both effects at merged post-state, neither at the common base; identical hashes/readback on all nodes and after restart.
2. **Same channel:** exactly the expected rejected deploy, no partial effect, correct rejection list and state, successful fresh-signature retry. Executed status alone does not imply inclusion.
3. **Seventeen deploys:** compare uncontrolled burst with inclusion-checked installation; record individual IDs, blocks, and monotone committed keys.
4. **Restart:** crash/replay preserves anchored state; CURRENT_ROOT names an accepted block.
5. **Admission:** run with 8–16 GB per validator, inspect 429/503/Retry-After, pool bounds, expiry and resubmission; no OOM or silently lost accepted deploys.
6. **Lifecycle:** record submission, execution, block inclusion, finality, and resulting state separately.

Publish source SHA, image digest, RAM, bond set, genesis, block IDs, pre/post-state hashes, rejections, timestamps, raw JSON, and failed attempts. A finite disposable-network pass is not mainnet approval. Deployment of a hard-fork binary requires its own explicit operational authorization.

## Execute the real four-validator campaign in GitHub Actions

A source-pinned disposable-Docker campaign is available through the existing
devnet fuzz workflow (workflow_dispatch input: mode=issue447-n4). This job
builds the real node image from the selected branch, starts four validators
on a uniquely named network, drives the 2+2 split/fork/healing witness,
captures evidence on failure, and tears down disposable resources.

    gh workflow run devnet-fuzz.yml --repo aixaria0/rchain-rust \
      --ref verification/issue-447-partition-topology \
      -f mode=issue447-n4

A failed run is NOT a pass. The runner caps validators at 2 GiB each;
this is not the 8-16 GiB capacity acceptance required for #447.
