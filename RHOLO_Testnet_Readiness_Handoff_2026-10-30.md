# Testnet readiness handoff

Date: 2026-10-30. **NO-GO: mandatory candidate gates are not yet independently established.**

Candidate source: b26bda1a1aa8a1adac9fc9146a712b9d9c8dc3a9.

[Candidate CI](https://github.com/aixaria0/rchain-rust/actions/runs/40987327326). The workflow packages one binary image for all acceptance arms; its actual SHA256 must be read from the completed verified-node-image artifact. No binary hash is asserted before that evidence exists.

| Gate | Current qualification |
|---|---|
| A. N8 partition/rejoin | Pending on candidate |
| B. Two same-volume restarts and fresh proposals | Pending on candidate |
| C. Quiet finality 8/8 | Pending on candidate |
| D. Quiet finality 7/8 | Pending on candidate |
| E. Exact finalized identities, retained markers and readiness | Pending on candidate |
| F. Zero unresolved orphans, pipeline faults, OOM and irreversible divergence | Pending on candidate |
| G. Safety regressions | Offline quiet harness: 6 passed; hosted Rust verification pending |
| Independent repetition | Two repetitions configured; results pending |

The earlier [recovery pass](https://github.com/aixaria0/rchain-rust/actions/runs/40983016920) belongs to 20ef2505ffed761cdf1d2b4330f1ec1f326f0eec and cannot substitute for candidate recovery results. The [original quiet failure](https://github.com/aixaria0/rchain-rust/actions/runs/40980671027) and [integrated control failure](https://github.com/aixaria0/rchain-rust/actions/runs/40986341277) remain preserved.

The scheduling correction is a candidate remedy for a proven repeated-escape defect. Exact final support-map attribution and successful final acceptance remain unproven until archived causal replay and candidate CI complete. No claim of live-testnet readiness follows from isolated success. Live configuration/genesis/storage compatibility and offline replay of operator-provided storage remain separate preconditions.

See [RCA and fix](RHOLO_Quiet_Finality_7of8_RCA_and_Fix_2026-10-30.md) and [operator deployment/rollback instructions](RHOLO_Testnet_Deployment_and_Rollback_2026-10-30.md). The operator document is instructions only; no live host was accessed or changed.
