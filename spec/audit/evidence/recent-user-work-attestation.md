# Recent user work and stalled attestation — C336

Candidate correction; distributed acceptance is pending. No release authorization.

## Failure evidence

The integrated source d3ccb4c03ecbf09f786c420e8dff9243f1809413 fails the N8 one-validator partition progress bound in runs [39393626848](https://github.com/aixaria0/rchain-rust/actions/runs/39393626848) and [39395723981](https://github.com/aixaria0/rchain-rust/actions/runs/39395723981). The second run's pre-healing raw DAG capture records new user-deploy blocks at every height 70–75, while surviving validators report `nothing_to_finalize=false`, `new_state_transition=false`, `quorum_reachable=false` and `cadence_due=true` at tip 66. Seven surviving validators finish the 300-second window at finalized height 29 from a height-27 start, below the required 30. The proposer timers are not halted; task-panic and self-validation-failure counters are zero. Finalizer refusal remains correct for the observed 100/800 live support.

This identifies a proposer suppression path; it does not claim all liveness causes are resolved. Archive artifact 12251753528 has ZIP SHA256 7588623a1949b0d5d14e1cb3ae1bcb4aee8aaa75ea01d3f367b419813afe1727. Full signed raw bytes are retained, but capture does not independently verify their signatures.

## Correction and finite boundary

The oldest unfinalized work still determines whether work remains. Once that work's prompt licence expires, scan the recent height window for actual user deploys and give them the existing `ATTESTATION_HORIZON` licence. The scan excludes system deploys: repeated CloseBlock work must not renew a minority's prompt licence. Cadence remains required when quorum is unreachable. No finalizer, validity rule, stake denominator or live-weight-set rule changes.

For fixed oldest/fringe floor f and last user-work height u, both prompt licences are false once tip exceeds max(f,u) plus the existing horizon. This is a bound on the prompt licence, not a universal block-count or liveness theorem. The quiet-chain model keeps user deploys separate from epoch system work and consumes the same helper.

## Acceptance

- `cargo test --locked -p rchain-casper --lib recent_user_work_tests`
- Existing quiet-chain, attestation-suppression, round-escape and monotonic regressions must pass.
- Restore the old helper result while preserving the new tests: the fresh-work regression must fail. Record this as fault reintroduction rather than an unmodified historical checkout.
- Rebuild the changed source once. Healthy N8 and two same-volume partition/rejoin cycles must meet the pre-registered criteria, preserve finalized marker/snapshots and retain all failed controls. If the progress arm remains red, this candidate does not close the distributed gap.
