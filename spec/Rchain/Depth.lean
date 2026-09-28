import Rchain.Par

/-!
# Law 50 — the AST-depth budget

`rholang/src/parser.rs` refuses a term whose **AST depth** exceeds `MAX_AST_DEPTH`. This module is
the quantity that bound is about.

**Why a law and not a comment (AUDIT C99).** The parser's older guards bound two *shapes* and compose
into nothing: `MAX_PARSE_DEPTH` bounds the parser's recursion, and a flat operator chain is built by a
loop — so it costs a bounded number of frames and produces an **AST of depth `n`** — and `d` levels of
`c` operators compose into an AST of depth `d × c` with both guards satisfied. Every consumer of the
term then recurses once per level (`normalize_proc`, `sort_par`/`sort_expr`, `well_scoped_par`,
`eval_expr_to_expr`, `spatial_match_core`, `build_string`), which on the node's 32 MiB worker stack is
a **process abort** at an AST depth of ~1,000 (a ~4 KB deploy) and 64 s of CPU at ~8,000. The fix is a
bound on the *term*, and this is the term's depth.

**What is here, and what is owed.** `parDepth` below is the definition — the depth of a `Par` as a
tree of subterms, field-wise on the flat `Par` exactly as `closed` and `freeVarOf` are. The obligation
that is **not** yet discharged is the walk: `rholang/src/parser.rs::exceeds_ast_depth` tests
`parDepth p ≤ MAX_AST_DEPTH` by an iterative, early-exiting traversal (iterative because the depth is
the thing a recursive walk cannot survive), and what ties the two is

  * `walkExceeds limit p = false → parDepth p ≤ limit` — the direction that matters: a **sound**
    refusal test, so a term the node accepts cannot be deeper than the bound; and its control,
  * `parDepth p ≤ limit → walkExceeds limit p = false` — completeness, so the walk is not refusing by
    accident.

Both are owed; the row is `owed` for exactly that reason, and `spec/INDUCTIVE`-style recipes are not
needed to say so. The shape to prove them in is `Rchain/FreeVars.lean`'s: one `mutual` block with a
member per type and per `List`, `termination_by … => sizeOf …`, and a second `mutual` block for the
walk so that the walk is an **independent recursion** rather than `decide (limit < parDepth p)` — a
walk defined as the depth it is checking would make both directions `rfl`, which is the vacuity
`Rchain/Laws.lean` records for law 22.

**The falsifier, when the proof lands**: dropping one arm of the walk's children function must make
the soundness direction false on a concrete term — `a_dropped_arm_breaks_soundness`. The Rust half of
that risk (a `Proc` constructor the walk does not descend into) is not a Lean claim at all: it is
pinned by `rholang/tests`' exhaustive-construct depth test and by the parser's own refusal tests.
-/

namespace Rchain

