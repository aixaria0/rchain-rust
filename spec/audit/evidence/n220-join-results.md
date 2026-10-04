# n220 — a validator joins a running net with no autopropose: results

Rig: [`n220-join-run.sh`](n220-join-run.sh). Artefacts: `n220-join-blocks/9a75d45-20261004T005453Z/`.

**Tree `9a75d45`** — #219's head: C209 (the attestation guard reads what the node has seen) and C210 (a
not-due propose is retried by the node itself). Binary built natively (`sha256 0aab26a6…`), image
`rnode:local`. Net: 3 validators at 100/100/50, `--no-autopropose --propose-on-deploy`,
`--epoch-length 10`. Newcomer: `tools/devnet.sh` key 3 (`04ea3ce0…`), started with `reset 3` (synced,
unbonded), funded and trusted by the genesis deployer (`examples/join-admit.rho`), bonding 50 itself
(`examples/join-bond.rho`).

**Result: every condition passes.**

| | condition | observed |
|---|---|---|
| J1 | the bond lands | admit `ProcessedWithSuccess` in block 5, bond in block 9; block 10 (the boundary) is the first whose `bonds` name the newcomer |
| J2 | the newcomer produces a block once active | it produced block 10, and 14 of the 84 blocks in the run |
| J3 | a deploy sent to the newcomer finalises | its block 18 is finalised (`/api/is-finalized` true), witnessed by the deploy's own status, not a height |
| J4a | finality advances past the activation boundary | finalised 20, four behind the tip at 24 |
| J4b | the chain is quiet afterwards | height 24 → 24 over the 90 s read; no round-gate escapes on any node |

**Two runs before this one are not results, and are kept out of the table on purpose.** The first funded
the newcomer with 1,000 and its bond deploy failed with `preCharge: insufficient funds (1000 < 1000000)`
before `bond` was called — a deploy is precharged `phlo-limit × phlo-price`; `join-admit.rho` now sends
100,000,000. Its J3 "pass" was also unsound (finality passing a height is not the deploy's inclusion),
which is why J3 now reads the deploy's own status. The second activated the newcomer and passed J1, J2
and J4, and its J3 reported a false FAIL because the rig read the status key `Processed` where the API
says `ProcessedWithSuccess`; that deploy's block was checked by hand and was finalised.

**Limits.** One run, one host, no delivery delay, a newcomer that stays up. Not measured: a newcomer that
bonds and then never speaks (the silent-stake case `running-a-public-testnet.md` §4 warns about), a
withdrawal, or a join on the public testnet's stake shape.
