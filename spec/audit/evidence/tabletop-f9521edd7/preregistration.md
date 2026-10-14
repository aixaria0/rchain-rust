# Red-team tabletop — pre-registration

**Reviewed revision.** `dev` at `f9521edd7` (merge of #357). Every `file:line` below is **Control's
prep, for the world only** — the players re-derive what they rely on, and a player who cites this file
instead of the tree has produced an *undecided*, not a finding.

**What this is.** A **tabletop**: Control writes injects on a clock, a cast answers them in role, and an
independent evaluator rules on every claim — including the defenders'. It is the complement of what the
two prior passes could do, and they say so of themselves:

- the wargame over the code (`docs/src/spec/adversary.md`): *"It **cannot supply a rate**"*;
- the acceptance page (`docs/src/spec/testnet-acceptance.md`): *"a liveness and membership verdict, not a
  safety verdict"*, and §5 — *"a warning against adding one more unchecked register."*

This file is committed **before any agent runs**. The budget is fixed here so that every price the
exercise reports is derived against a number, not against what the code happens to allow.

---

## 1. The world (Control's prep)

**The net.** RChain (`rchain-rust`), a bonded-validator L1. Public testnet as the platform, standing in
for a production deployment: four validators, equal stake, the assumption of at most one Byzantine
(`docs/src/spec/adversary.md §The model`). The testnet *"holds no value, and its chain can be reset at any
time"* (`docs/src/node/testnet.md`) — so the money layer is **played in role**, and that is stated rather
than hidden.

**What is actually exposed.** From off-host, default config, an anonymous party reaches exactly two
surfaces: the deploy gRPC on `0.0.0.0:40401` (decode cap 16 MiB) and the public read HTTP on
`0.0.0.0:40403`. Propose/repl gRPC (`40402`) and the admin HTTP (`40405`) are loopback-bound; the peer
and discovery transports (`40400`/`40404`) are mutual-TLS. **Nothing on any surface checks a credential** —
the only authenticators in the tree are a loopback bind and a peer certificate.

**What an anonymous party costs the node.** Admission is gated by: a signature check that any key passes,
`min-phlo-price = 1`, a ≤64 KiB term (`casper/src/multi_parent_casper.rs`), a per-deployer rate of
`100/s` keyed on the deployer *public key* (`casper/src/dag.rs`), a per-deployer pool share of 1 000, and
a node-wide pool of 10 000. **The sender is charged nothing at admission** — phlo moves only if the deploy
is included in a block. Identities are free. There is **no peer reputation anywhere**.

**The money.** Bond minimum 1 drop, permissioned (a key must be in `pos:trusted` to bond). Rewards are
paid only from burned deploy phlo; `executor-share` is 25 %; an absent validator is paid the same as a
live one — there is no inactivity leak and no decay. Slash tiers: `Malicious` 100 %, `Misdemeanour` 25 %,
`HonestMistake` 10 %, taken from the offender's **whole** PoS position (bond + escrowed withdrawal +
accrued rewards). Equivocation **is** slashable, and the evidence path is fully wired — but the map that
records a second block at a `(sender, seq_num)` is **in memory, so a restart forgets it**
(`casper/src/dag.rs`).

**The operators.** Validator keys are a file: a 64-hex scalar or an unencrypted PKCS#8 PEM, read
owner-only (any group/other bit is refused, on the read descriptor). The node's shipped key generator,
`rnode keygen`, writes an **encrypted** PEM — which the node's own loader refuses. The live systemd units
exist only on the hosts, not in the tree.

## 2. The cast

Each player is a **mandate**, not a persona: the role constrains what the player may know and do.
**Control** (the orchestrator) keeps the clock and reads the injects. **Mallory**, the adversary, holds
the budget — played through seven lenses. The defenders: the **median validator operator** (not the core
dev; woken at 03:00; has systemd and the docs); the **incident commander**; the **core engineer**; the
**custodian** (an exchange or large holder, seeing only what is public); the **release engineer**; and the
**challenger** — the role the acceptance page names (`:90-101`), whose only duty is to falsify.

## 3. The adversary's budget (fixed before play)

Mallory may spend, in total: one mid-size cloud VM for a month (~EUR 40) plus ~EUR 200 burst; up to
10 000 REV at spot (the faucet drips 0.3 REV per *address*, and addresses are free — a bond is the
expensive thing and is **not** in this budget unless a scenario grants a validator); 20 free identities;
unlimited loopback addresses; 200 engineer-hours; all public information.

**Out of budget:** a corrupt validator key (except where a scenario grants one), host access to a
validator, a dependency zero-day, a coerced operator, post-quantum capability.

## 4. Calibration

Six seeded controls are mixed into the evaluators' queues **unlabelled** — two `known-true` (must return
`CONFIRMED`), two `false` (must return `REFUTED`), two `out-of-model` (must return
`NOT-RE-ESTABLISHED`). **Calibration = caught / planted**, computed by the orchestrator from the
evaluators' verdicts, never asserted by an agent. **A run whose evaluators confirm a `false` control or
refute a `known-true` one is theatre, and is rerun.** The wargame's first two pilots each over-confirmed a
control; the number is quoted every run for that reason.

## 5. Falsifiers — the run is void unless

1. **The budget was fixed before play** (this file, committed first), and every price is derived against it.
2. **Calibration holds** — the caught/planted count is reported, and the controls are unlabelled in the queue.
3. **Every capability claim carries a re-derived `file:line`.** An ungrounded claim is an *undecided*, not a finding.
4. **The defenders are contested as hard as the adversary** — the counts are reported for both.
5. **The timeline is reconstructed from the tree**, not from the players' prose.
6. **What could not be run is counted and labelled**, never dropped silently.

## 6. Evidence shape (this directory)

```
spec/audit/evidence/tabletop-f9521edd7/
  preregistration.md      # this file — committed before any agent ran
  run.sh                  # stage A: brief, timeline, play, contest, measure-author
  run-after.sh            # stage B: the after-action, given the probes' results
  stage-a.json            # stage A's return value, written by the orchestrator
  journal.jsonl           # every agent's return value
  probes/<id>-run.sh      # the authored probes, run serially by the orchestrator
  probes/<id>-run.log.txt # their captures (output under target/, never /tmp)
  timeline.md             # control's escalation as played
  undecideds.md           # what the system does not decide — the tabletop's unique output
  prices.tsv              # per attack, cost against the budget
  verdict.md              # the readiness verdict, with dissent
  provenance
```

Timestamps are stamped **after** the workflow returns — scripts cannot call `Date.now()`.