mutual
  /-- The depth of `p` as a tree of subterms: `1` at a leaf, `1 + max` over its children. The quantity
  `MAX_AST_DEPTH` bounds and every consumer of a term recurses over. -/
  def parDepth : Par → Nat
    | Par.mk s r nw e m u b c =>
        1 + max (listDepthSend s)
              (max (listDepthReceive r)
              (max (listDepthNew nw)
              (max (listDepthExpr e)
              (max (listDepthMatch m)
              (max (listDepthGUnforgeable u)
              (max (listDepthBundle b)
                   (listDepthConnective c)))))))
  termination_by p => sizeOf p

  def sendDepth : Send → Nat
    | Send.mk c d _ => 1 + max (parDepth c) (listDepthPar d)
  termination_by s => sizeOf s

  def receiveBindDepth : ReceiveBind → Nat
    | ReceiveBind.mk ps s _ => 1 + max (listDepthPar ps) (parDepth s)
  termination_by b => sizeOf b

  def receiveDepth : Receive → Nat
    | Receive.mk bs b _ _ => 1 + max (listDepthReceiveBind bs) (parDepth b)
  termination_by r => sizeOf r

  def newDepth : New → Nat
    | New.mk _ b => 1 + parDepth b
  termination_by n => sizeOf n

  def matchCaseDepth : MatchCase → Nat
    | MatchCase.mk p s _ => 1 + max (parDepth p) (parDepth s)
  termination_by m => sizeOf m

  def matchDepth : Match → Nat
    | Match.mk t cs => 1 + max (parDepth t) (listDepthMatchCase cs)
  termination_by m => sizeOf m

  def exprDepth : Expr → Nat
    | Expr.ground _ => 1
    | Expr.evar _ => 1
    | Expr.eneg p => 1 + parDepth p
    | Expr.enot p => 1 + parDepth p
    | Expr.eplus p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eminus p q => 1 + max (parDepth p) (parDepth q)
    | Expr.emult p q => 1 + max (parDepth p) (parDepth q)
    | Expr.ediv p q => 1 + max (parDepth p) (parDepth q)
    | Expr.emod p q => 1 + max (parDepth p) (parDepth q)
    | Expr.elt p q => 1 + max (parDepth p) (parDepth q)
    | Expr.ele p q => 1 + max (parDepth p) (parDepth q)
    | Expr.egt p q => 1 + max (parDepth p) (parDepth q)
    | Expr.ege p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eeq p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eneq p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eand p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eor p q => 1 + max (parDepth p) (parDepth q)
    | Expr.ematches p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eshortand p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eshortor p q => 1 + max (parDepth p) (parDepth q)
    | Expr.elist ps _ => 1 + listDepthPar ps
    | Expr.etuple ps => 1 + listDepthPar ps
    | Expr.eset ps _ => 1 + listDepthPar ps
    | Expr.emap kvs _ => 1 + listDepthPair kvs
    | Expr.ebigint _ => 1
    | Expr.emethod _ p args => 1 + max (parDepth p) (listDepthPar args)
    | Expr.epercentPercent p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eplusPlus p q => 1 + max (parDepth p) (parDepth q)
    | Expr.eminusMinus p q => 1 + max (parDepth p) (parDepth q)
  termination_by e => sizeOf e

  def bundleDepth : Bundle → Nat
    | Bundle.mk p _ _ => 1 + parDepth p
  termination_by b => sizeOf b

  /-- Every `GUnforgeable` is a leaf — no `termination_by` clause, because there is no recursion to
  justify (a clause here is an unused one, and Lean says so). -/
  def gUnforgeableDepth : GUnforgeable → Nat
    | GUnforgeable.gPrivate _ => 1
    | GUnforgeable.gDeployId _ => 1
    | GUnforgeable.gDeployerId => 1
    | GUnforgeable.gSysAuthToken => 1

  def connectiveDepth : Connective → Nat
    | Connective.connAnd ps => 1 + listDepthPar ps
    | Connective.connOr ps => 1 + listDepthPar ps
    | Connective.connNot p => 1 + parDepth p
    | Connective.connVarRef _ _ => 1
  termination_by c => sizeOf c

  def listDepthPar : List Par → Nat
    | [] => 0
    | a :: as => max (parDepth a) (listDepthPar as)
  termination_by l => sizeOf l

  def listDepthPair : List (Par × Par) → Nat
    | [] => 0
    | (a, b) :: as => max (parDepth a) (max (parDepth b) (listDepthPair as))
  termination_by l => sizeOf l

  def listDepthSend : List Send → Nat
    | [] => 0
    | a :: as => max (sendDepth a) (listDepthSend as)
  termination_by l => sizeOf l

  def listDepthReceive : List Receive → Nat
    | [] => 0
    | a :: as => max (receiveDepth a) (listDepthReceive as)
  termination_by l => sizeOf l

  def listDepthReceiveBind : List ReceiveBind → Nat
    | [] => 0
    | a :: as => max (receiveBindDepth a) (listDepthReceiveBind as)
  termination_by l => sizeOf l

  def listDepthNew : List New → Nat
    | [] => 0
    | a :: as => max (newDepth a) (listDepthNew as)
  termination_by l => sizeOf l

  def listDepthMatch : List Match → Nat
    | [] => 0
    | a :: as => max (matchDepth a) (listDepthMatch as)
  termination_by l => sizeOf l

  def listDepthMatchCase : List MatchCase → Nat
    | [] => 0
    | a :: as => max (matchCaseDepth a) (listDepthMatchCase as)
  termination_by l => sizeOf l

  def listDepthExpr : List Expr → Nat
    | [] => 0
    | a :: as => max (exprDepth a) (listDepthExpr as)
  termination_by l => sizeOf l

  def listDepthBundle : List Bundle → Nat
    | [] => 0
    | a :: as => max (bundleDepth a) (listDepthBundle as)
  termination_by l => sizeOf l

  def listDepthGUnforgeable : List GUnforgeable → Nat
    | [] => 0
    | a :: as => max (gUnforgeableDepth a) (listDepthGUnforgeable as)
  termination_by l => sizeOf l

  def listDepthConnective : List Connective → Nat
    | [] => 0
    | a :: as => max (connectiveDepth a) (listDepthConnective as)
  termination_by l => sizeOf l
