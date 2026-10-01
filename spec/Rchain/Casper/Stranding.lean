import Rchain.Casper.Validate
import Rchain.Progress

/-!
# Law 53 — one attribution is terminal

C173, measured on a live testnet on 2026-09-29 (#105): a node that attributes **one** validation failure
to a bonded validator's block never follows that validator's chain again. Two rules make it so, and both
are the oracle's (`block_number` skips failed justifications when it computes the maximum, and
`neglected_invalid_block` refuses a block that justifies a failed bonded sender's block), so the law is
about the *combination* rather than about either rule:

- **the record persists** — a block marked failed is never unmarked. That is a modelling claim read off
  the port (`mark_failed` records the metadata of every `ValidateError::ValidationFailed`, and no rule
  clears the record) and it is the *only* thing this module takes from the code rather than proves from
  a rule. It is the same shape as 2PC's terminal records — `an_abort_is_absorbing`,
  `a_commit_is_absorbing` (`Rchain/CrossShard.lean`) — which is why Law 51's vocabulary names the two
  polarities apart: `Persistent` for this, `Unrestorable` for its dual;
- **the rule refuses the child** — `neglects` below, which is `neglected_invalid_block`'s body.

Together they are `Rchain.System.persistent_blocks_the_goal`: from a state that holds the refusal, **no**
reachable state admits a block above it. The node is `Terminal`, not slow — which is the difference
between a repair that waits and a repair that needs an inverse.

**The restoring rule landed (2026-10-01, #125).** The port now has the rule this module said it did not:
`casper/src/multi_parent_casper.rs:restore_divergent_justifications` re-validates a failed record whose
cause is *view-dependent*, bounded on three axes (keyed on the cause, capped per record by a persisted
count, budgeted per incoming block). It is modelled below as `restoreStep` — the exact inverse of
`strandStep` — and `the_refusal_is_not_persistent_once_a_rule_restores` is **the falsifier fired**: with
a step that unmarks in the relation, `the_refusal_is_persistent` is false by construction, exactly as
this module and the register's `falsifiable` cell predicted.

`the_refusal_is_persistent` is therefore stated over the rule set *without* the restoring rule, and is
kept: it is the guard, and Clause b is now where a reader is sent for the port's behaviour.
-/

namespace Rchain

/-- A node's record of the blocks it has refused, by the sender whose block it refused. The port keeps
    this in the DAG's metadata (`BlockMetadata::validation_failed`, set by `mark_failed` for every
    `ValidateError::ValidationFailed`, with the *cause* the rules classify the status into — which is
    what clause b's restoring rule is keyed on). -/
structure Strand where
  failed : List Nat
deriving DecidableEq

/-- **The refusal step**: the node marks one more sender's block failed. Nothing in the modelled rule set
    unmarks one — read off the port rather than assumed, and the subject of this law: `mark_failed`
    records, and no rule clears. -/
def strandStep (s s' : Strand) : Prop := ∃ v, s'.failed = v :: s.failed

/-- The transition system this law is stated over. `enabled` is `true` everywhere — a node can always
    refuse another block — and the interface field is discharged by construction. -/
def strandSystem : System where
  State := Strand
  Step := strandStep
  enabled := fun _ => true
  enabled_iff := by
    intro s
    exact ⟨fun _ => ⟨⟨0 :: s.failed⟩, 0, rfl⟩, fun _ => rfl⟩

/-- **The refusal persists.** At least one attribution frees nobody: once a sender's block has been marked
    failed it stays failed, because no step of the modelled rule set unmarks one. This is the amplifier
    C173 found, and it is the port's own sense of *absorbing* (`Rchain.an_abort_is_absorbing`): the node is
    not slow to forgive, it cannot. -/
theorem the_refusal_is_persistent (v : Nat) :
    strandSystem.Persistent (fun s : Strand => v ∈ s.failed) := by
  rintro s s' ⟨w, hw⟩ hv
  rw [hw]
  exact List.mem_cons_of_mem w hv

/-- **The rule**, on `Rchain.Casper.Validate`'s model of a block: a block that justifies a failed
    **bonded** sender's block is refused. That is `neglected_invalid_block`
    (`casper/src/validate.rs:321-340`), and it fires before any other rule runs — which is what makes it
    the structural half of the estrangement rather than one rule among several. -/
def neglects (bonded : List Nat) (b : Block) : Prop :=
  ∃ p ∈ b.justifications, p.validationFailed ∧ p.sender ∈ bonded

/-- **The rule is not vacuous**, on the model's own fixture: a block justifying a failed bonded sender's
    block is neglected, and one justifying a failed *unbonded* sender's is not — the port's own contrast,
    and the reason the reachable route to a failed parent is the unbonded one. -/
theorem a_neglected_block_is_detected :
    neglects [1] ⟨10, 0, 0, [⟨1, 9, 0, true⟩], 0, []⟩ ∧
      ¬ neglects [1] ⟨10, 0, 0, [⟨2, 9, 0, true⟩], 0, []⟩ := by
  constructor
  · exact ⟨⟨1, 9, 0, true⟩, by simp, by simp⟩
  · rintro ⟨p, hp, _hf, hb⟩
    simp only [List.mem_singleton] at hp
    subst hp
    simp at hb

/-- **…so a refused validator is never followed again.** The persistence keeps the refusal on every
    reachable state, the rule refuses any block above it, and the combination is `Terminal`: there is no
    reachable state in which a descendant of the refused block is admitted. The `hrule` hypothesis is the
    rule itself, named rather than assumed — which is the honest boundary of this clause, and the place a
    restoring rule would have to intervene. -/
theorem a_refused_validator_is_never_followed (v : Nat) (s : Strand) (hs : v ∈ s.failed)
    (above : Strand → Prop) (hrule : ∀ s, v ∈ s.failed → ¬ above s) :
    ¬ ∃ s', strandSystem.Reach s s' ∧ above s' :=
  strandSystem.persistent_blocks_the_goal (the_refusal_is_persistent v) hrule hs

/-! ## Clause b — a refusal has an inverse

The port's restoring rule (`restore_divergent_justifications`), and the execution of clause a's own
falsifier. -/

/-- **The restoring step**: the node unmarks one sender's block — the exact inverse of `strandStep`,
    which has none. This is `restore_divergent_justifications`, bounded there by the cause, by a
    per-record cap and by a per-block budget; the modelling here is of *that a step exists*, which is
    what `Terminal` demands and what clause a's falsifier says would end the persistence. -/
def restoreStep (s s' : Strand) : Prop := ∃ v, s.failed = v :: s'.failed

/-- The transition system with the restoring rule in it: the port's rule set **after** #125's fix. -/
def strandSystemRestoring : System where
  State := Strand
  Step := fun s s' => strandStep s s' ∨ restoreStep s s'
  enabled := fun _ => true
  enabled_iff := by
    intro s
    exact ⟨fun _ => ⟨⟨0 :: s.failed⟩, Or.inl ⟨0, rfl⟩⟩, fun _ => rfl⟩

/-- **The restoring step is not vacuous**: it removes the refusal `strandStep` put there, on the
    model's own fixture. A rule that existed and could never fire would satisfy the refutation below
    while restoring nothing. -/
theorem a_restoring_step_removes_a_refusal (v : Nat) :
    restoreStep ⟨v :: []⟩ ⟨[]⟩ := ⟨v, rfl⟩

/-- **The falsifier, fired.** Law 53a's `falsifiable` cell says the restoring rule "makes
    `the_refusal_is_persistent` false *by construction*" and calls that theorem a guard on this fix.
    Here it is: over the relation that *has* `restoreStep`, the refusal is **not** persistent.

    Read together with `the_refusal_is_persistent` this is the whole of the law's story — the refusal
    is absorbing exactly as long as no rule restores it, and the port now has one. -/
theorem the_refusal_is_not_persistent_once_a_rule_restores (v : Nat) :
    ¬ strandSystemRestoring.Persistent (fun s : Strand => v ∈ s.failed) := by
  intro h
  have hstep : strandSystemRestoring.Step ⟨[v]⟩ ⟨[]⟩ := Or.inr ⟨v, rfl⟩
  have hmem : v ∈ ([] : List Nat) := h ⟨[v]⟩ ⟨[]⟩ hstep (by simp)
  simp at hmem

end Rchain
