import Rchain.Par

/-!
# Law 60 — an install is total on a channel

The space keeps **one** installed continuation per channel (`rspace/src/hot_store.rs`'s
`installed_continuations`), and `install_continuation` used to write into it with a plain `insert`, so
a second install of a *different* continuation **replaced** the first and said nothing. That is AUDIT
C219: a minted vault handle installed `balance` at arity 2 and `transfer` at arity 5 as two
continuations on one name, only the second matched, and `balance` answered **nothing at all** — with no
error, because an unmatched receive is silence.

- **60a** — an install that would replace a *different* continuation on the same channel is
  **refused**; an idempotent re-install of the same one changes nothing.
- **60b** — what a channel carries after any permitted run: the continuation it was first given.

**Why the rule is a relation and not a function.** The content of 60a is that there is *no* step
replacing a different continuation, and a statement nothing can falsify is not a statement (the
`Dag.lean` lesson). So the defect's shape is modelled beside it as a **second relation** —
`ReplacingStep`, which is what `insert` did — and the two theorems that pair them are the claim:
`a_permitted_run_keeps_the_first` is the law, and `the_replacing_rule_does_not_keep_the_first` is the
same statement *false* of the rule it replaces. The pair is what makes 60b about the rule that produced
the store rather than about the container the store happens to be.

Layer `RSpace`; the Rust is `rspace/src/hot_store.rs`'s `install_continuation` (the refusal) and
`rholang/src/system_processes.rs`'s `install_vault_handle` (the one continuation that dispatches).
-/

namespace Rchain

/-- A store's installed continuations, as the map is keyed: a channel and the continuation under it.
The continuation is abstract — the law is about *which* one a channel keeps, not what it is. -/
abbrev InstallStore (κ : Type) := List (String × κ)

/-- The continuation a channel carries, if any. The head wins, which is what a map's key does. -/
def installed {κ : Type} : InstallStore κ → String → Option κ
  | [], _ => none
  | (c', k) :: rest, c => if c' = c then some k else installed rest c

/-- A channel that carries nothing takes the continuation; one that already carries the **same** one
is unchanged. **There is no third constructor**: an install of a *different* continuation on a channel
that carries one has no step at all, and that absence *is* the refusal (60a). -/
inductive InstallStep {κ : Type} : InstallStore κ → String → κ → InstallStore κ → Prop
  /-- The channel is free, so the continuation is installed. -/
  | fresh {s : InstallStore κ} {c : String} {k : κ} :
      installed s c = none → InstallStep s c k ((c, k) :: s)
  /-- The channel already carries this one, so nothing changes — the case play and replay need, since
  both install the system contracts over one store. -/
  | same {s : InstallStore κ} {c : String} {k : κ} :
      installed s c = some k → InstallStep s c k s

/-- The defect's shape: what `installed_continuations.insert` did — a second install **replaces**
whatever the channel carried, silently (AUDIT C219). -/
inductive ReplacingStep {κ : Type} : InstallStore κ → String → κ → InstallStore κ → Prop
  /-- The insert: the new continuation is the one that matches from here on. -/
  | replace {s : InstallStore κ} {c : String} {k : κ} : ReplacingStep s c k ((c, k) :: s)

/-- **60a** — a different install on a channel that carries one **has no step**. The proof has teeth:
the `fresh` arm is excluded by the channel not being free and the `same` arm by the continuations
differing, so the statement is not true of the relation by construction. -/
theorem a_different_install_has_no_step {κ : Type} {s : InstallStore κ} {c : String} {k k' : κ}
    (carried : installed s c = some k') (different : k' ≠ k) :
    ¬ ∃ s', InstallStep s c k s' := by
  rintro ⟨s', step⟩
  cases step with
  | fresh free => rw [carried] at free; exact absurd free (by simp)
  | same was => exact different (Option.some.inj (was.symm.trans carried)).symm

/-- **60b** — a permitted install leaves the channel carrying what it already carried, so a channel's
continuation is the **first** one it was given. This is the law on the rule that produced the store. -/
theorem a_permitted_run_keeps_the_first {κ : Type} {s : InstallStore κ} {c : String} {k : κ}
    (carried : installed s c = some k) :
    ∀ s', InstallStep s c k s' → installed s' c = some k := by
  intro s' step
  cases step with
  | fresh free => rw [carried] at free; exact absurd free (by simp)
  | same _ => exact carried

/-- **The falsifier, and it is the same statement about the rule the fix replaces.** Under
`ReplacingStep` the channel carries the *new* continuation and **no longer** carries the first — which
is exactly the vault handle's `balance` arm disappearing when `transfer` was installed second. The pair
of this theorem with `a_permitted_run_keeps_the_first` is what makes 60b a claim about a rule rather
than a restatement of a container. -/
theorem the_replacing_rule_does_not_keep_the_first {κ : Type} {s : InstallStore κ} {c : String} {k k' : κ}
    (different : k ≠ k') :
    installed ((c, k) :: s) c = some k ∧ installed ((c, k) :: s) c ≠ some k' := by
  constructor
  · simp [installed]
  · simp [installed, different]

/-- The same channel installed on twice under the replacing rule, in the order the vault handle used:
the first arm is gone and a method at its arity answers nothing. Stated concretely so that the
falsifier names the observable rather than a shape. -/
theorem the_vault_handles_balance_arm_was_lost :
    installed ((("vault", "transfer") :: [("vault", "balance")]) : InstallStore String) "vault"
      ≠ some "balance" := by
  simp [installed]

end Rchain
