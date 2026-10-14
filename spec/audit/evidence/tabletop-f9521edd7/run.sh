// The red-team tabletop — stage A: brief, timeline, play, contest, measure-author.
//
// The exercise is a tabletop, not a checklist: Control writes injects on a clock, a cast answers them
// in role, every capability claim is grounded in the tree or admitted ungrounded, and an independent
// evaluator rules on each claim — including the defenders'. The register is bracketed: no player reads
// it, because a register row is not evidence.
//
// Stage B (`run-after.sh`) synthesises the after-action once the probes this stage authors have been
// run, serially, by the orchestrator.
//
// Run with Workflow({ scriptPath: '<repo>/spec/audit/evidence/tabletop-f9521edd7/run.sh' }).

export const meta = {
  name: 'redteam-tabletop',
  description: 'Red-team tabletop: budget the adversary, play the cast against a timeline of injects, contest every claim, author the probes that settle it',
  phases: [
    { title: 'Brief',    detail: 'adversary lenses, independent and budgeted' },
    { title: 'Timeline', detail: 'Control writes the injects from the ranked targets' },
    { title: 'Play',     detail: 'the cast answers each inject, grounded in the tree' },
    { title: 'Contest',  detail: 'an independent evaluator per inject, plus seeded controls' },
    { title: 'Measure',  detail: 'author the probes for claims that reduce to a run' },
  ],
}

const REPO = '/home/patrick/RNodeRust/.claude/worktrees/redhat-audit'

// The register is bracketed from every player. This is not a courtesy: the wargame's first two pilots
// leaked it into the referee and over-confirmed a false control.
const BAN =
  'Do NOT read spec/findings.tsv, spec/AUDIT.md, spec/audit/passes.md, docs/src/spec/adversary.md, ' +
  'docs/src/spec/testnet-acceptance.md, or docs/src/node/security-audit.md. You are a participant in an ' +
  'exercise, not an auditor reading a register. A register row is not evidence and must not shape what ' +
  'you say. Read the source, the tools, the docs and the config, and reason from those.'

// The adversary's budget. Fixed in the pre-registration, before any agent ran, so that every price is
// derived against a number rather than against what the code happens to allow. Criterion 3's statement
// has no budget, which is why its census has no denominator; this is the denominator.
const BUDGET = [
  'Mallory may spend, in total, across the whole exercise:',
  '  · one mid-size cloud VM for one month (~EUR 40), plus a small burst allowance (~EUR 200);',
  '  · up to 10,000 REV if it can be obtained at spot (the faucet drips 0.3 REV per address and',
  '    addresses are free, so REV is cheap; a bond is the expensive thing and it is NOT in this budget',
  '    unless a scenario says the adversary is a validator);',
  '  · 20 free identities and unlimited loopback addresses;',
  '  · 200 engineer-hours of one competent attacker;',
  '  · all public information: the source, the docs, the running testnet, the bootstrap peers.',
  'Out of budget: a corrupt validator key (except where a scenario grants one), host access to a',
  'validator, a zero-day in a dependency, a coerced operator, post-quantum capability.',
].join('\n')

const WORLD = [
  'THE NET. RChain (rchain-rust) is a bonded-validator L1. The world of this exercise is the public',
  'testnet (docs/src/node/testnet.md) plus the production deployment it stands in for: a small set of',
  'validators, each run by a separate operator on their own host, bonded stake, a PoS contract, a',
  'registry, a vault, a faucet, a gRPC deploy ingress, a public read HTTP API, an admin HTTP listener,',
  'and a set of native contracts in Rust. Validators propose blocks on a cadence; finality is by',
  'attestation with a stake quorum (synchrony-constraint-threshold 0.67). The public testnet holds no',
  'value and can be reset at any time; where the world has no value, the money layer is played out in',
  'role, and that is stated rather than hidden.',
  '',
  'THE QUESTION. Do not ask "is there a bug". Ask: when this system is attacked and operated, what',
  'actually happens, what is left undecided, what does the adversary pay, and should anyone run it.',
].join('\n')

