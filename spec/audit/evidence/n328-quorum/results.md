# The post-mesh `quorum_reachable` reading — #315's second clause, and it holds

**The clause.** #315's close condition asked for *"no `quorum_reachable=false` persisting after the mesh
is complete"*. The two runs that closed the issue captured the **API** surface — finality advancing past
the kill, every validator attesting — and `quorum_reachable` is not on that surface: it is computed in the
proposer (`casper/src/blocks/proposer/proposer.rs`) and rendered only into a log line, rate-limited to
fire on change and then every 100 tips. So the clause was a reading of a proposer-side line, and this is
the run that reads it.

**The reading: it holds.** Three validators, `--no-autopropose --propose-on-deploy`, deploys to drive
production. Final heights **19/19/19**, finalised **15**. Of the **16** collected
`attestation at tip N: …` lines, **14 read `quorum_reachable=true`** and **2 read `false`** — and both
`false` lines are the **genesis round**:

```
attestation at tip 0: withheld — … quorum_reachable=false … round=[04f700a4]
attestation at tip 1: licensed — … quorum_reachable=false … round=[04f700a4]
```

`round=[04f700a4]` is a **one-sender** round, and `quorum_reachable` is `is_super_majority(live stake +
own stake, the whole bonded map)` — so on a one-sender round there is no supermajority to reach *by
construction*, which is exactly what `n213-results.md` recorded when the first cut read `false` there
("the round is its genesis alone, the exact state in which the first cut read `quorum_reachable=false`").

Every line from the moment the round is the full three-sender set reads `true`:

```
attestation at tip 18: withheld — nothing_to_finalize=true  new_state_transition=false quorum_reachable=true cadence_due=false paced=true round=[04d8b6c3 04dbe32c 04f700a4]
attestation at tip 6:  licensed — nothing_to_finalize=false new_state_transition=true  quorum_reachable=true cadence_due=false paced=true round=[04d8b6c3 04dbe32c 04f700a4]
attestation at tip 5:  withheld — nothing_to_finalize=true  new_state_transition=false quorum_reachable=true cadence_due=false paced=true round=[04d8b6c3 04dbe32c 04f700a4]
```

So there is **no `quorum_reachable=false` persisting after the mesh is complete**: the only falses are the
bootstrap's own first round, where the flag is answering a question about a round with one participant.

**What this is and is not.**

- It is a **positive witness** for the clause: after the mesh, on every node, the flag reads true, and the
  two falses are named and explained rather than excluded.
- It is **not** a statement about the N=8 shape (`A1.3`), which is a different failure with its own row:
  there the refusal is `400 of 800` **with all eight validators sending**, which is a *stake*, not a round
  with one sender.
- **Not a general health check.** The clause is about the flag not *persisting* false; a chain that is
  genuinely short of a supermajority should read false, and A1.3's witness says what that looks like.

**Rig and raw material.** `n328-quorum-run.sh` in this directory (three validators, `--stakes
100,100,50`, `--epoch-length 10`, `--no-autopropose --propose-on-deploy`; a fresh four-validator
autopropose net was deliberately *not* used, because it self-diverges into the §92 self-reject stall).
The raw capture is [`run.txt`](run.txt) and the collected lines are
[`attestation-lines.txt`](attestation-lines.txt).

**One host, one run.** Three containers on this box; the heights and the timings are this machine's.
