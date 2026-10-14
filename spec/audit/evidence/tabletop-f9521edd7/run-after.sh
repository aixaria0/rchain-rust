// The red-team tabletop — stage B: the after-action.
//
// Runs once stage A has returned AND the probes it authored have been executed, serially, by the
// orchestrator. Given those two inputs it produces the exercise's real products, in this order:
// the timeline, the undecideds, the prices, and the verdict. The register is not touched here.
//
// Invoke with:
//   Workflow({ scriptPath: '<repo>/spec/audit/evidence/tabletop-f9521edd7/run-after.sh',
//              args: { stageA: '<repo>/spec/audit/evidence/tabletop-f9521edd7/stage-a.json',
//                      probeDir: '<repo>/spec/audit/evidence/tabletop-f9521edd7/probes' } })

export const meta = {
  name: 'redteam-tabletop-after',
  description: 'After-action for the red-team tabletop: timeline, undecideds, prices, verdict, plus a completeness critic',
  phases: [
    { title: 'After',   detail: 'the four products, from the played record and the probe results' },
    { title: 'Critic',  detail: 'what modality was not run, what claim is unverified, what was skipped' },
  ],
}

const REPO = '/home/patrick/RNodeRust/.claude/worktrees/redhat-audit'
const A = (args && args.stageA) || `${REPO}/spec/audit/evidence/tabletop-f9521edd7/stage-a.json`
const P = (args && args.probeDir) || `${REPO}/spec/audit/evidence/tabletop-f9521edd7/probes`

const BAN =
  'Do NOT read spec/findings.tsv, spec/AUDIT.md, spec/audit/passes.md, docs/src/spec/adversary.md, ' +
  'docs/src/spec/testnet-acceptance.md, or docs/src/node/security-audit.md. The exercise is not scored ' +
  'against a register; a register row is not evidence.'

const base =
  `${BAN}\n\nYou are synthesising the after-action of a red-team tabletop against RChain at ` +
  `f9521edd7. The repository is at ${REPO}.\n\n` +
  `The played record is at ${A} (JSON: the adversary briefs, the inject timeline, every role's actions ` +
  `and claims, and the evaluator's verdicts per claim).\n` +
  `The probe results, if any, are at ${P} (each <id>-run.sh with its <id>-run.log.txt).\n\n` +
  `Read both, and go back to the tree for anything you need to state as fact. Rules: a claim is only a ` +
  `finding if the evaluator returned CONFIRMED with a specific artifact; a claim whose grounding is ` +
  `UNGROUNDED is recorded as an UNDECIDED, not a finding; anything the probes did not settle is labelled ` +
  `as unmeasured, never dropped. Return markdown, no preamble.\n\n`

phase('After')
const products = await parallel([
  () => agent(base +
    `Produce **timeline.md**: what actually happened, inject by inject, as a reconstruction — each step ` +
    `naming the observable, who acted, what the tree actually does there (path::symbol), and where the ` +
    `reconstruction contradicts a player's account of themselves. Order by the inject clock. This is the ` +
    `document a reader uses to understand the incident; it is not a list of findings.`,
    { label: 'after:timeline', phase: 'After' }),

  () => agent(base +
    `Produce **undecideds.md**: every decision, owner or capability the system does NOT have, each with ` +
    `the role it blocked and the tree fact that shows it is absent (or "no path found in this tree"). ` +
    `This is the tabletop's most valuable output and cannot appear in a code register — a rule for who ` +
    `may halt; a key-rotation path; an owner for the faucet; a way for an operator to distinguish a ` +
    `partition from a halt; a procedure for a protocol upgrade. Rank by consequence: what does it cost ` +
    `when it is needed at 03:00.`,
    { label: 'after:undecideds', phase: 'After' }),

  () => agent(base +
    `Produce **prices.tsv**: one row per attack the red team established, columns ` +
    `\`attack \\t cost_to_adversary \\t yield \\t budget_used_pct \\t basis\` — where cost is expressed in ` +
    `the budget's own units (EUR, REV, identities, engineer-hours, wall-clock) and \`basis\` names the ` +
    `probe or the tree fact the price rests on. A price with no basis is written as \`unmeasured\`, never ` +
    `invented. Include the load arm's measured rate as a row.`,
    { label: 'after:prices', phase: 'After' }),

  () => agent(base +
    `Produce **verdict.md**: the answer to "should anyone run this", in this order: (1) the headline ` +
    `verdict in one sentence; (2) per attack class from the adversary's briefs, whether the cast could ` +
    `handle it, and on what evidence; (3) the **safety** statement the acceptance page defers — whether ` +
    `two validators can finalise a conflicting block, stated as far as this exercise established it and ` +
    `no further; (4) the **dissent**, recorded not smoothed: what the challenger held out against, and ` +
    `what the verdict does NOT decide.`,
    { label: 'after:verdict', phase: 'After' }),
]).then((r) => r.filter(Boolean))

phase('Critic')
const critic = await agent(
  `${BAN}\n\nYou are the completeness critic for a red-team tabletop at ${REPO}. The record is at ${A}; ` +
  `the probe results at ${P}; the four draft products are below.\n\n` +
  `Answer only this: what is missing? Name (1) the modality that was not run, (2) each claim that is ` +
  `asserted but unverified, (3) each surface the cast never touched, (4) each adversary lens that ` +
  `returned empty and whether the emptiness was earned or lazy. Be specific and short — this list is the ` +
  `next round's work, so an entry that cannot be acted on is not an entry.\n\n` +
  `PRODUCTS:\n${products.map((p, i) => `--- product ${i + 1} ---\n${p}`).join('\n\n')}`,
  { label: 'critic:completeness', phase: 'Critic' })

return { products, critic }