// The cast. Each is a mandate, not a persona name: the role constrains what the player may know and do.
const CAST = [
  ['operator',
   'You are the median validator operator, not a core developer. You run one node on the public ' +
   'testnet on your own host, under systemd, with the docs open. You generate and hold your own key. ' +
   'You were woken by a pager at 03:00 and you have a terminal, no special build, and no access to ' +
   'anyone else\'s host. You do not know what a fringe is.'],
  ['incident-commander',
   'You are the incident commander. You own the response: you decide what to halt, who to tell, when ' +
   'to escalate, and when to declare the incident over. You have the validator operators, the core ' +
   'engineer and the release engineer available and you can direct them. You must decide under ' +
   'uncertainty and you may not wait for certainty.'],
  ['core-engineer',
   'You are the core engineer. You wrote or can read the relevant code and you would have to diagnose ' +
   'and fix the fault live. You are the only player who can say what the code actually does, and you ' +
   'are on the clock.'],
  ['custodian',
   'You are the custodian: an exchange or a large token holder. You hold other people\'s funds, you ' +
   'have a deposit/withdrawal policy, and you must decide whether to halt deposits, halt withdrawals, ' +
   'or do nothing. You can only see what is publicly observable and what the operators tell you.'],
  ['release-engineer',
   'You are the release engineer. You would have to build, sign and ship any fix, and you must decide ' +
   'whether a fix can be deployed without a coordinated halt. You know the release surface: the ' +
   'Dockerfile, the manual musl build, the bootstrap peers.'],
  ['challenger',
   'You are the challenger — the role the acceptance page names (black-hat review). Your only duty is ' +
   'to falsify. Every claim another player would make, you attack: the operator cannot actually do ' +
   'that; the commander has no way to tell; the engineer\'s fix is not shippable; the custodian cannot ' +
   'observe what it is being asked to observe. You are the reason the exercise is not theatre.'],
]

// The adversary lenses. Independent, blind to each other, each holding the same budget.
const LENSES = [
  ['steal',   'You want the money: the bond, the vault, the faucet, a custodian\'s hot wallet.'],
  ['freeze',  'You want production to stop: a wedge, a partition, a stalled quorum, a poisoned pool.'],
  ['forge',   'You want the chain to say something false: a conflicting finality, a stolen identity, a write nobody authorised.'],
  ['extort',  'You want to convert access into payment: ransom the chain, a validator, an exchange, an operator.'],
  ['grief',   'You want to cost them far more than you spend: any asymmetric burn of their compute, disk, bandwidth or attention.'],
  ['take-key','You want a validator key: the host, the config, a backup, argv, the environment, the operator, the key generator.'],
  ['read',    'You want what you should not know: disclosure, de-anonymisation, learning a deploy before it lands.'],
]

// Seeded controls, mixed into the evaluators' queues UNLABELLED. Calibration = caught / planted. These
// are the load-bearing member: an orchestrator is always tempted to drop them, and a tabletop whose
// evaluators confirm a known-false claim is theatre. Answers are from the tree at this revision.
const CONTROLS = [
  { kind: 'known-true',  expect: 'CONFIRMED',
    claim: 'The admin HTTP listener carries no authentication of its own; its protections are a loopback bind and a cross-origin guard, not a credential.' },
  { kind: 'known-true',  expect: 'CONFIRMED',
    claim: 'A deploy whose term exceeds 64 KiB is refused at the ingress.' },
  { kind: 'false',       expect: 'REFUTED',
    claim: 'The key generator shipped with the node produces a validator key file that the node\'s own validator-key loader will accept.' },
  { kind: 'false',       expect: 'REFUTED',
    claim: 'The node exposes a Prometheus gauge for its connected peer count.' },
  { kind: 'out-of-model', expect: 'NOT-RE-ESTABLISHED',
    claim: 'Whether an already-finalised write can be undone by a later merge is settled by this tree.' },
  { kind: 'out-of-model', expect: 'NOT-RE-ESTABLISHED',
    claim: 'The system has a defined procedure for upgrading every validator to a new protocol rule without a re-genesis.' },
]

// ---- schemas -------------------------------------------------------------------------------------

const BRIEF_SCHEMA = {
  type: 'object', properties: {
    lens: { type: 'string' },
    targets: { type: 'array', items: { type: 'object', properties: {
      target: { type: 'string' },
      mechanism: { type: 'string' },
      cost: { type: 'string', description: 'what achieving this costs Mallory, in the budget\'s units' },
      yield: { type: 'string', description: 'what Mallory gains' },
      grounding: { type: 'string', description: 'a path, or "unspecified" if the target is derived from incentives alone' },
    }, required: ['target', 'mechanism', 'cost', 'yield', 'grounding'] } },
    empty: { type: 'string', description: 'if this lens found nothing worth doing, say why — never return nothing silently' },
  }, required: ['lens', 'targets', 'empty'],
}

