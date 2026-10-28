# Independent development continuity

This repository is an independent development and verification lane for the public Rholo source tree. It does not replace the upstream maintainers, review process, or protocol acceptance gates.

## Source separation

- `dev` is the account owner's working branch. Never reset or force-push it during synchronization.
- `snapshots/upstream-dev` is a read-only reference in intent to the last **fast-forward verified** upstream `dev` commit. Only the snapshot workflow may advance it; any non-fast-forward change must fail visibly.
- Experimental branches derive from an immutable source SHA and retain their own work. They may be developed, tested, and published independently.
- Changes intended for Rholo are proposed as ordinary upstream PRs. No workflow in this lane merges or deploys upstream code.

## Independent evidence

Run **Independent upstream PR verification** manually with an upstream PR number. The workflow fetches its exact head SHA, checks formatting, and runs the storage and consensus library tests with a read-only token. Its uploaded artifact records the tested SHA and outcome. A failure is not silently converted to a pass; a pass is not permission to merge.

The upstream repository's canonical CI, source-pinned regression evidence, safety invariants, protocol compatibility checks, and maintainer approval remain required for integration. Use the same base SHA and fixture when comparing different implementations.

## Review continuity proposal for maintainers

- Reviews address code, reproducible failures, invariant violations, security boundaries, or missing evidence. Every blocking technical claim identifies a file/line, expected behavior, and reproduction or proof obligation.
- Reviews and decisions are recorded on GitHub. A change of scope or acceptance criteria becomes a distinct tracked proposal rather than retroactively invalidating an unrelated verified fix.
- A PR without substantive feedback after 72 hours can be escalated to an **alternative authorized reviewer** chosen by the maintainers; elapsed time never bypasses required checks or approvals.
- No single named contributor is the sole required reviewer across all changes. The maintainers should maintain a pool of qualified reviewers, with code-owner coverage for sensitive consensus, cryptography, genesis, and state-transition changes.
- Governance disagreement does not stop independent research, unit tests, or work on another branch. It also does not authorize bypassing project governance for merges or production deployments.

## Requested upstream branch protections (administrator action)

The upstream owner should configure `dev` to disallow force pushes/deletion, require the relevant successful CI checks and resolved review threads, and require approval from a qualified reviewer pool. Give at least two maintainers authority to handle stalled reviews. Any emergency bypass must be recorded with a specific incident, rollback path, and post-incident review.

These protections are **not** installed by this document. They require administrator approval and configuration.

## Operational separation

No automated synchronization or verification task accesses Validator signing keys, modifies live state directories, updates production services, or grants deployment privileges. Keep staging and production promotion as explicit, auditable, separately authorized operations.
