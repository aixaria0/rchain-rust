# Initial LFS reply delivery

Base: PR #392 at 1b73664dfcdbc4cec88b0650ab98ace6dbebd334.

A successful unary request does not establish delivery of the separately streamed finalized-fringe reply. Previously startup stopped requesting after the first successful send; the NodeSyncing retry loop cannot start without any admitted fringe. A dropped response could therefore leave startup waiting indefinitely.

The startup driver now routes packets concurrently with at most ten solicitations. Each send and each response wait is bounded by ten seconds (at most 200 seconds of those waits, excluding scheduler delay). Only an authenticated, nonempty bootstrap target signals admission. Retries preserve the explicit operator anchor, trim setting and metadata request. Admission stops solicitation; completion of the existing full restore remains the only successful transition to Running.

Tests cover successful sends with missing replies, recovery on a second response, a response admitted before polling, hung sends, missing bootstrap, and wrong-peer/empty-target admission. The existing finalizer, stake denominator, trust identity and restore checkpoints are unchanged.

The earlier run 39349348631 is a motivating transport symptom, not proof this is its only cause. Targeted tests and disposable anchored recovery must establish the correction separately. Active validator reactivation and general CBC safety remain independent acceptance obligations.
