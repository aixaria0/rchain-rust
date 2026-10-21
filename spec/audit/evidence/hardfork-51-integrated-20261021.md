# Integrated acceptance witness — 21 October 2026

Run: [39603467902](https://github.com/aixaria0/rchain-rust/actions/runs/39603467902), source `71b2c69ace4c80a346afd89b6c4f41f7ea7720bc` on #399. This source includes `dev@93dbdf364` and the unmerged #403 attestation experiment. It is not evidence for bare dev or a release authorization. The node image built successfully; the regression arm succeeded; the real-node arm failed.

| Arm | Result | Observed boundary |
|---|---|---|
| Anchored LFS recovery and original-key reactivation | PASS | Two acknowledged initial requests while reply delivery was blocked; 5/5 ancestry/state fingerprints retained; same durable volume; source and recovered node reached finalized height 10 with the same hash; 2 recovered-key proposals; no second LFS reported |
| Healthy N=4 | PASS | Finalized marker retained through two more epoch boundaries; final heights 25,25,24,24 against target 24 |
| Healthy N=8 | PASS | All eight validators retained the marker through target finalized height 24 |
| N=4 partition/rejoin | FAIL | Online group advanced 56→59, satisfying its +3 bound within 300 seconds. The all-validator finalized-marker/recovery condition did not pass within the separate 150-second healing window |
| N=8 partition/rejoin | FAIL | Online group advanced 57→59 against required 60 within 300 seconds. The subsequent healing check passed and retained the marker, but does not erase the failed partition-progress condition |

Neither two-cycle partition campaign completed. The N=4 failure is a healing failure; the N=8 failure is the pre-registered online-progress bound. The logs do not establish one shared root cause. A finalizer refusal with 100/800 or 200/800 supporting stake is not grounds to relax the bonded-stake supermajority rule.

Evidence: [artifact 12348133663](https://github.com/aixaria0/rchain-rust/actions/runs/39603467902/artifacts/12348133663), `final-gap-real-nodes-39603467902`, 17,773,238 bytes, ZIP SHA-256 `b6494e9f201f808fda9a1c074c00e3a7ad08081d2b280bc652804d68a8780887` as reported by GitHub's upload record. Its retention is 14 days under the workflow; archive it before expiry if it is needed longer.

Inspect `final-gap/exit-codes.txt`, the five campaign logs, `n{4,8}-partition/partition-1-live.json`, the causal captures and `lfs-anchor-evidence/reactivation_result.json`. The existing N=4 harness writes no healed causal capture when its wait raises, so missing post-heal capture is a diagnostic limit rather than a successful observation.

Reproduce on a disposable hosted runner using the pinned #399 source and `.github/workflows/devnet-fuzz.yml`, dispatch mode `final-gap`. The workflow builds one exact image, preserves it with a source manifest, runs the scoped arms and uploads their distinct exit codes. Do not use the separate `final-gap-recovery` mode as if it rebuilt this source: that mode explicitly loads an older pinned image.

Next falsifiers: reproduce N=4 catch-up with complete post-heal accepted-DAG/operator snapshots, and locate the missing N=8 supporting layers under the existing 300-second bound. Keep the threshold, quorum and finalized-state rules unchanged while isolating liveness behavior.
