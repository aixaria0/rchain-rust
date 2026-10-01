import Rchain.Progress

/-!
# Law 55 — bounded work per step

**The class this law states** is the one AUDIT C180 named (`candidate:bounded-work-per-step`, #127): *work
whose cost grows with an input nothing bounds, and where a bound exists it is applied **after** the cost
rather than before it.* The register's defect rows are the instances — the merge's conflict search
enumerating one state per acyclic accepted subset before anything refused it (C178), an unbounded ingress
queue carrying full block messages (C175), a 3.2k-block start-up rebuild (C176) — and each of them was
tracked separately until the shape was named. This module is the statement the class needed, held to its
first member by the port's own bounds.

Two clauses, and they are the two things a bound must be:

- **a — the guard is before the work.** The step relation carries `spent < limit` as a *hypothesis*, so no
  reachable state has spent more than the protocol allows. The defect shape is its inverse, and
  `lateGuardStep` below models it: a loop that charges the step and *then* asks whether it was allowed, so
  a state past the budget is reachable. That the two are different relations is what makes clause a a claim
  rather than a definition.
- **b — the bound cannot change an answer.** A run whose budget suffices returns what the work produced,
  and a larger budget returns the same thing. So the bound is a **resource policy** and not a consensus
  change: two nodes that both complete compute the identical answer, which is Law 17's determinism.

**Why the second clause is stated over a schedule rather than over a counter.** The port's refusal is
`SearchBudgetExceeded`, which carries `steps` and `options` — two counts — and deliberately **no** option
set, because a truncated option set would pick a different rejection. So "the bound is all-or-nothing" is
not a policy the port chose to document; it is what the *type* says, and the model below is that shape: a
completion or nothing.
-/

namespace Rchain

/-! ## Clause a — the guard is before the work -/

/-- A work loop charged against a declared budget: `limit` is the quantity the protocol bounds and `spent`
    is what the loop has used. The port's `SearchBudget` is this pair, with `spent` being
    `SearchCensus::expanded` — the field the census publishes and the budget refuses on. -/
structure Work where
  limit : Nat
  spent : Nat
deriving DecidableEq

/-- **The guarded step**: one unit of work is charged, and the step exists only while the budget is not
    spent. `spent < limit` is a *hypothesis of the relation*, which is what "the guard is before the work"
    means mechanically — there is no state from which the over-budget step can be taken, rather than a
    check that refuses it afterwards. -/
def spendStep (s s' : Work) : Prop := s.spent < s.limit ∧ s' = ⟨s.limit, s.spent + 1⟩

/-- The transition system clause a is stated over. `enabled` is the guard, tied to the relation by
    `enabled_iff` so it cannot drift from it. -/
def workSystem : System where
  State := Work
  Step := spendStep
  enabled := fun s => decide (s.spent < s.limit)
  enabled_iff := by
    intro σ
    constructor
    · intro h
      exact ⟨⟨σ.limit, σ.spent + 1⟩, of_decide_eq_true h, rfl⟩
    · rintro ⟨_σ', hlt, -⟩
      exact decide_eq_true hlt

/-- **The budget is never exceeded.** Every state reachable from one inside its budget is inside its
    budget: the work a step costs is bounded by a quantity the protocol bounds, and the bound holds in
    every reachable state rather than being checked after the fact. -/
theorem the_work_never_exceeds_the_budget (s₀ : Work) (h : s₀.spent ≤ s₀.limit) :
    ∀ s', workSystem.Reach s₀ s' → s'.spent ≤ s'.limit := by
  intro s' hr
  induction hr with
  | refl => exact h
  | tail _ hstep _ =>
      rw [hstep.2]
      exact Nat.succ_le_of_lt hstep.1

/-- **An exhausted budget enables no step.** The guard is not merely early — it is the *first* thing that
    has to hold, so there is nothing to take once it fails. This is the clause stated as the interface the
    `System` vocabulary asks for. -/
theorem an_exhausted_budget_enables_no_step (n : Nat) : ¬ workSystem.Enabled ⟨n, n⟩ := by
  intro h
  have : ∃ s', spendStep ⟨n, n⟩ s' := (workSystem.enabled_iff ⟨n, n⟩).mp h
  rcases this with ⟨_s', hlt, -⟩
  exact Nat.lt_irrefl n hlt

/-- **The defect shape, in the same model**: a loop that charges the step *and then* asks whether it was
    allowed. Its step relation has no guard, so a state past the budget is reachable — which is
    `the_late_guard_overspends` below. The class's three instances all had this shape; clause a is the
    statement that the port's bounds do not. -/
def lateGuardStep (s s' : Work) : Prop := s' = ⟨s.limit, s.spent + 1⟩

/-- The transition system with the guard *after* the work — the shape C180 names. -/
def lateGuardSystem : System where
  State := Work
  Step := lateGuardStep
  enabled := fun _ => true
  enabled_iff := by
    intro σ
    exact ⟨fun _ => ⟨⟨σ.limit, σ.spent + 1⟩, rfl⟩, fun _ => rfl⟩

/-- **The late guard overspends**, from an exhausted start: with the check after the work, a state one unit
    past the budget is reachable in a single step. This is the falsifier of clause a — not a schedule that
    happens to overspend, but the relation itself admitting it. -/
theorem the_late_guard_overspends (n : Nat) :
    ∃ s', lateGuardSystem.Reach ⟨n, n⟩ s' ∧ n < s'.spent :=
  ⟨⟨n, n + 1⟩, Relation.ReflTransGen.single rfl, Nat.lt_succ_self n⟩

/-! ## Clause b — the bound cannot change an answer -/

/-- **A bounded run over a schedule.** `steps` is the sequence of expansions the search would perform, in
    order, each either a **completion** (`some`) or a continuation (`none`); `fuel` is the budget. The run
    returns the first completion within the budget, and `none` if the budget is spent first.

    `none` is the port's `SearchBudgetExceeded` — it carries the counters and deliberately **no**
    accumulator, because a truncated option set would pick a different rejection. Modelling the schedule as
    a list rather than as a function of the step index is what makes the induction below the list's own. -/
def boundedRun {α : Type} : List (Option (List α)) → Nat → Option (List α)
  | _, 0 => none
  | [], _ + 1 => none
  | some done :: _, _ + 1 => some done
  | none :: rest, fuel + 1 => boundedRun rest fuel

/-- **A spent budget carries no answer** — which is the *type's* doing rather than a policy: `none` is the
    empty case, so there is no partial accumulator for a caller to mistake for a result. Named because it
    is half of what the clause claims. -/
theorem a_spent_budget_carries_no_answer {α : Type} (steps : List (Option (List α))) :
    boundedRun steps 0 = none := by simp [boundedRun]

/-- **An answer inside the budget is the work's own answer, and a larger budget returns the same one.** If
    a run completes within `fuel`, then it completes with *that same* answer within every larger budget:
    the answer is a function of the work whenever the budget is not the binding constraint. So two nodes
    that both complete — however different their budgets — compute the identical option set, which is what
    keeps the bound a resource policy rather than a fork.

    The budget is written `extra + fuel` rather than `fuel + extra` so that the successor case is
    `Nat.add_succ`, which `simp` normalizes; the two are the same claim. -/
theorem a_larger_budget_does_not_change_an_answer {α : Type} (steps : List (Option (List α)))
    {fuel : Nat} {a : List α} (h : boundedRun steps fuel = some a) :
    ∀ extra, boundedRun steps (extra + fuel) = some a := by
  induction fuel generalizing steps with
  | zero => simp [boundedRun] at h
  | succ n ih =>
      intro extra
      simp only [Nat.add_succ]
      cases steps with
      | nil => simp [boundedRun] at h
      | cons s rest =>
          cases s with
          | some d =>
              have hd : d = a := by simpa [boundedRun] using h
              rw [hd]
              simp [boundedRun]
          | none =>
              have h' : boundedRun rest n = some a := by simpa [boundedRun] using h
              simpa [boundedRun] using ih rest h' extra

/-- **The clause is not vacuous**: a completion within the budget is returned, on the model's own fixture —
    an empty schedule is not a schedule that answers. -/
theorem a_completion_within_the_budget_is_returned {α : Type} (d : List α)
    (rest : List (Option (List α))) (fuel : Nat) :
    boundedRun (some d :: rest) (fuel + 1) = some d := by simp [boundedRun]

end Rchain