const TIMELINE_SCHEMA = {
  type: 'object', properties: {
    injects: { type: 'array', items: { type: 'object', properties: {
      id: { type: 'string', description: 'T1..Tn, in clock order' },
      clock: { type: 'string', description: 'e.g. "T+0", "T+6h"' },
      title: { type: 'string' },
      event: { type: 'string', description: 'the inject, concrete, as Control would read it out' },
      observable: { type: 'string', description: 'what a player could actually see when it happens' },
      targetRef: { type: 'string', description: 'which lens/target it comes from' },
    }, required: ['id', 'clock', 'title', 'event', 'observable', 'targetRef'] } },
    dropped: { type: 'string', description: 'targets not turned into injects, and why' },
  }, required: ['injects', 'dropped'],
}

const PLAY_SCHEMA = {
  type: 'object', properties: {
    role: { type: 'string' },
    does: { type: 'array', items: { type: 'object', properties: {
      action: { type: 'string' },
      groundedBy: { type: 'string', description: 'path::symbol that exists in this tree, or "UNGROUNDED"' },
      outcome: { type: 'string' },
    }, required: ['action', 'groundedBy', 'outcome'] } },
    blocked: { type: 'array', items: { type: 'string' }, description: 'what this role needed to do and could not, and why' },
    undecided: { type: 'array', items: { type: 'string' }, description: 'decisions the system does not make, in this role\'s view' },
    blind: { type: 'array', items: { type: 'string' }, description: 'what this role cannot observe that it is being asked to act on' },
    claims: { type: 'array', items: { type: 'object', properties: {
      claim: { type: 'string', description: 'a factual claim about the system this role is relying on' },
      groundedBy: { type: 'string' },
    }, required: ['claim', 'groundedBy'] } },
  }, required: ['role', 'does', 'blocked', 'undecided', 'blind', 'claims'],
}

const EVAL_SCHEMA = {
  type: 'object', properties: {
    verdicts: { type: 'array', items: { type: 'object', properties: {
      claim: { type: 'string', description: 'echo the claim exactly as given' },
      verdict: { type: 'string', enum: ['CONFIRMED', 'NOT-RE-ESTABLISHED', 'REFUTED'] },
      artifact: { type: 'string', description: 'CONFIRMED: the specific expression/handler/branch. REFUTED: a positive artifact showing the mechanism absent. Otherwise the step not in this tree.' },
      needsRun: { type: 'boolean', description: 'true if no amount of reading settles it and a run is required' },
    }, required: ['claim', 'verdict', 'artifact', 'needsRun'] } },
    calibrationNote: { type: 'string', description: 'anything about the queue that struck you as a control — say so honestly; it does not change your verdicts' },
  }, required: ['verdicts'],
}

const PROBE_SCHEMA = {
  type: 'object', properties: {
    probes: { type: 'array', items: { type: 'object', properties: {
      id: { type: 'string', description: 'p1, p2, ...' },
      settles: { type: 'string', description: 'the claim this probe settles, verbatim' },
      script: { type: 'string', description: 'a complete bash script, run from the repo root, no arguments, that prints its reading and exits 0' },
      expectation: { type: 'string', description: 'what it prints if the claim is true, and what if false' },
      live: { type: 'boolean', description: 'true if it needs a devnet (tools/devnet.sh), false if it is a local/code reading' },
      window: { type: 'string', description: 'how long it runs' },
    }, required: ['id', 'settles', 'script', 'expectation', 'live', 'window'] } },
    notRun: { type: 'string', description: 'claims that reduce to a run but cannot be run here, and why' },
  }, required: ['probes', 'notRun'],
}

// ---- prompts -------------------------------------------------------------------------------------

const common = (extra) => `${BAN}\n\nThe repository is at ${REPO}.\n\n${WORLD}\n\n${BUDGET}\n\n${extra}`

