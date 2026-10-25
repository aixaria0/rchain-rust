# Issue #447: same-channel merge conflict witness

The positive distinct-channel N=4 tests in #448 do not establish what
happens when two signed deploys consume the **same linear datum**.

This bounded test starts four disposable, equal-stake validators under the
merged #448 code. A signed seed deploy publishes a single integer datum
to an ordinary public Rholang channel, and its block is replicated to all
four validators before any network partition.

The test then partitions validators into two internally connected groups
of two. The same signing test key submits one update per group. Each update
must consume the pre-partition datum, produce a distinct updated value,
and publish a distinct marker. Both validators propose independently.
After healing, each validator must receive both diverging tips. The test
requires a multi-parent merge block that all four validators serve with
identical hashes, and records that block's rejection list.

The acceptance rule is intentionally strict: one, not zero or two, of
the two signed deploys must be included in rejectedDeploys, and exactly
one of the two markers may be visible at the pinned merged root.
Unexpected absence, duplicated effects or disagreement are failures.

The signed deploy ID is read from the block's own deploy list, never
assumed from a ProcessedWithSuccess execution status. This test does not
claim the deploy retry or contract-level Root.register arm. Its finite
positive witness cannot replace a real r-cash hot-cell workload.

Run on hosted Docker using:

    gh workflow run devnet-fuzz.yml -R aixaria0/rchain-rust \
      --ref verification/issue-447-same-channel \
      -f mode=issue447-conflict

All containers and volumes are disposable, limited to 2 GiB each on the
hosted CI runner; memory admission at 8-16 GiB remains a separate gate.
No public testnet, bonded validator or production wallet is touched.
