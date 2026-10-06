import Rchain.Par

/-!
# Law 61 — a delivery in flight does not stall the loop

A session is served by one task, and that task reads a delivery, hands it to the object, and writes
the answer ([`crate::conn`]'s `handle_deliver`, awaited on `SessionLoop::run`). An object that cannot
answer without waiting on something outside the session therefore waited **inside** it — and the loop
could not read the next message while it did. AUDIT C223: a third-party handoff claim whose gift had
not been deposited yet polled for up to ten seconds in `Bootstrap::deliver`, so an unrelated delivery
on the **same** session went unanswered for ten seconds. The observable is the stall, not the timeout.

The fix is a `Reply::Deferred`: the object hands the loop a future, the loop goes on reading, and the
answer is written when the future lands. This is the same frame as Law 51 — the shapes of
non-progress — with the one shape a *session* can have: an answer that is not ready yet.

- **61a** — with an answer outstanding, the loop can still read the next delivery.
- **61b** — and the outstanding answer is still written: deferring is not dropping.

**Why the rule is a relation, and its defect beside it.** As in Law 60, the content is that a step
exists where the blocking shape has none, and a statement nothing can falsify is not a statement. So
`BlockingStep` — what polling inside the delivery did — is modelled beside `Step`, and the two
theorems that pair them carry the claim: `a_pending_answer_does_not_block_the_next_delivery` is the
law and `the_blocking_rule_never_shortens_the_queue` is the same shape *false* of the rule it
replaces, where the loop spins on the state it is already in.

Layer `Progress`; the Rust is `ocapn/src/conn.rs`'s `Reply::Deferred` and `SessionLoop::run`, and
`ocapn/src/bootstrap.rs`'s withdraw arm.
-/

namespace Rchain

/-- One turn — a delivery the loop reads — abstract: the law is about *when* the loop reads, not what a delivery says. -/
abbrev Turn := Nat

/-- A loop's state: the deliveries it has not read yet, in order, and the answers it has taken on but
not written. -/
structure LoopState where
  /-- Deliveries not yet read, oldest first. -/
  queued : List Turn
  /-- Answers the loop owes, whose objects are waiting on something outside the session. -/
  pending : List Turn

/-- What the loop may do. **`defer` is the whole law**: a delivery whose answer is not ready is taken
on and the loop goes straight back to reading, rather than waiting for it. -/
inductive Step : LoopState → LoopState → Prop
  /-- Read the next delivery and answer it now. -/
  | serve {d : Turn} {rest pending : List Turn} :
      Step ⟨d :: rest, pending⟩ ⟨rest, pending⟩
  /-- Read a delivery whose answer is not ready yet, take it on, and keep reading. -/
  | defer {d : Turn} {rest pending : List Turn} :
      Step ⟨d :: rest, pending⟩ ⟨rest, pending ++ [d]⟩
  /-- Write an answer that has landed. -/
  | fulfil {d : Turn} {queued rest : List Turn} :
      Step ⟨queued, d :: rest⟩ ⟨queued, rest⟩

/-- The defect's shape: the loop **waits** for the answer it owes before reading anything else — which
is what polling inside `handle_deliver` did (AUDIT C223). -/
inductive BlockingStep : LoopState → LoopState → Prop
  /-- An answer outstanding: the loop stays exactly where it is. A *step* that makes no progress is
  what a spin is, and is why the falsifier below is stated about the queue's length rather than about
  the existence of a step. -/
  | blocked {queued pending : List Turn} (waiting : pending ≠ []) :
      BlockingStep ⟨queued, pending⟩ ⟨queued, pending⟩
  /-- Only with nothing outstanding may it read at all. -/
  | read {d : Turn} {rest pending : List Turn} (free : pending = []) :
      BlockingStep ⟨d :: rest, pending⟩ ⟨rest, pending⟩

/-- **61a** — with an answer outstanding, the loop can still read the next delivery. The proof names
the step: `Step.defer`, whose successor's queue is strictly shorter. -/
theorem a_pending_answer_does_not_block_the_next_delivery
    (d : Turn) (rest : List Turn) (p : Turn) :
    ∃ s', Step ⟨d :: rest, [p]⟩ s' ∧ s'.queued.length < (d :: rest).length :=
  ⟨_, Step.defer, by simp only [List.length_cons]; omega⟩

/-- **The falsifier, and it is the same shape false of the rule the fix replaces.** Under
`BlockingStep` no step at all shortens the queue while an answer is outstanding: the loop either spins
on the state it is in or waits for a permission it will not get. That *is* the stall — the ten seconds
in which an unrelated delivery on the same session was not read. -/
theorem the_blocking_rule_never_shortens_the_queue
    (d : Turn) (rest : List Turn) (p : Turn) :
    ¬ ∃ s', BlockingStep ⟨d :: rest, [p]⟩ s' ∧ s'.queued.length < (d :: rest).length := by
  rintro ⟨s', step, shorter⟩
  cases step with
  | blocked _ => exact absurd shorter (Nat.lt_irrefl _)
  | read free => exact absurd free (by simp)

/-- The reflexive-transitive closure of [`Step`], spelled here so this module needs no closure
library: what the law is about is the states a loop can *reach*. -/
inductive Reach : LoopState → LoopState → Prop
  /-- Every state reaches itself. -/
  | refl (s : LoopState) : Reach s s
  /-- One step, then any number more. -/
  | step {a b c : LoopState} : Step a b → Reach b c → Reach a c

/-- **61b** — the outstanding answer is still written. Deferring is not dropping: the loop serves the
delivery and then writes the answer it took on. -/
theorem a_deferred_answer_is_still_written
    (d : Turn) (rest : List Turn) (p : Turn) :
    ∃ s', Reach ⟨d :: rest, [p]⟩ s' ∧ s'.pending = [] :=
  ⟨⟨rest, []⟩, Reach.step Step.serve (Reach.step Step.fulfil (Reach.refl _)), rfl⟩

end Rchain
