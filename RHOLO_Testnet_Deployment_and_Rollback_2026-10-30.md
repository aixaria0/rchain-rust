# Testnet deployment and rollback — instructions only

Date: 2026-10-30. No live host was accessed or modified in this investigation.

The release decision and exact candidate are recorded in [the readiness handoff](RHOLO_Testnet_Readiness_Handoff_2026-10-30.md). Do not deploy while a mandatory gate or compatibility check remains unproven.

## Read-only operator preflight

Record every live validator's running source revision, executable SHA256, architecture/libc, service arguments, storage mounts, public identity, network/shard IDs, genesis hash, bonded stakes, epoch/version activation configuration, latest signed sequence/hash and finalized height/hash. Keep private keys out of reports.

Require exact finalized identity agreement, retained finalized markers, readiness, zero unresolved orphan bodies, zero pipeline faults, no unrecoverable records, no proposer halt and zero OOM kills. Record available RAM, cgroup headroom, disk space, database/WAL size and backup capacity. Establish the actual operator service layout before writing commands for it.

Never run the disposable devnet's fresh-genesis or volume-deletion commands against live storage.

## Resources and compatibility

CI requires a 16GB-class runner: at least 14 GiB total RAM and 12 GiB initially available. The eight-node partition fixture uses 1536 MiB per validator. Recovery reserves 2304 MiB for the restarted validator and 1536 MiB for each other validator: 13056 MiB total caps. Preserve the existing restore requirement of at least 2048 MiB cgroup-available memory.

These are fixture budgets, not a capacity guarantee for long-lived live history. Reserve space for a complete consistent backup, the current database/WAL, both binaries/images and incident evidence. Measure restore working memory on an isolated copy of actual storage.

The scheduling correction changes local escape cadence. Block encoding, quorum arithmetic, genesis, bonded stake, liveness window and the finality-lag bound are unchanged. The full integrated candidate also includes the verified recovery changes. Compatibility with the currently deployed live revision remains subject to operator inventory and offline replay.

Compare protocol/version activation rules, genesis identity, network/shard IDs and storage codecs. Replay an operator-approved storage copy without signing keys or live networking using the exact candidate. Verify finalized identity, markers, clean shutdown and reopen. The CI image uses Debian trixie; verify architecture and libc compatibility before using a standalone binary.

## Backup and rollback

1. Preserve the previous image/binary by digest, configuration, service definitions and permissions. Verify backup hashes.
2. Take a database-supported consistent snapshot, or stop one validator before copying its complete data directory and WAL. A copy of an actively changing database is not a verified backup.
3. Protect signing material separately. Record the latest signed sequence/hash and durable equivocation protection state. Never run two signers for one identity.
4. Test restoration to a new isolated, non-signing volume. Keep the original volume intact.
5. Establish whether the old binary can read candidate-written storage before allowing binary rollback.

Prefer rollback to the preserved binary against current storage only after compatibility is established. Otherwise quarantine the validator and preserve current storage/WAL and signing high-water state. Never automatically restore an older signing database: sequence reuse can equivocate. Recover backups first as non-signing observers and reconcile later signed history before re-enabling the identity.

## Ordered rollout for Jim/operator

1. Confirm every mandatory gate on the exact designated source and binary. Complete preflight and offline replay.
2. Keep strictly more than two thirds of original bonded stake online throughout. Seven of eight equal validators is mathematically sufficient; observe actual finality before proceeding.
3. Select one validator, record its finalized marker and signed sequence, make its consistent backup, then stop only that validator.
4. Install the verified image/binary without changing keys, genesis, configuration or storage paths. Verify SHA256 before starting the existing service.
5. Require clean restore and readiness, current block validation, retained finalized marker, exact finalized identity agreement and a fresh local signed proposal above the saved sequence.
6. Upgrade the next validator only after the preceding validator and surviving quorum meet every check. Process availability alone is insufficient.
7. After all upgrades, record every binary/source hash and repeat agreement, marker, finality-progress and resource checks. Preserve live evidence separately from isolated CI.

## Automatic stop conditions

Abort further upgrades and preserve diagnostics on any source/binary/genesis/configuration mismatch; finalized hash conflict or regression; lost marker; replay divergence; missing required evidence; unresolved orphan bodies; pipeline faults; unrecoverable records; failed readiness; OOM or insufficient headroom; disk pressure; proposer isolation/finality-lag halt; repeated self-validation failures; unexpected task exits/panics; absent fresh proposals; signed-sequence reuse; lost original bonded quorum; or failure to progress within the fixed approved observation window.

Stop an affected signer if its state is uncertain. Use the compatibility-aware rollback above. Never clear safety latches, reduce quorum, reset genesis/storage, increase timeouts or raise the finality-lag limit to continue rollout.