end

/-- The node's own bound, `rholang/src/parser.rs::MAX_AST_DEPTH`. Kept as a named constant here so a
law statement can speak about it rather than about a literal: the number is the code's choice, and
the law is that the walk and `parDepth` agree about it. -/
def maxAstDepth : Nat := 768

/-- A concrete term at a known depth — the witness shape the owed directions are stated over. A unit
term is depth 1, and wrapping it in `n` unary nots gives depth `n + 1`, which is the arithmetic a
falsifying mutation has to break.

**Why this is a definition and not a `decide`d example** — the trap `spec/STYLE.md` records, met here:
`parDepth` is a `mutual` block over nested lists, so it needs `termination_by`, and a well-founded
definition is **not kernel-reducible** — `decide` cannot evaluate `parDepth (notsDepth 767)`, and the
attempt reports `reduction got stuck`. So law 50's two directions are proofs by induction, not
evaluations, and this term is their witness: `notsDepth n` has depth `n + 1`, so `notsDepth 767`
reaches `maxAstDepth` exactly and `notsDepth 768` is the first term over it. -/
def notsDepth (n : Nat) : Par :=
  Par.mk [] [] [] (List.replicate n (.enot (Par.mk [] [] [] [] [] [] [] []))) [] [] [] []

/-! ## Clause 50b — the bound the *space* applies to a **runtime-built value**

`parDepth` above is one quantity, and it is bounded on two routes: the parser refuses a source whose
tree exceeds `maxAstDepth`, and the space refuses a produced value whose tree exceeds
`maxValueDepth`. The second route exists because a rholang program builds terms the parser never saw —
a contract folding its accumulator into a deeper pair each iteration reaches depth `n` in `O(n)`
reduce steps — and every consumer of a stored value recurses once per level.

**Why two numbers rather than one** (`rholang/src/storage.rs::MAX_VALUE_DEPTH` carries both
measurements): the walks have different frames. On the node's 32 MiB worker in a debug build the
parser route's overspill aborts at an AST depth of ~1,000, while the value route aborts at ~401 inside
`eval_single_expr`'s recursion over the value. A single number at the parser's 768 would be a bound
that never fires before the crash it exists to prevent — which is what this unit's first draft shipped.
So 256 it is, and the two constants are kept apart deliberately.

**What the Rust walk's accounting is, and the obligation that follows.** `exceeds_value_depth` is
field-wise over the flat `Par` like `parDepth`, but it gives an `Expr` node **no level of its own**: a
`Par`'s expression children are charged the same depth as its other fields. `parDepth` does count the
`Expr` node, so the two functions are not equal and the owed directions are *not* the trivial ones:

  * `walkExceeds limit p = false → parDepth p ≤ limit + (the expression nesting the walk did not
    count)` — soundness with that slack, and the slack is what the proof has to bound; and its
    control, that the walk is not refusing by accident.

The slack is bounded on this route for a reason worth stating rather than assuming: a *value's*
expression nesting is syntax, and no runtime construction path builds `Expr` nodes — so on any value
that reached the space through a parsed program the slack is at most the parser's own
`MAX_PARSE_DEPTH` (128), and the space's values are bounded by `maxValueDepth + 128`. A value injected
by a hand-built or wire-carried message is where that argument stops, which is why the row is `owed`
and not tied.
-/

/-- The bound the **space** applies to a produced value, `rholang/src/storage.rs::MAX_VALUE_DEPTH`.
Apart from `maxAstDepth` because the two bound different walks (see the section above). -/
def maxValueDepth : Nat := 256

/-- The witness shape for the **value** route: nested pairs, which is what a folding contract actually
builds (`([[..], 1], 1)`) and what an attacker builds. `pairsDepth n` has `parDepth` `2 * n + 1` — the
arithmetic a falsifying mutation has to break, and the shape a `not`-chain *cannot* supply here,
because the value walk gives an `Expr` node no level of its own while `parDepth` counts it. -/
def pairsDepth : Nat → Par
  | 0 => Par.mk [] [] [] [] [] [] [] []
  | n + 1 => Par.mk [] [] [] [.etuple [pairsDepth n]] [] [] [] []

/-! ## The walk — `parDepth` as a refusal test

