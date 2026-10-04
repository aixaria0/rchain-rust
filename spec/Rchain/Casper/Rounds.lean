import Rchain.Casper.Stake
import Rchain.Casper.Fringe
import Rchain.Casper.Liveness

/-!
# Law 58 — the forward dual: a fully-participating set makes the gate publish

Law 14's gate (`Rchain.Casper.Fringe.nextFringe`, with `finality_iff_supermajority`) is a biconditional
over a **free** `supp : SupportMap`: it says the fringe advances *given* a support map that is a strict
supermajority of the bonded total. It never says such a support map **obtains**. That absence is not a
detail — the live stall of 2026-10-03 (C209) was exactly it: a fully-live three-validator net produced
blocks and finalised nothing, and the model could not say a word about it, because the hypothesis the
gate assumes was never connected to the liveness hypothesis the tree also declared and never used
(`spec/Rchain/Casper/Liveness.lean`'s `Participation`, `Delivery`, `StalenessBound`).

This file supplies the connection, and states honestly what is theorem and what is hypothesis:

* **clause a** — a full partition of the bonded set makes the gate publish (`gate_publishes_of_a_full_
  partition`, and the `nextFringe` corollary). General over `bonds`, not a fixture.
* **clause b** — a `Participation`-closed view **is** that full partition
  (`participation_all_current`), which is the step that consumes `Participation` and `StalenessBound`.
* **clause c** — the attestation licence is bounded (`the_licence_ends_at_the_horizon`), the model half
  of the Rust test `the_licence_ends_at_the_horizon_and_the_work_does_not`.

**What is *not* here, and must not be read into it.** "A fully-live net reaches a full partition within N
rounds" is **not** stated: it quantifies over a schedule, and this model has none — the tree's own
`Rchain.reduce_not_deterministic` is the proof that the flat calculus fixes no schedule, and Law 51's
`Fair` is named and not defined. That remains a named hypothesis. Likewise `Delivery` (the network is not
modelled) and the guard's reader being `ReadsTheView` (C209's content): the model has no guard, so it can
only *refute* the old reader, not prove the new one.

The Rust realization is `casper/src/blocks/proposer/proposer.rs` (`attestation_inputs`,
`ATTESTATION_HORIZON`, `attestation_suppressed`) and `block-storage/src/dag/{liveness,finalizer}.rs`.
-/

namespace Rchain

/-! ## Clause b — a participating view is a full partition -/

/-- Whether a sender's latest message is *current*: present, and within the window of the tip. This is
    `StalenessBound` as a `Bool`, and the shape the guard reads — a sender with no message is not
    current, which is the case C174 had to keep from making the partition unsatisfiable. -/
def currentOf (latest : Sender → Option Nat) (tip w : Nat) (s : Sender) : Bool :=
  match latest s with
  | some h => decide (h + w ≥ tip)
  | none => false

/-- `StalenessBound`, as the `Bool` the guard reads. Stated because `decide` cannot synthesise a
    `Decidable` instance through the `def` — the model says the same thing two ways, and this is the
    bridge between them. -/
theorem stalenessBound_iff (tip h w : Nat) : StalenessBound tip h w ↔ h + w ≥ tip := Iff.rfl

/-- **The step that consumes `Participation`.** If every bonded sender's latest message is within the
    window of the tip, then filtering the bonded senders by `currentOf` removes nothing — the *view* a
    guard sees, read from the current messages, is the whole bonded set.

    This is what makes the gate's free argument **obtain**: the support map is not supplied by fiat, it is
    what a participating set produces. It is also the theorem whose absence let C209's stall happen
    unnoticed — the register declared `Participation` and used it in nothing. -/
theorem participation_all_current (bonds : Bonds) (latest : Sender → Option Nat) (tip w : Nat)
    (h : Participation bonds latest tip w) :
    (bondedSenders bonds).filter (fun s => currentOf latest tip w s) = bondedSenders bonds := by
  refine List.filter_eq_self.mpr (fun s hs => ?_)
  obtain ⟨h', hsome, hge⟩ := h s hs
  show currentOf latest tip w s = true
  simp only [currentOf, hsome, StalenessBound]
  exact decide_eq_true hge