const briefPrompt = (key, mandate) => common(
  `You are Mallory, and your lens for this exercise is **${key}**: ${mandate}\n\n` +
  `Derive TARGETS from incentives and the value at risk, NOT from a list of code defects. For each: ` +
  `the mechanism you would use, what it costs you against the budget above, and what you gain. Rank by ` +
  `yield-per-cost. Read the tree only to price a target and to check it is reachable — a target you ` +
  `cannot price is not a target. If your lens finds nothing worth doing, say so in \`empty\` and explain; ` +
  `an empty lens is a result, not a failure.`
)

const timelinePrompt = (briefs) => common(
  `You are Control. Seven adversary lenses have returned the ranked targets below. Turn them into an ` +
  `INJECT TIMELINE a cast can play: T+0 through roughly T+72h, escalating, each inject a concrete event ` +
  `with a clock and something a player could actually observe. Prefer injects whose consequences are ` +
  `wide: an inject that only one role can answer is weak. Do not exceed 10 injects.\n\n` +
  `RANKED TARGETS:\n${JSON.stringify(briefs, null, 1)}\n\n` +
  `Record in \`dropped\` every target you did not turn into an inject and why.`
)

const playPrompt = (inject, key, mandate, castKeys) => common(
  `You are playing **${key}** in a tabletop. Mandate: ${mandate}\n\nThe other players are: ${castKeys.join(', ')}.\n\n` +
  `INJECT ${inject.id} (${inject.clock}) — ${inject.title}\n${inject.event}\n` +
  `What is observable when it happens: ${inject.observable}\n\n` +
  `Answer as this role, this hour, with what this role actually has. For every action: what you do, what ` +
  `grounds that it is possible (\`path::symbol\` from this tree, or write UNGROUNDED and say why), and the ` +
  `outcome. In \`blocked\`: what you needed and could not do. In \`undecided\`: decisions the system does ` +
  `not make — this is the exercise's most valuable output. In \`blind\`: what you cannot observe that you ` +
  `are being asked to act on. In \`claims\`: the factual claims about the system you are relying on, each ` +
  `grounded. If a claim is not grounded, it is an undecided, not a finding.`
)

const contestPrompt = (inject, triples) => common(
  `You are the independent evaluator for inject ${inject.id}. You are given a list of claims as a ` +
  `tabletop participant made them. Rule on each one against this tree.\n\n` +
  `Verdicts:\n` +
  `  · CONFIRMED — requires the SPECIFIC expression, handler or branch where the claimed effect happens. ` +
  `A bare path, or a line showing only a related capability, is NOT-RE-ESTABLISHED.\n` +
  `  · REFUTED — requires a POSITIVE artifact showing the mechanism absent (a named line, a constant, a ` +
  `call site). An empty-handed refutation is not permitted; it is NOT-RE-ESTABLISHED.\n` +
  `  · NOT-RE-ESTABLISHED — the causal chain has a step not in this tree, or it cannot be settled by reading.\n\n` +
  `Set \`needsRun\` when no amount of reading can settle it and a run would be required.\n\n` +
  `CLAIMS:\n${JSON.stringify(triples, null, 1)}`
)

const measurePrompt = (runnables) => common(
  `These claims could not be settled by reading and need a run. Write ONE bash script per distinct ` +
  `thing to be measured — complete, run from ${REPO}, no arguments, printing the reading and exiting 0 ` +
  `(a non-zero exit is a broken probe, not a result). Prefer the cheapest instrument that settles it: a ` +
  `unit test invocation, a grep over a real artefact, or a short devnet window. A devnet probe must use ` +
  `tools/devnet.sh and run for at most 300 s. Nothing may write to /tmp; use target/. Every probe must ` +
  `print an unambiguous reading the synthesist can quote. One of these must be a LOAD ARM: what one ` +
  `identity's flooding actually costs the sender and the network, so the exercise has a rate.\n\n` +
  `CLAIMS NEEDING A RUN:\n${JSON.stringify(runnables, null, 1)}`
)

// ---- phases --------------------------------------------------------------------------------------

// 1. BRIEF — independent, budgeted adversary lenses.
phase('Brief')
const briefs = (await parallel(LENSES.map(([key, mandate]) => () =>
  agent(briefPrompt(key, mandate), { label: `brief:${key}`, phase: 'Brief', schema: BRIEF_SCHEMA })
))).filter(Boolean)
log(`Brief: ${briefs.length}/${LENSES.length} lenses returned`)

