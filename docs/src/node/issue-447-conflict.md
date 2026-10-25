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
does **not** require those original tips to appear as direct justifications
of a later block. In a DAG, the conflicting effects may be merged only after
one or more further proposal rounds. The test follows bounded, verified
justification paths from a rejection-bearing block back to **both** original
fork tips. A rejected deploy signature alone is not a passing witness.

The selected merge successor must be served with identical block, pre-state,
post-state hashes and pinned application state by all four validators; the
pre-fork cell and marker channels are negative controls. The cell must contain
exactly the surviving update's expected integer, while the opposite marker
must be absent. One validator is then restarted and the exact same pinned
block, roots and readbacks must be preserved.

The acceptance rule is intentionally strict: exactly one of the two
signed conflicting deploys must be included in the candidate block's
rejectedDeploys, and exactly one of the two markers may be visible at the
pinned merged root. Unrelated rejections are recorded but do not count
toward the conflicting-deploy verdict.
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


## False-positive controls added before merge

The first conflict witness used a read-only exploratory deploy that
consumed and returned one cell value. That cannot establish cardinality:
if a faulty merge leaves both the original zero and the accepted writer's
new value, an exploratory read can return only the new value and conceal
the duplicate. The harness now uses the pinned data-at-name-by-block-hash
API for the linear cell and both marker channels, and requires the reply's
block hash to match the requested block. Duplicate values, malformed
values and error envelopes fail closed.

An additional DAG check independently traces each signed branch to the
actual included seed hash before healing. Greater block heights alone
do not establish a common causal ancestor. The original strict checks
for branch-local values, merged-root rejection and four-node readbacks
remain mandatory.

These tests do not constitute a passing real-N4 verdict. The exact source
commit must pass a fresh disposable 2+2 campaign with raw witness before
this pull request is eligible for merge.