/-! ## Clause a — a full partition publishes -/

/-- Who saw a candidate, when the whole bonded set is current: **every bonded sender**, each having seen
    the whole bonded set. The support map a participating view yields, in the gate's own vocabulary. -/
def seersOf (bonds : Bonds) : List (Sender × List Sender) :=
  (bondedSenders bonds).map (fun s => (s, bondedSenders bonds))

/-- The support map `calculateFringe` is handed when every bonded sender is current. -/
def suppOfView (bonds : Bonds) : SupportMap :=
  (bondedSenders bonds).map (fun c => (c, seersOf bonds))

/-- Every candidate's seer set contains the whole partition — `allBonded` at `seersOf`. -/
theorem allBonded_seersOf (bonds : Bonds) (hne : bondedSenders bonds ≠ []) :
    allBonded (bondedSenders bonds) (seersOf bonds) = true := by
  have hall : (seersOf bonds).all
      (fun p => (bondedSenders bonds).all (fun b => p.2.contains b)) = true := by
    refine List.all_eq_true.mpr (fun p hp => ?_)
    have hseen : p.2 = bondedSenders bonds := by
      simp only [seersOf, List.mem_map] at hp
      obtain ⟨s, _, hs⟩ := hp
      rw [← hs]
    rw [hseen]
    exact List.all_eq_true.mpr (fun b hb =>
      (List.contains_iff_exists_mem_beq (bondedSenders bonds) b).mpr ⟨b, hb, by simp⟩)
  have hnil : (seersOf bonds).isEmpty = false := by
    cases hb : bondedSenders bonds with
    | nil => exact absurd hb hne
    | cons x xs => simp [seersOf, hb]
  unfold allBonded
  rw [hall, hnil]
  rfl

/-- The support a full view records is the whole bonded set. -/
theorem bondedSupport_suppOfView (bonds : Bonds) (hne : bondedSenders bonds ≠ []) :
    bondedSupport (suppOfView bonds) (bondedSenders bonds) = bondedSenders bonds := by
  unfold bondedSupport suppOfView
  have hfilter : (List.map (fun c => (c, seersOf bonds)) (bondedSenders bonds)).filter
      (fun p => allBonded (bondedSenders bonds) p.2)
      = List.map (fun c => (c, seersOf bonds)) (bondedSenders bonds) :=
    List.filter_eq_self.mpr (fun p hp => by
      obtain ⟨c, _, hc⟩ := List.mem_map.mp hp
      rw [← hc]
      exact allBonded_seersOf bonds hne)
  rw [hfilter, List.map_map]
  exact List.map_id'' (fun _ => rfl) (bondedSenders bonds)

/-- **The stake a full partition carries is the whole bonded stake.** Each bonded sender's stake is looked
    up once and no sender is counted twice, so the sum is `totalStake`. The `Nodup` hypothesis is the
    model's counterpart of the port's `BTreeMap` keys being distinct (`finalizer.rs:29`) — without it
    `stakeOf`'s first-match lookup would count a repeated sender once while `totalStake` counted it twice,
    which is why the hypothesis is named rather than assumed. -/