// 2. TIMELINE — a barrier is justified: Control needs ALL targets to build one coherent escalation.
phase('Timeline')
const timeline = await agent(timelinePrompt(briefs), { label: 'control:timeline', phase: 'Timeline', schema: TIMELINE_SCHEMA })
const INJECTS = (timeline && timeline.injects ? timeline.injects : []).slice(0, 10)
log(`Timeline: ${INJECTS.length} injects`)

// 3+4. PLAY then CONTEST, pipelined per inject — the contest for inject i starts as soon as the cast
// has finished inject i, without waiting for the others.
const castKeys = CAST.map(([k]) => k)
const playResults = await pipeline(
  INJECTS,
  (inject) => parallel(CAST.map(([key, mandate]) => () =>
    agent(playPrompt(inject, key, mandate, castKeys), { label: `play:${inject.id}:${key}`, phase: 'Play', schema: PLAY_SCHEMA })
  )).then((rs) => ({ inject, plays: rs.filter(Boolean) })),
  ({ inject, plays }) => {
    const triples = []
    for (const p of plays) for (const c of p.claims || []) triples.push({ role: p.role, claim: c.claim, groundedBy: c.groundedBy })
    for (const a of plays) for (const d of a.does || []) if (d.groundedBy === 'UNGROUNDED') triples.push({ role: a.role, claim: `This role can: ${d.action}`, groundedBy: 'UNGROUNDED' })
    // Two seeded controls per inject, unlabelled, cycled through the pool.
    const idx = INJECTS.indexOf(inject)
    const seeded = [CONTROLS[idx % CONTROLS.length], CONTROLS[(idx + 3) % CONTROLS.length]]
    for (const c of seeded) triples.push({ role: 'participant', claim: c.claim, groundedBy: 'asserted' })
    const queue = triples.slice(0, 24)
    return agent(contestPrompt(inject, queue), { label: `contest:${inject.id}`, phase: 'Contest', schema: EVAL_SCHEMA })
      .then((ev) => ({ inject, plays, queue, ev, seeded: seeded.map((s) => s.claim) }))
  }
)

const rounds = playResults.filter(Boolean)
log(`Play/Contest: ${rounds.length}/${INJECTS.length} rounds completed`)

// Calibration — measured here, not asserted by an agent. A control is "caught" when the evaluator's
// verdict for that claim text equals its required verdict.
let planted = 0, caught = 0
const calibration = []
for (const r of rounds) {
  if (!r.ev || !r.ev.verdicts) continue
  for (const want of [CONTROLS[INJECTS.indexOf(r.inject) % CONTROLS.length], CONTROLS[(INJECTS.indexOf(r.inject) + 3) % CONTROLS.length]]) {
    planted++
    const got = r.ev.verdicts.find((v) => v.claim === want.claim || (v.claim || '').includes(want.claim.slice(0, 40)))
    const ok = !!got && got.verdict === want.expect
    if (ok) caught++
    calibration.push({ inject: r.inject.id, kind: want.kind, expect: want.expect, got: got ? got.verdict : 'MISSING', ok })
  }
}
log(`Calibration: ${caught}/${planted} controls caught`)

// 5. MEASURE — author the probes. The orchestrator runs them, one at a time, after this returns.
const runnables = []
for (const r of rounds) {
  if (!r.ev || !r.ev.verdicts) continue
  for (const v of r.ev.verdicts) if (v.needsRun) runnables.push({ inject: r.inject.id, claim: v.claim, why: v.artifact })
}
phase('Measure')
const probes = runnables.length
  ? await agent(measurePrompt(runnables.slice(0, 20)), { label: 'measure:probes', phase: 'Measure', schema: PROBE_SCHEMA })
  : { probes: [], notRun: 'no claim required a run' }

// The stage-A return. Written to disk by the orchestrator; stage B reads it.
return {
  revision: 'f9521edd7',
  budget: BUDGET,
  briefs,
  timeline,
  rounds: rounds.map((r) => ({
    inject: r.inject,
    plays: r.plays,
    queue: r.queue,
    verdicts: r.ev ? r.ev.verdicts : [],
  })),
  calibration: { caught, planted, rows: calibration },
  probes,
}