`rholang/src/parser.rs::exceeds_ast_depth` is an **iterative, early-exiting traversal**: it starts the
root at depth `1`, returns `true` the moment a node's depth exceeds the limit, and otherwise pushes its
children one level deeper. It cannot be written recursively on the node, because the depth is the very
thing a recursive walk does not survive — so the two directions law 50 owes are *not* `rfl`, and the
walk has to be a recursion of its own (`Rchain/Laws.lean` records the alternative as law 22's vacuity:
a walk defined as `decide (limit < parDepth p)` would make both directions trivial and prove nothing).

`walkPar` below is the same test as a **structurally-recursive descent with a budget**, so that it *can*
be reasoned about: a node with budget `0` is already too deep (its own depth is `1`), and a node with
budget `n + 1` is too deep exactly when one of its children is too deep at budget `n`. That is the
iterative walk's `d > limit` / `push children at d + 1`, read from the limit down instead of from the
root up.

`walkPar_iff_parDepth` is the obligation: `walkPar n p = false ↔ parDepth p ≤ n` — soundness in the
`→` direction (a term the walk accepts is never deeper than the budget) and completeness in the `←`
(the walk is not refusing by accident).

**The heartbeat budget** is stated rather than hit by accident: the walk's termination goals are
`sizeOf` comparisons over mutually-defined inductives, and `simp_wf` unfolds enough of the size table
for the default 200,000 to expire on the list arms. The goals themselves are one-line arithmetic
(`sizeOf a < 1 + sizeOf a + sizeOf as`); it is the unfolding that costs. -/
set_option maxHeartbeats 1000000 in
mutual
  def walkPar : Nat → Par → Bool
    | 0, _ => true
    | n + 1, Par.mk s r nw e m u b c =>
        walkListSend n s || walkListReceive n r || walkListNew n nw || walkListExpr n e ||
        walkListMatch n m || walkListGUnforgeable n u || walkListBundle n b ||
        walkListConnective n c
  termination_by _ x => sizeOf x

  def walkSend : Nat → Send → Bool
    | 0, _ => true
    | n + 1, Send.mk c d _ => walkPar n c || walkListPar n d
  termination_by _ x => sizeOf x

  def walkReceiveBind : Nat → ReceiveBind → Bool
    | 0, _ => true
    | n + 1, ReceiveBind.mk ps s _ => walkListPar n ps || walkPar n s
  termination_by _ x => sizeOf x

  def walkReceive : Nat → Receive → Bool
    | 0, _ => true
    | n + 1, Receive.mk bs b _ _ => walkListReceiveBind n bs || walkPar n b
  termination_by _ x => sizeOf x

  def walkNew : Nat → New → Bool
    | 0, _ => true
    | n + 1, New.mk _ b => walkPar n b
  termination_by _ x => sizeOf x

  def walkMatchCase : Nat → MatchCase → Bool
    | 0, _ => true
    | n + 1, MatchCase.mk p s _ => walkPar n p || walkPar n s
  termination_by _ x => sizeOf x

  def walkMatch : Nat → Match → Bool
    | 0, _ => true
    | n + 1, Match.mk t cs => walkPar n t || walkListMatchCase n cs
  termination_by _ x => sizeOf x

  def walkExpr : Nat → Expr → Bool
    | 0, _ => true
    | _ + 1, Expr.ground _ => false
    | _ + 1, Expr.evar _ => false
    | _ + 1, Expr.ebigint _ => false
    | n + 1, Expr.eneg p => walkPar n p
    | n + 1, Expr.enot p => walkPar n p
    | n + 1, Expr.eplus p q => walkPar n p || walkPar n q
    | n + 1, Expr.eminus p q => walkPar n p || walkPar n q
    | n + 1, Expr.emult p q => walkPar n p || walkPar n q
    | n + 1, Expr.ediv p q => walkPar n p || walkPar n q
    | n + 1, Expr.emod p q => walkPar n p || walkPar n q
    | n + 1, Expr.elt p q => walkPar n p || walkPar n q
    | n + 1, Expr.ele p q => walkPar n p || walkPar n q
    | n + 1, Expr.egt p q => walkPar n p || walkPar n q
    | n + 1, Expr.ege p q => walkPar n p || walkPar n q
    | n + 1, Expr.eeq p q => walkPar n p || walkPar n q
    | n + 1, Expr.eneq p q => walkPar n p || walkPar n q
    | n + 1, Expr.eand p q => walkPar n p || walkPar n q
    | n + 1, Expr.eor p q => walkPar n p || walkPar n q
    | n + 1, Expr.ematches p q => walkPar n p || walkPar n q
    | n + 1, Expr.eshortand p q => walkPar n p || walkPar n q
    | n + 1, Expr.eshortor p q => walkPar n p || walkPar n q
    | n + 1, Expr.elist ps _ => walkListPar n ps
    | n + 1, Expr.etuple ps => walkListPar n ps
    | n + 1, Expr.eset ps _ => walkListPar n ps
    | n + 1, Expr.emap kvs _ => walkListPair n kvs
    | n + 1, Expr.emethod _ p args => walkPar n p || walkListPar n args
    | n + 1, Expr.epercentPercent p q => walkPar n p || walkPar n q
    | n + 1, Expr.eplusPlus p q => walkPar n p || walkPar n q
    | n + 1, Expr.eminusMinus p q => walkPar n p || walkPar n q
  termination_by _ x => sizeOf x
  -- A two-`Par` arm leaves `sizeOf p < 1 + sizeOf p + sizeOf q`, which the default tactic does not
  -- discharge: it is true but needs arithmetic on `sizeOf q ≥ 0`, not structural comparison.

  def walkBundle : Nat → Bundle → Bool
    | 0, _ => true
    | n + 1, Bundle.mk p _ _ => walkPar n p
  termination_by _ x => sizeOf x

  def walkGUnforgeable : Nat → GUnforgeable → Bool
    | 0, _ => true
    | _ + 1, _ => false

  def walkConnective : Nat → Connective → Bool
    | 0, _ => true
    | n + 1, Connective.connAnd ps => walkListPar n ps
    | n + 1, Connective.connOr ps => walkListPar n ps
    | n + 1, Connective.connNot p => walkPar n p
    | _ + 1, Connective.connVarRef _ _ => false
  termination_by _ x => sizeOf x

  def walkListPar : Nat → List Par → Bool
    | _, [] => false
    | n, a :: as => walkPar n a || walkListPar n as
  termination_by _ x => sizeOf x

  def walkListPair : Nat → List (Par × Par) → Bool
    | _, [] => false
    | n, (a, b) :: as => walkPar n a || walkPar n b || walkListPair n as
  termination_by _ x => sizeOf x

  def walkListSend : Nat → List Send → Bool
    | _, [] => false
    | n, a :: as => walkSend n a || walkListSend n as
  termination_by _ x => sizeOf x

  def walkListReceive : Nat → List Receive → Bool
    | _, [] => false
    | n, a :: as => walkReceive n a || walkListReceive n as
  termination_by _ x => sizeOf x

  def walkListReceiveBind : Nat → List ReceiveBind → Bool
    | _, [] => false
    | n, a :: as => walkReceiveBind n a || walkListReceiveBind n as
  termination_by _ x => sizeOf x

  def walkListNew : Nat → List New → Bool
    | _, [] => false
    | n, a :: as => walkNew n a || walkListNew n as
  termination_by _ x => sizeOf x

  def walkListMatch : Nat → List Match → Bool
    | _, [] => false
    | n, a :: as => walkMatch n a || walkListMatch n as
  termination_by _ x => sizeOf x

  def walkListMatchCase : Nat → List MatchCase → Bool
    | _, [] => false
    | n, a :: as => walkMatchCase n a || walkListMatchCase n as
  termination_by _ x => sizeOf x

  def walkListExpr : Nat → List Expr → Bool
    | _, [] => false
    | n, a :: as => walkExpr n a || walkListExpr n as
  termination_by _ x => sizeOf x

  def walkListBundle : Nat → List Bundle → Bool
    | _, [] => false
    | n, a :: as => walkBundle n a || walkListBundle n as
  termination_by _ x => sizeOf x

  def walkListGUnforgeable : Nat → List GUnforgeable → Bool
    | _, [] => false
    | n, a :: as => walkGUnforgeable n a || walkListGUnforgeable n as
  termination_by _ x => sizeOf x

  def walkListConnective : Nat → List Connective → Bool
    | _, [] => false
    | n, a :: as => walkConnective n a || walkListConnective n as
  termination_by _ x => sizeOf x