theorem filterMap_stakeOf_eq_totalStake (bonds : Bonds) (hnodup : (bondedSenders bonds).Nodup) :
    (bondedSenders bonds).filterMap (stakeOf bonds) = bonds.map (·.2) := by
  induction bonds with
  | nil => rfl
  | cons p rest ih =>
    obtain ⟨a, s⟩ := p
    have hb2 : bondedSenders ((a, s) :: rest) = a :: bondedSenders rest := by simp [bondedSenders]
    have hnod : a ∉ bondedSenders rest ∧ (bondedSenders rest).Nodup := by
      rw [hb2] at hnodup
      exact List.nodup_cons.mp hnodup
    obtain ⟨hnotin, hnodup'⟩ := hnod
    have hstakeAt : ∀ x, stakeOf ((a, s) :: rest) x =
        if a == x then some s else stakeOf rest x := by
      intro x
      unfold stakeOf
      rw [List.find?_cons]
      by_cases hx : (a == x) = true <;> simp [hx]
    have hstep : (bondedSenders rest).filterMap (stakeOf ((a, s) :: rest))
        = (bondedSenders rest).filterMap (stakeOf rest) :=
      List.filterMap_congr (fun x hx => by
        have hne : a ≠ x := by intro h; exact hnotin (by simpa [h] using hx)
        have hbeq : (a == x) = false := (beq_eq_false_iff_ne a x).mpr hne
        simp [hstakeAt x, hbeq])
    rw [hb2]
    simp only [List.filterMap_cons, hstakeAt a, beq_self_eq_true, if_true, List.map_cons]
    rw [hstep, ih hnodup']

/-- The gate's own arithmetic at a full partition: the supporting stake is the whole bonded stake. -/
theorem fullPartitionStake_suppOfView (bonds : Bonds) (hne : bondedSenders bonds ≠ [])
    (hnodup : (bondedSenders bonds).Nodup) :
    fullPartitionStake (suppOfView bonds) bonds bonds = totalStake bonds := by
  unfold fullPartitionStake totalStake
  rw [bondedSupport_suppOfView bonds hne, filterMap_stakeOf_eq_totalStake bonds hnodup]

/-- **Clause a — the forward dual Law 14 lacks.** A full partition of the bonded set makes the gate
    publish: `calculateFringe` is `true`, because the supporting stake is the whole bonded stake and the
    whole is a strict supermajority of itself exactly when it is non-empty.

    Law 14 says *given* such a support the fringe advances. This says a fully-participating set
    **produces** such a support, which is the sentence the model did not have. -/
theorem gate_publishes_of_a_full_partition (bonds : Bonds) (hne : bondedSenders bonds ≠ [])
    (hnodup : (bondedSenders bonds).Nodup) (hpos : 0 < totalStake bonds) :
    calculateFringe (suppOfView bonds) bonds bonds = true := by
  unfold calculateFringe
  rw [fullPartitionStake_suppOfView bonds hne hnodup]
  exact decide_eq_true (by unfold isSuperMajority; omega)

/-- **Clause a, at the fringe.** With a strictly new layer, the gate publishes it — the composition of
    clause a with Law 14's `nextFringe`, which is where the register previously stopped. -/
theorem nextFringe_publishes_of_a_full_partition (prev next : Fringe) (bonds : Bonds)
    (hne : bondedSenders bonds ≠ []) (hnodup : (bondedSenders bonds).Nodup) (hpos : 0 < totalStake bonds)
    (hnew : next ≠ prev) :
    nextFringe prev next (suppOfView bonds) bonds bonds = some next := by
  unfold nextFringe
  rw [gate_publishes_of_a_full_partition bonds hne hnodup hpos, decide_eq_true hnew]
  rfl

/-! ## Clause c — the licence is bounded -/

/-- The prompt-attestation licence, as the model sees it: a deploy-bearing block is within `horizon`
    heights of the tip. The counterpart of `attestation_inputs`'s second component
    (`casper/src/blocks/proposer/proposer.rs`), with `horizon = ATTESTATION_HORIZON`. -/
def inHorizon (tip h horizon : Nat) : Bool := decide (h + horizon ≥ tip)

/-- **Clause c — the licence ends at the horizon.** True at the horizon and false one height past it, so
    a finality stall cannot license attestations for ever (the C171 storm) while a live round still gets
    the range it needs. The Rust test with the same name pins the same boundary
    (`casper/src/blocks/proposer/proposer.rs:the_licence_ends_at_the_horizon_and_the_work_does_not`).

    **The `3 × LIVENESS_WINDOW` factor itself is measured, not proved** (`quiet_chain_tests`: a healthy
    round finalises a deploy within 3 heights, a killed-validator round within 7). What is a theorem is
    the boundary value; what is a measurement is that the factor is *sufficient*. -/
theorem the_licence_ends_at_the_horizon (h horizon : Nat) :
    inHorizon (h + horizon) h horizon = true ∧ inHorizon (h + horizon + 1) h horizon = false := by
  refine ⟨?_, ?_⟩
  · unfold inHorizon; exact decide_eq_true (by omega)
  · unfold inHorizon; exact decide_eq_false (by omega)

end Rchain
