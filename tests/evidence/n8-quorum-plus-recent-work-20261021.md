# N=8 paired production discriminator (integration experiment)

**Experimental fork branch only, not upstream merge or release approval.**

The unchanged canonical N=8 campaign still fails on the isolated quorum-clock
candidate [run 39639570219](https://github.com/aixaria0/rchain-rust/actions/runs/39639570219):
baseline `n8=0`, partition `n8-partition=1`; 57 -> 59 instead of >=60
within 300 seconds. The failure-state captures show seven online senders
in the liveness partition and `quorum_reachable=true`, but the finalizer
still reports 100/800 support among seven candidates, while automatic
attestations are frequently withheld with no prompt user-work licence.

This experiment combines two distinct production mechanisms:

1. #407: quorum-backed live-set/round clock, with full bonded quorum intact.
2. #403: bounded recent-user-deploy attestation licence, carrying the user
   workload beyond the expiry of older unfinalized work.

It cherry-picks the original #403 implementation and test commits
(`e85bb962d` and `0422e5d5a`) onto the #407 candidate at
`940165cd0`. No acceptance harness, validator count, partition route,
300-second budget, finality threshold, stake denominator or public-network
configuration is modified.

A successful campaign on this combination would identify an integration
candidate, **not** establish that either individual PR is independently
sufficient. Run canonical source-pinned N=8 with unchanged two partition
cycles and repeat relevant Rust regression, replay, recovery and anti-storm
tests. Any failure remains a failure.
