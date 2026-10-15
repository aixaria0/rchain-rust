import Rchain.Install

/-!
# Law 77 — a reset is a projection onto its root

**Why this module exists.** A read is `POST /api/explore-deploy` with no hash: the node forks a runtime
at the last finalised block's state and evaluates a term against it. The fork builds a store and
`build_runtime_core` installs every system-process definition into it
(`rholang/src/runtime.rs:175`); then `RhoRuntime::reset` **replaces** that store — `ISpace::reset`
assigns a fresh `InMemHotStore` (`rspace/src/rspace.rs:666`) — and installs the same definitions again
(`rholang/src/runtime.rs:447`). One read therefore pays the install pass twice, and the first pass
lands in a store that is discarded.

The code states the reason in a comment: *"the reset replaces the hot store, dropping the in-memory
system-process installs (they are not checkpointed)"*. **A comment is not a theorem**, and a fix
chosen from a comment is a fix chosen from whoever wrote the comment on whatever day. This module
states the same content as a law about the reset's rule, so that removing the first pass is a
*derivation* — and so that a measurement has an a priori outcome to be measured against.

**The law.** A reset to a root is a **projection** onto that root: it keeps the root it names and drops
the overlay. An install made before such a reset is therefore **unobservable** after it — the channel
carries what the reset left and what the *later* install put there, and nothing the earlier one did.
Two constructions that differ only by such an install are **equal as functions**, which is exactly what
licenses removing it: the change cannot alter any read's answer, and that follows from the reset's rule
rather than from a test.

**The falsifier is the shape a *folding* reset would have**, and it is not a straw man: folding an
overlay into the trie rather than dropping it is what a **checkpoint** does, and Law 74 is the law that
it must fold what it drains. Under that rule the same pre-reset install **is** observable
(`a_folding_reset_keeps_the_first_pass`), so `the_first_pass_is_erased` is a claim about the reset's
rule and not a restatement of a container — the pair is what gives it teeth, in the way
`Rchain/Install.lean`'s pair does for Law 60.

**What this does not claim.** Nothing here is about cost. The theorem says the two constructions answer
the same; it does *not* say the first pass is cheap to keep, and it does not say how much removing it
buys. The clock is the census's (`casper::runtime_manager::read_path_census`), and the measurement is
what those counters are for.
-/

namespace Rchain

/-- A state as the read path's construction manipulates it: the **root** it names, and the **overlay**
written since the construction touched it. The overlay is Law 60's `InstallStore`, so the law that a
channel keeps the *first* continuation it is given (`Rchain/Install.lean`) is the one that decides what
the two constructions below answer. -/
structure StoreView (κ : Type) where
  /-- The state root this view names — the block's post-state hash, as the code passes it. -/
  root : String
  /-- What has been installed since the view was made. -/
  overlay : InstallStore κ

/-- An install as a **function** of a view: what Law 60's `InstallStep` permits, applied. A channel
that carries nothing takes the continuation; one that already carries *this* continuation is
unchanged — Law 60's `same` arm, which is the idempotence the two constructions rest on. -/
def installInto {κ : Type} [DecidableEq κ] (v : StoreView κ) (c : String) (k : κ) : StoreView κ :=
  if installed v.overlay c = some k then v
  else { v with overlay := (c, k) :: v.overlay }

/-- The reset as RSpace implements it: the root it names, and **no overlay**. The store is replaced by
the state at that root, so what was written to the store it replaced is gone.

The view is an argument and is **not consulted** — that is the law's whole content, so the binder is
`_v` rather than `v`: the reset's result is the same *for every* view, which is what makes an install
made before it unobservable. -/
def resetTo {κ : Type} (_v : StoreView κ) (r : String) : StoreView κ :=
  { root := r, overlay := [] }

/-- The defect's shape, and a rule the port already contains under another name: a reset that **folds**
its overlay in rather than dropping it. That is what a checkpoint does — Law 74 requires the fold
before the clear — and it is the rule under which a pre-reset install would survive. -/
def foldingResetTo {κ : Type} (v : StoreView κ) (r : String) : StoreView κ :=
  { v with root := r }

/-- Installing a whole definition list, in order. -/
def installAll {κ : Type} [DecidableEq κ] : StoreView κ → List (String × κ) → StoreView κ
  | v, [] => v
  | v, ck :: rest => installAll (installInto v ck.1 ck.2) rest

/-- **The read path's construction as the code has it**: install the definitions, reset to the root,
then install them again (`rholang/src/runtime.rs:175`, `rspace/src/rspace.rs:666`,
`rholang/src/runtime.rs:447`). -/
def readPathAsWritten {κ : Type} [DecidableEq κ]
    (v : StoreView κ) (r : String) (defs : List (String × κ)) : StoreView κ :=
  installAll (resetTo (installAll v defs) r) defs

/-- The construction the law licenses: reset to the root, then install once. -/
def readPathDerived {κ : Type} [DecidableEq κ]
    (v : StoreView κ) (r : String) (defs : List (String × κ)) : StoreView κ :=
  installAll (resetTo v r) defs

/-- **77** — the first pass is **erased**: the construction that installs, resets and installs again is
the *same function* as the one that resets and installs once. Every answer a read can get is
therefore unchanged by removing the earlier pass, and that is a consequence of the reset's rule rather
than of any test of the read path. -/
theorem the_first_pass_is_erased {κ : Type} [DecidableEq κ]
    (v : StoreView κ) (r : String) (defs : List (String × κ)) :
    readPathAsWritten v r defs = readPathDerived v r defs := by
  simp only [readPathAsWritten, readPathDerived, resetTo]

/-- The same law said about the observable, which is what a read actually consults: after a reset, the
channel's continuation cannot be one the pre-reset install put there. -/
theorem an_install_before_a_reset_is_unobservable {κ : Type} [DecidableEq κ]
    (v : StoreView κ) (r c : String) (k : κ) :
    installed (resetTo (installInto v c k) r).overlay c = none := by
  simp [resetTo, installed]

/-- **The falsifier.** Under the folding reset the same install **is** observable — so
`the_first_pass_is_erased` is a statement about the reset's rule, and a reset that folded would make
the first pass live rather than dead. Without this, the law would be true of the definitions by
construction and would carry no content. -/
theorem a_folding_reset_keeps_the_first_pass {κ : Type} [DecidableEq κ]
    (v : StoreView κ) (r c : String) (k : κ) (empty : v.overlay = []) :
    installed (foldingResetTo (installInto v c k) r).overlay c = some k := by
  simp [foldingResetTo, installInto, installed, empty]

/-- **Non-vacuity**, the other direction: the two rules are *distinguishable*, so the law is not
satisfied by a model in which nothing installs anything. With one definition installed before the
reset, the folding rule carries it and the projecting rule does not. -/
theorem the_two_reset_rules_differ {κ : Type} [DecidableEq κ]
    (c : String) (k : κ) :
    (resetTo (installInto ⟨"s", []⟩ c k) "r").overlay = []
      ∧ (foldingResetTo (installInto ⟨"s", []⟩ c k) "r").overlay = [(c, k)] := by
  constructor <;> simp [resetTo, foldingResetTo, installInto, installed]

end Rchain
