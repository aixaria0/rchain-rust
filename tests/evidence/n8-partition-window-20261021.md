# N=8 partition: proposer concentration and live-window discrimination

Status: diagnostic candidate, **not consensus acceptance or a production fix**.

## Source and observation

The historical disposable-node capture for
[run 39613912681](https://github.com/aixaria0/rchain-rust/actions/runs/39613912681)
was built at source `d0925f6c731e4b04e34ca191aefc97733b2ccd13`. Its
`n8-partition/partition-1-before-healing/causal/node-0.json` inventory
contains accepted blocks and justified parents. The campaign failed the
pre-registered online-progress test (58 to 59, target >=61 in 300 seconds).
All eight later reported finalized height 59 with one hash after healing.
Neither observation clears the release blocker.

For accepted blocks at heights >=61, replaying the *input liveness-window
calculation* on that node's captured parent sets gives:

- 106 accepted blocks: 46 from sender `04f700a41775...`, ten each from
  six further active senders. The isolated sender `0494961fa32a...`
  stops at height 60.
- That leading sender reaches height 106; six others reach 90.
- 57 of those 106 block parent sets contain live stake not exceeding two
  thirds of the whole bonded map. In late blocks 95--106, only one 100-stake
  sender is within the five-height freshness window out of 800 bonded.

This reconstructs **partition membership inputs**, not next-fringe support,
signature validity or a finalizer verdict. A one-sender live subset has
insufficient bonded stake even if its single candidate meets the coverage
condition. Do not change the total-stake denominator.

## Falsifiable next discriminator

`tools/consensus-acceptance-drill.py` historically directs each periodic
signed `Nil` workload deploy to validator 0, every five seconds. This is
valid bounded work but concentrates proposals on one peer. The original
`recovery_case=n8` remains completely canonical.

A separate **diagnostic-only** `recovery_case=n8-rotating` preserves the
same N=8 genesis, source/image, partitioned validator, two bounded
partition/rejoin cycles, marker checks, volume retention, and unchanged
+3 finalized-height threshold within 300 seconds. It changes only the
destination of the periodic signed deploys *after baseline acceptance*:
round-robin over validator indices 0..6 (index 7 is isolated).

For a proper A/B, dispatch both modes at the same commit, retain both
artifacts, record producer-height distribution and derived parent live
sets, and inspect whether finality progresses. A green rotating run
**does not** pass the canonical one-sender workload or certify release
liveness. If both arms fail, revisit parent/justification/finalizer traces
rather than weaken quorum or increase the timeout.

## Read-only reconstruction

```sh
python3 tools/consensus-window-diagnostic.py \
  --capture path/to/partition-1-before-healing/causal/node-0.json \
  --min-height 61 --output /tmp/n8-window.json
python3 -m unittest tests.test_consensus_window_diagnostic \
  tests.test_consensus_partition_workload -v
```

The tool refuses absent justification dependencies, malformed bonds and
empty input. It counts potentially live sender stake using the
source-defined five-height bound; it does not emulate Casper CBC,
authorization or finality and must not be read as release acceptance.
