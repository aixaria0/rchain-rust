import Rchain.Progress

/-!
# Law 56 — the rooted namespace

**The rule this law states** is the one issue #99 owed a decision on and the port now enforces:
`casper/src/genesis/resources/rgov/MasterDictionary.rho`. A name is **rooted in the identity that owns
it**, and the owner prefix is *derived* from the caller's deployer id
(`rho:rev:address("fromDeployerId", …)`) rather than supplied by the caller — so a write outside your
own root is **inexpressible rather than refused**, and a caller with no derivable identity is refused
outright. Short names are a separate, governed tier holding *aliases* to rooted paths; a version, once
published, is answered for ever.

Three clauses, and each is a claim rather than a definition because the defect shape beside it is a
*different relation*:

- **a — the owner is derived, and a path outside it is refused.** `publish` succeeds iff the path's
  owner prefix equals the caller's derived identity. The defect shape is `unguardedPublish`: the same
  function with the ownership test removed, which publishes under *another's* root. That the two
  differ is what makes clause a a claim — `an_unguarded_publish_hits_anothers_root` is its witness.
- **b — versions are append-only and sealing is final.** A publish appends; it never replaces. So the
  version a reader pinned at index `v` answers the same thing however much later the read happens, and
  a sealed path refuses every further version while still resolving the ones it has.
- **c — the alias tier is governed, and only by the root.** Setting a short name is the one operation
  the rooted tier does not make self-scoped, so it is the one that needs an authority — the identity
  that installs the dictionary, which can hand the role on.

**Why the model is over `Nat`.** The law is about the *shape* of the rule — which comparisons decide a
publish, and what a refusal leaves behind — not about base58. Modelling an address and a name as
abstract tokens keeps every predicate computable, so each theorem below is discharged by `decide` over
concrete values rather than by an induction that could hide a vacuous statement. Nothing here claims
anything about string encoding; `MasterDictionary.rho` is where the encoding lives, and
`casper/tests/master_dictionary.rs` is where it is pinned.

**Why the lookups are hand-written.** `List.lookup` and `List.contains` carry `BEq` instances that
`decide` will not reduce through, which would leave every theorem below unprovable *by computation* —
and an unprovable one invites a hand proof that hides whether the statement constrains anything. The
three helpers unfold structurally, so the tactic reaches a literal answer.
-/

namespace Rchain