end

/-- The node's own walk, as the parser applies it: `rholang/src/parser.rs::exceeds_ast_depth root limit`.
The budget is the limit, so `walkExceeds limit p = false` is the parser's acceptance. -/
def walkExceeds (limit : Nat) (p : Par) : Bool := walkPar limit p


/-! ## What is discharged, and what is not

The walk above is **defined** — an independent recursion, as law 50's row requires, so that the two
directions are not `rfl`. The **proof** that it agrees with `parDepth` is still owed, the row stays
`owed`, and `LAWS.md` still reads **2 owed** for it.

**Two attempts have been made, and the second found the root cause and got within one theorem of the
end.** What follows is what a third attempt needs, in the order it needs it.

**1. The walk must recurse on the *term*, with the budget as a parameter.** This is the whole reason
the first attempt hit a wall, and it is measured rather than guessed. The definitions above as
committed take the budget **first** (`walkPar : Nat → Par → Bool`, matched `| 0, _ => true | n + 1,
Par.mk …`), which makes the recursion decrease on the *budget* rather than on the term — so Lean needs
`termination_by`, and every theorem about them inherits a well-founded fixpoint. In a `mutual` *theorem*
block that fixpoint's termination goals are stated over the statement's binder rather than over the
pattern the arm matched, and they are **not provable as posed** (the context carries a fresh
`l : List T` beside the `a`/`as` the equation bound, with nothing saying they are equal).

Rewriting them to `walkPar (n : Nat) : Par → Bool` with

```
  | Par.mk s r nw e m u b c => if n = 0 then true else walkListSend n.pred s || …
```

— the budget as a parameter, `n.pred` at the recursive calls, the `0` case inside — makes the
recursion **structural on the term**, and Lean then infers termination across the mutual block with
**no `termination_by` and no `decreasing_by` at all**, exactly as `Rchain/FreeVars.lean` does. That
version compiles cleanly, definitions and all. **It is the shape the next attempt should start from.**

**2. The theorem bodies are then one `simp only` each, and they work.** With the structural definitions,
23 of the 24 theorems proved, in `FreeVars.lean`'s equation style with no termination clauses:

  * `if_neg`/`if_pos` discharge the budget test, and `Nat.pred_succ` and `Nat.add_one` normalise
    `(m + 1).pred` to `m`;
  * `Bool.or_eq_false_iff` turns the walk's `||` chain into a conjunction, `and_assoc` flattens the
    eight-way case, `Nat.max_le` splits `parDepth`'s `max` the same way, and `Nat.succ_le_succ_iff`
    relates `1 + max … ≤ m + 1` to `… ≤ m`;
  * `Nat.zero_le` closes the leaf arms (`True ↔ 0 ≤ m`) and the empty-list arms.

**3. The one that did not land, and it is a `Decidable` detail rather than a mathematical one.** The
remaining theorem is `walkPar_iff_parDepth`, and its goal after `simp only` is

```
⊢ (if (m.succ == 0) = true then true else walkListSend m s || … ) = false ↔
    listDepthSend s ≤ m ∧ … ∧ listDepthConnective c ≤ m
```

— the RHS is exactly right and the LHS is the right disjunction; all that is missing is discharging the
`if`. The condition is the **`BEq` form** (`(m.succ == 0) = true`), so `Nat.succ_ne_zero` alone does
not reach it, and the generic `beq_eq_false_iff_ne` makes `simp` report
`typeclass instance problem is stuck … Preorder ?m` — it is polymorphic and cannot be instantiated at
`Nat` from the goal. The next attempt should either give `simp` a **`Nat`-specific** form (or `decide`
on the closed condition), or write the definition's test as a `match` on the budget so no `Decidable`
enters at all:

```
  def walkPar : Par → Nat → Bool
    | Par.mk s r nw e m u b c, 0 => true
    | Par.mk s r nw e m u b c, k + 1 => walkListSend s k || …
```

That last form has `k` in the successor branch **syntactically**, which removes the `pred`, the
`Decidable`, and the arithmetic normalisation together — and it is the change this note would make
first.

**4. Why the walk had to be an independent recursion**, restated because the proof is what cashes it:
`decide (limit < parDepth p)` would make both directions `rfl` and prove nothing — the vacuity
`Rchain/Laws.lean` records for law 22.

**A note on the tooling, since it cost more than the mathematics**: two `replace_all` passes over this
file corrupted the *definitions* while I was editing the theorems (a pattern that matched inside the
`parDepth` block), and both were caught by the build and reverted. The committed state is the walk plus
this record, and `lake build Rchain.Depth` succeeds.
-/

end Rchain