/-- `xs.lookup k`, by structural recursion so `decide` can unfold it. -/
def lookupWith {α β : Type} [DecidableEq α] (k : α) : List (α × β) → Option β
  | [] => none
  | (k', v) :: rest => if k = k' then some v else lookupWith k rest

/-- `xs.contains k`, by structural recursion — the same reason as [`lookupWith`]. -/
def hasWith {α : Type} [DecidableEq α] (x : α) : List α → Bool
  | [] => false
  | y :: ys => if x = y then true else hasWith x ys

/-- `xs.set k v` (replace in place, or append), by structural recursion. -/
def setWith {α β : Type} [DecidableEq α] (k : α) (v : β) : List (α × β) → List (α × β)
  | [] => [(k, v)]
  | (k', v') :: rest => if k = k' then (k, v) :: rest else (k', v') :: setWith k v rest

/-- The last element, if any — `resolve`'s "latest version". -/
def lastWith : List Nat → Option Nat
  | [] => none
  | [x] => some x
  | _ :: rest => lastWith rest

/-- A rooted path: the identity that owns it, and the name within that root. The port carries these as
    the string `<revAddr>/<name>`; the pair is that string's meaning, and the law is about which
    comparisons on it decide a write. -/
structure NsPath where
  owner : Nat
  name : Nat
deriving DecidableEq, Repr

/-- The dictionary's whole state, as `stateCh` holds it: the append-only version log, the sealed set,
    the per-path writekey epoch, the alias tier, and the root authority's identity. -/
structure NsState where
  versions : List (NsPath × List Nat) := []
  sealed : List NsPath := []
  epoch : List (NsPath × Nat) := []
  aliases : List (Nat × NsPath) := []
  root : Nat := 0
deriving DecidableEq, Repr

/-- What a verb answered. `published` carries the version index the caller may pin. -/
inductive Verdict where
  | published (p : NsPath) (v : Nat)
  | notYourNamespace
  | revoked
  | sealed
  | notRootAuthority
  | noIdentity
deriving DecidableEq, Repr

/-- The append-only log for a path, and its latest version — `resolve` in the contract. -/
def latest (st : NsState) (p : NsPath) : Option Nat :=
  match lookupWith p st.versions with
  | some vs => lastWith vs
  | none => none

/-- **Clauses a and b.** A publish is accepted iff the caller has a derived identity *and* the path is
    rooted in it, and it appends. A sealed path takes no further version. -/
def publish (st : NsState) (me : Option Nat) (p : NsPath) (_v : Nat) : Verdict :=
  match me with
  | none => .noIdentity
  | some a =>
    if p.owner = a then
      if hasWith p st.sealed then .sealed
      else .published p (lookupWith p st.versions |>.getD []).length
    else .notYourNamespace

/-- The state a successful publish leaves: the value appended, nothing replaced. -/
def publishState (st : NsState) (p : NsPath) (v : Nat) : NsState :=
  { st with versions := setWith p ((lookupWith p st.versions).getD [] ++ [v]) st.versions }

/-- **Clause c.** Setting a short name is the governed act: the root authority, and no one else. -/
def setAlias (st : NsState) (me : Option Nat) (short : Nat) (target : NsPath) : Verdict :=
  match me with
  | none => .noIdentity
  | some a => if a = st.root then .published target short else .notRootAuthority

/-- A writekey is bound to one path **and to the epoch it was granted at**, so `revoke` retires every
    key already handed out: the grant is valid iff the epoch has not moved since. -/
def writekeyValid (st : NsState) (p : NsPath) (grantedAt : Nat) : Bool :=
  (lookupWith p st.epoch).getD 0 = grantedAt

/-- **The defect shape clause a is stated against**: `publish` with the ownership test removed. It is
    the same function in every other respect, which is what makes the refusal theorems below about the
    rule and not about the code. -/
def unguardedPublish (st : NsState) (me : Option Nat) (p : NsPath) (_v : Nat) : Verdict :=
  match me with
  | none => .noIdentity
  | some _ => if hasWith p st.sealed then .sealed else .published p 0

/-- The empty state, spelled out where a theorem needs it. -/
def emptyState : NsState :=
  { versions := [], sealed := [], epoch := [], aliases := [], root := 0 }

/-- A state whose root authority is `r`. -/
def stateRootedAt (r : Nat) : NsState :=
  { versions := [], sealed := [], epoch := [], aliases := [], root := r }

/-! ## Clause a — the owner is derived -/

/-- **A stranger publishing under another's root is refused.** The probe the prototype ran, and the
    first of the four must-fail cases. -/
theorem a_stranger_publishing_under_alices_root_is_refused :
    publish emptyState (some 2) ⟨1, 7⟩ 42 = .notYourNamespace := by decide

/-- The control, in the same model and by the same tactic: the *same* call under the caller's own root
    is accepted. Without it the theorem above would be satisfied by a `publish` that refused
    everything. -/
theorem alice_publishing_under_her_own_root_is_accepted :
    publish emptyState (some 1) ⟨1, 7⟩ 42 = .published ⟨1, 7⟩ 0 := by decide

/-- **A caller with no derivable identity is refused.** In the port this is a forged deployer id: the
    address derivation answers `Nil`, and the verb guards it before touching shared state, because a
    `Nil` owner would make one row writable by anyone. -/
theorem a_forged_identity_is_refused :
    publish emptyState none ⟨1, 7⟩ 42 = .noIdentity := by decide

/-- **The falsifier.** With the ownership test removed, the *same* inputs publish under the other's
    root — so `a_stranger_publishing_under_alices_root_is_refused` is a claim about the guard, not a
    restatement of the code. -/
theorem an_unguarded_publish_hits_anothers_root :
    unguardedPublish emptyState (some 2) ⟨1, 7⟩ 42 = .published ⟨1, 7⟩ 0 := by decide

/-! ## Clause b — append-only, and sealing is final -/

/-- **The second publish of a path is accepted**, and reports the *next* version index. -/
theorem a_second_publish_appends :
    publish (publishState emptyState ⟨1, 7⟩ 42) (some 1) ⟨1, 7⟩ 43 = .published ⟨1, 7⟩ 1 := by decide

/-- **A pinned version never moves.** After two publishes, the log holds both, in order — the property
    that makes `resolveAt(path, 0)` worth having. -/
theorem a_pinned_version_is_unchanged_by_a_later_publish :
    (publishState (publishState emptyState ⟨1, 7⟩ 42) ⟨1, 7⟩ 43).versions = [(⟨1, 7⟩, [42, 43])]
      ∧ latest (publishState (publishState emptyState ⟨1, 7⟩ 42) ⟨1, 7⟩ 43) ⟨1, 7⟩ = some 43 := by
  decide

/-- **A sealed path refuses a further version** — the third of the four must-fail cases — while the
    versions it already has keep resolving, which is the half that makes sealing useful rather than
    destructive. -/
theorem a_sealed_path_refuses_a_further_version_and_keeps_its_versions :
    publish { emptyState with sealed := [⟨1, 7⟩], versions := [(⟨1, 7⟩, [42])] } (some 1) ⟨1, 7⟩ 43
        = .sealed
      ∧ latest { emptyState with sealed := [⟨1, 7⟩], versions := [(⟨1, 7⟩, [42])] } ⟨1, 7⟩
        = some 42 := by
  decide

/-- **A sealed path takes no version from anyone**, including its owner — sealing is a property of the
    path, not a permission its owner grants and can then publish around. -/
theorem sealing_is_not_the_owners_to_wave_off :
    publish { emptyState with sealed := [⟨1, 7⟩] } (some 1) ⟨1, 7⟩ 43 = .sealed := by decide

/-! ## Clause c — the alias tier is governed -/

/-- **A non-root identity may not set a short name** — the fourth of the four must-fail cases. -/
theorem a_non_root_may_not_set_a_short_name :
    setAlias (stateRootedAt 9) (some 2) 5 ⟨1, 7⟩ = .notRootAuthority := by decide

/-- The control: the root authority may, and that is the whole of the alias tier's governance — one
    identity, which names its own successor. -/
theorem the_root_may_set_a_short_name :
    setAlias (stateRootedAt 9) (some 9) 5 ⟨1, 7⟩ = .published ⟨1, 7⟩ 5 := by decide

/-- **A revoked writekey is refused.** `revoke` bumps the epoch, so a key granted before it no longer
    matches — the second of the four must-fail cases, and the reason the key carries an epoch at
    all. -/
theorem a_revoked_writekey_is_refused :
    ¬ writekeyValid { emptyState with epoch := [(⟨1, 7⟩, 1)] } ⟨1, 7⟩ 0 := by decide

/-- The control: a key granted at the current epoch is valid. -/
theorem a_live_writekey_is_valid :
    writekeyValid { emptyState with epoch := [(⟨1, 7⟩, 1)] } ⟨1, 7⟩ 1 := by decide

end Rchain
