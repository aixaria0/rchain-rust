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
tree of subterms, field-wise on the flat `Par` exactly as `closed` and `freeVarOf` are. What ties it to
the parser's guard, `rholang/src/parser.rs::exceeds_ast_depth` — an iterative, early-exiting traversal,
iterative because the depth is the thing a recursive walk cannot survive — is

  * `walkExceeds limit p = false → parDepth p ≤ limit` — the direction that matters: a **sound**
    refusal test, so a term the node accepts cannot be deeper than the bound; and its control,
  * `parDepth p ≤ limit → walkExceeds limit p = false` — completeness, so the walk is not refusing by
    accident.

**Clause a's two directions are proved** (2026-09-28), by the walk and theorem blocks below, in
`Rchain/FreeVars.lean`'s shape: one `mutual` block with a member per type and per `List`,
`termination_by … => sizeOf …`, and a second `mutual` block for the walk so that it is an **independent
recursion** rather than `decide (limit < parDepth p)` — a walk defined as the depth it is checking
would make both directions `rfl`, which is the vacuity `Rchain/Laws.lean` records for law 22.

**Clause b is discharged too**, by `Rchain/ValueDepth.lean` — see the closing note, which is also where the correction to this file's
earlier misdiagnosis of its own first two attempts lives, and where the reason clause b is harder than
it looked is spelled out.

**The Rust half of the same risk** (a `Proc` constructor the walk does not descend into) is not a Lean
claim at all — the walk here is over the de Bruijn `Par`, while the parser's is over the surface
`Proc`. It is pinned by `rholang/src/parser.rs`'s every-constructor test and by the parser's own
refusal tests.
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

/-- A concrete term at a known depth — the witness shape the falsifier is stated over. A unit term is
depth 1, and nesting it under `n` unary nots gives depth `2 * n + 1`, which is the arithmetic a
falsifying mutation has to break.

**Each level costs two, not one, and this definition's first draft got that wrong** (found by this
unit's review, 2026-09-28). A not sits in a `Par`'s `List Expr` field, so a level is `1` for the `enot`
node (`exprDepth`) *and* `1` for the `Par` that holds it (`parDepth`). The first draft wrote
`List.replicate n (.enot unit)` — `n` **siblings**, not `n` nested levels — which has `parDepth` `3`
for every `n ≥ 1`. At budget `maxAstDepth` that term is accepted by the walk *and* by every mutant of
it, so the mutation test this row's `falsifiable` cell names would have passed **vacuously** — the
failure law 22 records, in the cell that cites law 22. Hence the nesting below, and hence `2 * n + 1`.

**Why this is a definition and not a `decide`d example** — the trap `spec/STYLE.md` records, met here:
`parDepth` is a `mutual` block over nested lists, so it needs `termination_by`, and a well-founded
definition is **not kernel-reducible** — `decide` cannot evaluate `parDepth (notsDepth 383)`, and the
attempt reports `reduction got stuck`. So law 50's two directions are proofs by induction, not
evaluations, and this term is their witness: `notsDepth n` has depth `2 * n + 1`, so `notsDepth 383`
reaches 767 — under `maxAstDepth` — and `notsDepth 384` is 769, the first term over it. -/
def notsDepth : Nat → Par
  | 0 => Par.mk [] [] [] [] [] [] [] []
  | n + 1 => Par.mk [] [] [] [Expr.enot (notsDepth n)] [] [] [] []

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
`Expr` node, so the two functions are not equal and the directions are *not* the trivial ones:

  * `walkExceeds limit p = false → parDepth p ≤ limit + (the element nesting the walk did not
    count)` — soundness with that slack, and the slack is what the proof has to bound; and its
    control, that the walk is not refusing by accident.

**This section's first draft understated the gap, and the correction is the whole of clause b's work**
(2026-09-28, AUDIT C169). It said the walk gives an `Expr` node no level, so the slack is *that node
alone*, bounded by `MAX_PARSE_DEPTH` (128) "because no runtime path builds `Expr` nodes". In fact the
walk gives **no element node** a level — `push_value_fields` pushes its fields at the depth it was
handed, as do `push_value_expr` and `push_value_connective`, so `Send`/`Receive`/`New`/`Match`/
`Bundle`/`MatchCase`/`Connective` are transparent along with `Expr` — and a runtime path *does* build
`Expr` nodes: `(a, b)` is `Expr::ETuple`, the shape `rholang/tests/deep_value_bound.rs` exists for. So
the counted quantity is not `parDepth` at all but the number of **`Par` nodes** on the deepest
`Par`-chain, and the gap is a factor: at most 3, and 3 is attained. **`Rchain/ValueDepth.lean` holds
that quantity (`parNestDepth`), its walk, the agreement, the bridge `parDepth p ≤ 3 * parNestDepth p`
and a machine-checked falsifier** — and the register's last two `owed` entries closed with it.
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
set_option maxHeartbeats 4000000 in
mutual
  def walkPar : Par → Nat → Bool
    | Par.mk s r nw e m u b c, k =>
        (match k with
         | 0 => true
         | j + 1 =>
             walkListSend s j || walkListReceive r j || walkListNew nw j || walkListExpr e j ||
             walkListMatch m j || walkListGUnforgeable u j || walkListBundle b j ||
             walkListConnective c j)
  termination_by p _ => sizeOf p

  def walkSend : Send → Nat → Bool
    | Send.mk c d _, k => (match k with | 0 => true | j + 1 => walkPar c j || walkListPar d j)
  termination_by s _ => sizeOf s

  def walkReceiveBind : ReceiveBind → Nat → Bool
    | ReceiveBind.mk ps s _, k => (match k with | 0 => true | j + 1 => walkListPar ps j || walkPar s j)
  termination_by b _ => sizeOf b

  def walkReceive : Receive → Nat → Bool
    | Receive.mk bs b _ _, k => (match k with | 0 => true | j + 1 => walkListReceiveBind bs j || walkPar b j)
  termination_by r _ => sizeOf r

  def walkNew : New → Nat → Bool
    | New.mk _ b, k => (match k with | 0 => true | j + 1 => walkPar b j)
  termination_by n _ => sizeOf n

  def walkMatchCase : MatchCase → Nat → Bool
    | MatchCase.mk p s _, k => (match k with | 0 => true | j + 1 => walkPar p j || walkPar s j)
  termination_by m _ => sizeOf m

  def walkMatch : Match → Nat → Bool
    | Match.mk t cs, k => (match k with | 0 => true | j + 1 => walkPar t j || walkListMatchCase cs j)
  termination_by m _ => sizeOf m

  def walkExpr : Expr → Nat → Bool
    | Expr.ground _, k | Expr.evar _, k | Expr.ebigint _, k =>
        (match k with | 0 => true | _ + 1 => false)
    | Expr.eneg p, k | Expr.enot p, k =>
        (match k with | 0 => true | j + 1 => walkPar p j)
    | Expr.eplus p q, k | Expr.eminus p q, k | Expr.emult p q, k | Expr.ediv p q, k
    | Expr.emod p q, k | Expr.elt p q, k | Expr.ele p q, k | Expr.egt p q, k
    | Expr.ege p q, k | Expr.eeq p q, k | Expr.eneq p q, k | Expr.eand p q, k
    | Expr.eor p q, k | Expr.ematches p q, k | Expr.eshortand p q, k
    | Expr.eshortor p q, k =>
        (match k with | 0 => true | j + 1 => walkPar p j || walkPar q j)
    | Expr.elist ps _, k | Expr.etuple ps, k | Expr.eset ps _, k =>
        (match k with | 0 => true | j + 1 => walkListPar ps j)
    | Expr.emap kvs _, k => (match k with | 0 => true | j + 1 => walkListPair kvs j)
    | Expr.emethod _ p args, k =>
        (match k with | 0 => true | j + 1 => walkPar p j || walkListPar args j)
    | Expr.epercentPercent p q, k | Expr.eplusPlus p q, k | Expr.eminusMinus p q, k =>
        (match k with | 0 => true | j + 1 => walkPar p j || walkPar q j)
  termination_by e _ => sizeOf e
  -- A two-`Par` arm leaves `sizeOf p < 1 + sizeOf p + sizeOf q`, which the default tactic does not
  -- discharge: it is true but needs arithmetic on `sizeOf q ≥ 0`, not structural comparison.

  def walkBundle : Bundle → Nat → Bool
    | Bundle.mk p _ _, k => (match k with | 0 => true | j + 1 => walkPar p j)
  termination_by b _ => sizeOf b

  def walkGUnforgeable : GUnforgeable → Nat → Bool
    | _, k => (match k with | 0 => true | _ + 1 => false)

  def walkConnective : Connective → Nat → Bool
    | Connective.connAnd ps, k | Connective.connOr ps, k =>
        (match k with | 0 => true | j + 1 => walkListPar ps j)
    | Connective.connNot p, k => (match k with | 0 => true | j + 1 => walkPar p j)
    | Connective.connVarRef _ _, k => (match k with | 0 => true | _ + 1 => false)
  termination_by c _ => sizeOf c

  def walkListPar : List Par → Nat → Bool
    | [], _ => false
    | a :: as, k => walkPar a k || walkListPar as k
  termination_by l _ => sizeOf l

  def walkListPair : List (Par × Par) → Nat → Bool
    | [], _ => false
    | (a, b) :: as, k => walkPar a k || walkPar b k || walkListPair as k
  termination_by l _ => sizeOf l

  def walkListSend : List Send → Nat → Bool
    | [], _ => false
    | a :: as, k => walkSend a k || walkListSend as k
  termination_by l _ => sizeOf l

  def walkListReceive : List Receive → Nat → Bool
    | [], _ => false
    | a :: as, k => walkReceive a k || walkListReceive as k
  termination_by l _ => sizeOf l

  def walkListReceiveBind : List ReceiveBind → Nat → Bool
    | [], _ => false
    | a :: as, k => walkReceiveBind a k || walkListReceiveBind as k
  termination_by l _ => sizeOf l

  def walkListNew : List New → Nat → Bool
    | [], _ => false
    | a :: as, k => walkNew a k || walkListNew as k
  termination_by l _ => sizeOf l

  def walkListMatch : List Match → Nat → Bool
    | [], _ => false
    | a :: as, k => walkMatch a k || walkListMatch as k
  termination_by l _ => sizeOf l

  def walkListMatchCase : List MatchCase → Nat → Bool
    | [], _ => false
    | a :: as, k => walkMatchCase a k || walkListMatchCase as k
  termination_by l _ => sizeOf l

  def walkListExpr : List Expr → Nat → Bool
    | [], _ => false
    | a :: as, k => walkExpr a k || walkListExpr as k
  termination_by l _ => sizeOf l

  def walkListBundle : List Bundle → Nat → Bool
    | [], _ => false
    | a :: as, k => walkBundle a k || walkListBundle as k
  termination_by l _ => sizeOf l

  def walkListGUnforgeable : List GUnforgeable → Nat → Bool
    | [], _ => false
    | a :: as, k => walkGUnforgeable a k || walkListGUnforgeable as k
  termination_by l _ => sizeOf l

  def walkListConnective : List Connective → Nat → Bool
    | [], _ => false
    | a :: as, k => walkConnective a k || walkListConnective as k
  termination_by l _ => sizeOf l
end

/-- The node's own walk, as the parser applies it: `rholang/src/parser.rs::exceeds_ast_depth root limit`.
The budget is the limit, so `walkExceeds limit p = false` is the parser's acceptance. -/
def walkExceeds (limit : Nat) (p : Par) : Bool := walkPar p limit

/-! ## The agreement — the walk and `parDepth`

The 23 theorems below are one `mutual` block, mirroring `Rchain/FreeVars.lean`'s `freeVarOf*_iff_closed`
block member for member: each is the walk's own recursion, and each recursive call is discharged by
`simp` rewriting with the *sibling theorem's statement*. The block carries **no `termination_by` and no
`decreasing_by`** — the walk recurses on the term, so the equation compiler infers termination exactly
as it does for `FreeVars`.

**Why the definitions had to be reshaped for this**, since the module's earlier note got the cause
wrong: the extra `Nat` is a *parameter* here (`walkPar : Par → Nat → Bool`, matched `| Par.mk …, k =>`),
not a pattern-matched first argument. That is `FreeVars`' shape — it carries `termination_by` on its
definitions *and* a theorem block with no termination clause, which is what falsified the earlier note's
claim that `termination_by` was the obstacle. The budget test lives in the *body*, as a `match`, so `j`
appears syntactically in the successor arm: no `if`, no `Decidable`, no `Nat.pred` ever reaches a goal,
which is what the previous attempt's `if n = 0` version could not escape.

**`omega` does the arithmetic, and the named lemmas do not.** Measured: `Nat.succ_le_succ_iff` cannot
see through `1 + _` (`Nat.add` recurses on its second argument, so `1 + x` does not reduce to
`Nat.succ x`), and `Nat.max_le`/`and_assoc`/`Bool.or_eq_false_iff` in a `simp only` set made no progress
on the assembled goal. `omega` alone closes `A ≤ j ∧ B ≤ j ↔ 1 + max A B ≤ j + 1` and the eight-way
nested `max`, so none of the twelve list members needs a `List.max` lemma.

**No positivity lemma is needed**, which is what the second attempt expected to need: every
zero-budget goal is over a *known constructor* — the arm's own pattern — so the depth reduces to a
numeral-headed `1 + max …` and `simp` closes it. Positivity would be needed only against
`walkPar.induct`, whose budget-0 case quantifies over an arbitrary term.

**And `max` must be left alone.** An earlier draft added `max_def` to these sets, which turns `max` into
an `if` — and `omega` then treats the branches as opaque variables and cannot relate them, which is
what its counterexample reports. `omega` reads `max` natively; the temptation to normalise it away is
the trap here.
-/
set_option maxHeartbeats 4000000 in
mutual
  theorem walkPar_iff_parDepth : (p : Par) → ∀ k, walkPar p k = false ↔ parDepth p ≤ k
    | Par.mk s r nw e m u b c, k => by
      cases k with
      | zero => simp [walkPar, parDepth]
      | succ j =>
        simp only [walkPar, parDepth, Bool.or_eq_false_iff, and_assoc,
          walkListSend_iff_listDepthSend, walkListReceive_iff_listDepthReceive,
          walkListNew_iff_listDepthNew, walkListExpr_iff_listDepthExpr,
          walkListMatch_iff_listDepthMatch, walkListGUnforgeable_iff_listDepthGUnforgeable,
          walkListBundle_iff_listDepthBundle, walkListConnective_iff_listDepthConnective]
        omega

  theorem walkSend_iff_sendDepth : (s : Send) → ∀ k, walkSend s k = false ↔ sendDepth s ≤ k
    | Send.mk c d _, k => by
      cases k with
      | zero => simp [walkSend, sendDepth]
      | succ j =>
        simp only [walkSend, sendDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth, walkListPar_iff_listDepthPar]
        omega

  theorem walkReceiveBind_iff_receiveBindDepth :
      (b : ReceiveBind) → ∀ k, walkReceiveBind b k = false ↔ receiveBindDepth b ≤ k
    | ReceiveBind.mk ps s _, k => by
      cases k with
      | zero => simp [walkReceiveBind, receiveBindDepth]
      | succ j =>
        simp only [walkReceiveBind, receiveBindDepth, Bool.or_eq_false_iff,
          walkListPar_iff_listDepthPar, walkPar_iff_parDepth]
        omega

  theorem walkReceive_iff_receiveDepth :
      (r : Receive) → ∀ k, walkReceive r k = false ↔ receiveDepth r ≤ k
    | Receive.mk bs b _ _, k => by
      cases k with
      | zero => simp [walkReceive, receiveDepth]
      | succ j =>
        simp only [walkReceive, receiveDepth, Bool.or_eq_false_iff,
          walkListReceiveBind_iff_listDepthReceiveBind, walkPar_iff_parDepth]
        omega

  theorem walkNew_iff_newDepth : (nw : New) → ∀ k, walkNew nw k = false ↔ newDepth nw ≤ k
    | New.mk _ b, k => by
      cases k with
      | zero => simp [walkNew, newDepth]
      | succ j =>
        simp only [walkNew, newDepth, walkPar_iff_parDepth]
        omega

  theorem walkMatchCase_iff_matchCaseDepth :
      (m : MatchCase) → ∀ k, walkMatchCase m k = false ↔ matchCaseDepth m ≤ k
    | MatchCase.mk p s _, k => by
      cases k with
      | zero => simp [walkMatchCase, matchCaseDepth]
      | succ j =>
        simp only [walkMatchCase, matchCaseDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth]
        omega

  theorem walkMatch_iff_matchDepth :
      (m : Match) → ∀ k, walkMatch m k = false ↔ matchDepth m ≤ k
    | Match.mk t cs, k => by
      cases k with
      | zero => simp [walkMatch, matchDepth]
      | succ j =>
        simp only [walkMatch, matchDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth, walkListMatchCase_iff_listDepthMatchCase]
        omega

  theorem walkExpr_iff_exprDepth : (e : Expr) → ∀ k, walkExpr e k = false ↔ exprDepth e ≤ k
    | Expr.ground _, k | Expr.evar _, k | Expr.ebigint _, k => by
      cases k <;> simp [walkExpr, exprDepth]
    | Expr.eneg p, k | Expr.enot p, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, walkPar_iff_parDepth]
        omega
    | Expr.eplus p q, k | Expr.eminus p q, k | Expr.emult p q, k | Expr.ediv p q, k
    | Expr.emod p q, k | Expr.elt p q, k | Expr.ele p q, k | Expr.egt p q, k
    | Expr.ege p q, k | Expr.eeq p q, k | Expr.eneq p q, k | Expr.eand p q, k
    | Expr.eor p q, k | Expr.ematches p q, k | Expr.eshortand p q, k
    | Expr.eshortor p q, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth]
        omega
    | Expr.elist ps _, k | Expr.etuple ps, k | Expr.eset ps _, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, walkListPar_iff_listDepthPar]
        omega
    | Expr.emap kvs _, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, walkListPair_iff_listDepthPair]
        omega
    | Expr.emethod _ p args, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth, walkListPar_iff_listDepthPar]
        omega
    | Expr.epercentPercent p q, k | Expr.eplusPlus p q, k | Expr.eminusMinus p q, k => by
      cases k with
      | zero => simp [walkExpr, exprDepth]
      | succ j =>
        simp only [walkExpr, exprDepth, Bool.or_eq_false_iff,
          walkPar_iff_parDepth]
        omega

  theorem walkBundle_iff_bundleDepth :
      (b : Bundle) → ∀ k, walkBundle b k = false ↔ bundleDepth b ≤ k
    | Bundle.mk p _ _, k => by
      cases k with
      | zero => simp [walkBundle, bundleDepth]
      | succ j =>
        simp only [walkBundle, bundleDepth, walkPar_iff_parDepth]
        omega

  theorem walkGUnforgeable_iff_gUnforgeableDepth :
      (u : GUnforgeable) → ∀ k, walkGUnforgeable u k = false ↔ gUnforgeableDepth u ≤ k
    | GUnforgeable.gPrivate _, k => by cases k <;> simp [walkGUnforgeable, gUnforgeableDepth]
    | GUnforgeable.gDeployId _, k => by cases k <;> simp [walkGUnforgeable, gUnforgeableDepth]
    | GUnforgeable.gDeployerId, k => by cases k <;> simp [walkGUnforgeable, gUnforgeableDepth]
    | GUnforgeable.gSysAuthToken, k => by cases k <;> simp [walkGUnforgeable, gUnforgeableDepth]

  theorem walkConnective_iff_connectiveDepth :
      (c : Connective) → ∀ k, walkConnective c k = false ↔ connectiveDepth c ≤ k
    | Connective.connAnd ps, k | Connective.connOr ps, k => by
      cases k with
      | zero => simp [walkConnective, connectiveDepth]
      | succ j =>
        simp only [walkConnective, connectiveDepth, walkListPar_iff_listDepthPar]
        omega
    | Connective.connNot p, k => by
      cases k with
      | zero => simp [walkConnective, connectiveDepth]
      | succ j =>
        simp only [walkConnective, connectiveDepth, walkPar_iff_parDepth]
        omega
    | Connective.connVarRef _ _, k => by
      cases k <;> simp [walkConnective, connectiveDepth]

  theorem walkListPar_iff_listDepthPar :
      (l : List Par) → ∀ k, walkListPar l k = false ↔ listDepthPar l ≤ k
    | [], k => by simp [walkListPar, listDepthPar]
    | a :: as, k => by
      simp only [walkListPar, listDepthPar, Bool.or_eq_false_iff,
        walkPar_iff_parDepth, walkListPar_iff_listDepthPar]
      omega

  theorem walkListPair_iff_listDepthPair :
      (l : List (Par × Par)) → ∀ k, walkListPair l k = false ↔ listDepthPair l ≤ k
    | [], k => by simp [walkListPair, listDepthPair]
    | (a, b) :: as, k => by
      simp only [walkListPair, listDepthPair, Bool.or_eq_false_iff,
        walkPar_iff_parDepth, walkListPair_iff_listDepthPair]
      omega

  theorem walkListSend_iff_listDepthSend :
      (l : List Send) → ∀ k, walkListSend l k = false ↔ listDepthSend l ≤ k
    | [], k => by simp [walkListSend, listDepthSend]
    | a :: as, k => by
      simp only [walkListSend, listDepthSend, Bool.or_eq_false_iff,
        walkSend_iff_sendDepth, walkListSend_iff_listDepthSend]
      omega

  theorem walkListReceive_iff_listDepthReceive :
      (l : List Receive) → ∀ k, walkListReceive l k = false ↔ listDepthReceive l ≤ k
    | [], k => by simp [walkListReceive, listDepthReceive]
    | a :: as, k => by
      simp only [walkListReceive, listDepthReceive, Bool.or_eq_false_iff,
        walkReceive_iff_receiveDepth, walkListReceive_iff_listDepthReceive]
      omega

  theorem walkListReceiveBind_iff_listDepthReceiveBind :
      (l : List ReceiveBind) → ∀ k, walkListReceiveBind l k = false ↔ listDepthReceiveBind l ≤ k
    | [], k => by simp [walkListReceiveBind, listDepthReceiveBind]
    | a :: as, k => by
      simp only [walkListReceiveBind, listDepthReceiveBind, Bool.or_eq_false_iff,
        walkReceiveBind_iff_receiveBindDepth, walkListReceiveBind_iff_listDepthReceiveBind]
      omega

  theorem walkListNew_iff_listDepthNew :
      (l : List New) → ∀ k, walkListNew l k = false ↔ listDepthNew l ≤ k
    | [], k => by simp [walkListNew, listDepthNew]
    | a :: as, k => by
      simp only [walkListNew, listDepthNew, Bool.or_eq_false_iff,
        walkNew_iff_newDepth, walkListNew_iff_listDepthNew]
      omega

  theorem walkListMatch_iff_listDepthMatch :
      (l : List Match) → ∀ k, walkListMatch l k = false ↔ listDepthMatch l ≤ k
    | [], k => by simp [walkListMatch, listDepthMatch]
    | a :: as, k => by
      simp only [walkListMatch, listDepthMatch, Bool.or_eq_false_iff,
        walkMatch_iff_matchDepth, walkListMatch_iff_listDepthMatch]
      omega

  theorem walkListMatchCase_iff_listDepthMatchCase :
      (l : List MatchCase) → ∀ k, walkListMatchCase l k = false ↔ listDepthMatchCase l ≤ k
    | [], k => by simp [walkListMatchCase, listDepthMatchCase]
    | a :: as, k => by
      simp only [walkListMatchCase, listDepthMatchCase, Bool.or_eq_false_iff,
        walkMatchCase_iff_matchCaseDepth, walkListMatchCase_iff_listDepthMatchCase]
      omega

  theorem walkListExpr_iff_listDepthExpr :
      (l : List Expr) → ∀ k, walkListExpr l k = false ↔ listDepthExpr l ≤ k
    | [], k => by simp [walkListExpr, listDepthExpr]
    | a :: as, k => by
      simp only [walkListExpr, listDepthExpr, Bool.or_eq_false_iff,
        walkExpr_iff_exprDepth, walkListExpr_iff_listDepthExpr]
      omega

  theorem walkListBundle_iff_listDepthBundle :
      (l : List Bundle) → ∀ k, walkListBundle l k = false ↔ listDepthBundle l ≤ k
    | [], k => by simp [walkListBundle, listDepthBundle]
    | a :: as, k => by
      simp only [walkListBundle, listDepthBundle, Bool.or_eq_false_iff,
        walkBundle_iff_bundleDepth, walkListBundle_iff_listDepthBundle]
      omega

  theorem walkListGUnforgeable_iff_listDepthGUnforgeable :
      (l : List GUnforgeable) → ∀ k, walkListGUnforgeable l k = false ↔ listDepthGUnforgeable l ≤ k
    | [], k => by simp [walkListGUnforgeable, listDepthGUnforgeable]
    | a :: as, k => by
      simp only [walkListGUnforgeable, listDepthGUnforgeable, Bool.or_eq_false_iff,
        walkGUnforgeable_iff_gUnforgeableDepth, walkListGUnforgeable_iff_listDepthGUnforgeable]
      omega

  theorem walkListConnective_iff_listDepthConnective :
      (l : List Connective) → ∀ k, walkListConnective l k = false ↔ listDepthConnective l ≤ k
    | [], k => by simp [walkListConnective, listDepthConnective]
    | a :: as, k => by
      simp only [walkListConnective, listDepthConnective, Bool.or_eq_false_iff,
        walkConnective_iff_connectiveDepth, walkListConnective_iff_listDepthConnective]
      omega
end

/-- **Soundness** — the direction the parser's budget exists for: a term the walk accepts is never
deeper than the budget it was accepted at. -/
theorem walkExceeds_sound (limit : Nat) (p : Par) : walkExceeds limit p = false → parDepth p ≤ limit :=
  (walkPar_iff_parDepth p limit).mp

/-- **Completeness** — the control: the walk is not refusing by accident, so the bound refuses exactly
what is too deep and nothing else. -/
theorem walkExceeds_complete (limit : Nat) (p : Par) : parDepth p ≤ limit → walkExceeds limit p = false :=
  (walkPar_iff_parDepth p limit).mpr

/-! ## The falsifier — a dropped arm must break soundness

The register's `falsifiable` cell for law 50a names `a_dropped_arm_breaks_soundness` and says a walk
that could not be broken by dropping one of its children arms "would be a restatement of the definition
rather than a check of it". This is that theorem, machine-checked rather than recorded in a comment
(`Rchain/Corpus.lean:664` is the lighter house form, and it is not enough here: the row names a theorem).

The witness is `notsDepth 384`, whose depth is `2 * 384 + 1 = 769` — one past `maxAstDepth`. -/

/-- `notsDepth`'s depth, so the falsifier's arithmetic is a theorem rather than a claim in a comment. -/
theorem parDepth_notsDepth (n : Nat) : parDepth (notsDepth n) = 2 * n + 1 := by
  induction n with
  | zero =>
      simp [notsDepth, parDepth, listDepthSend, listDepthReceive, listDepthNew, listDepthExpr,
        listDepthMatch, listDepthGUnforgeable, listDepthBundle, listDepthConnective]
  | succ k ih =>
      simp [notsDepth, parDepth, exprDepth, listDepthExpr, listDepthSend, listDepthReceive,
        listDepthNew, listDepthMatch, listDepthGUnforgeable, listDepthBundle, listDepthConnective, ih]
      omega

/-- **The walk with its `exprs` arm dropped** — the mutation the falsifier is about, as a definition
rather than a described edit. The other seven children arms are the real walk's own list functions: the
witness below has an empty `Send`/`Receive`/`New`/`Match`/`GUnforgeable`/`Bundle`/`Connective` field, so
its value does not depend on that delegation — what it depends on is that the `e` field is never
visited, which is exactly the arm deleted. -/
def walkParDroppingExpr : Par → Nat → Bool
  | Par.mk s r nw _ m u b c, k =>
      (match k with
       | 0 => true
       | j + 1 =>
           walkListSend s j || walkListReceive r j || walkListNew nw j || walkListMatch m j ||
           walkListGUnforgeable u j || walkListBundle b j || walkListConnective c j)

/-- The mutant accepts `notsDepth 384` at the budget of `maxAstDepth`. -/
theorem mutant_accepts_nots384 : walkParDroppingExpr (notsDepth 384) 768 = false := by
  change walkParDroppingExpr (Par.mk [] [] [] [Expr.enot (notsDepth 383)] [] [] [] []) 768 = false
  simp [walkParDroppingExpr, walkListSend, walkListReceive, walkListNew, walkListMatch,
    walkListGUnforgeable, walkListBundle, walkListConnective]

/-- **The falsifier the row names.** The mutant is *unsound* at its witness: it accepts a term whose
depth is 769 while claiming no term past 768 is accepted. So the agreement proved above is a check of
the walk and not a restatement of the definition — the vacuity law 22 records is not what this is. -/
theorem a_dropped_arm_breaks_soundness :
    walkParDroppingExpr (notsDepth 384) 768 = false ∧ ¬ (parDepth (notsDepth 384) ≤ 768) := by
  refine ⟨mutant_accepts_nots384, ?_⟩
  rw [parDepth_notsDepth]
  omega

/-- The contrast, so the pair is a test rather than a slogan: the **real** walk refuses the very term
the mutant admits, at the same budget. -/
theorem the_walk_refuses_the_mutant_witness : ¬ (walkPar (notsDepth 384) 768 = false) := by
  intro h
  have hd := (walkPar_iff_parDepth (notsDepth 384) 768).mp h
  rw [parDepth_notsDepth] at hd
  omega


/-! ## What is discharged, and what is not

**Clause a is discharged** (2026-09-28). The walk above is an independent recursion, as law 50's row
requires, so the two directions were never `rfl` — and the 23-member `mutual` block above now proves
them: `walkPar_iff_parDepth` is the agreement, `walkExceeds_sound`/`walkExceeds_complete` are the two
directions the row quotes, and `a_dropped_arm_breaks_soundness` is the falsifier the row names, as a
theorem rather than a comment.

**The two earlier attempts misdiagnosed the wall, and the corrected cause is worth keeping.** The note
this replaces claimed that budget-first recursion forces `termination_by`, and that a `termination_by`'d
mutual definition drags its `mutual` *theorem* block into a well-founded fixpoint whose goals "are not
provable as posed". `Rchain/FreeVars.lean` — the file the register itself names as the recipe —
falsifies that: its definitions **do** carry `termination_by`, and its 23 `freeVarOf*_iff_closed`
theorems sit in a block with **no `termination_by` and no `decreasing_by` at all**. The variable is
where the extra `Nat` sits. `FreeVars` takes the recursed term first with the level as a *plain pattern
variable*; the walk as first committed matched the budget *in the equation header*
(`| 0, _ => true | n + 1, Par.mk …`), which is what routed it through a fixpoint. The reshape above —
term first, budget a parameter covered by a body-level `match` — is `FreeVars`' shape, and it is why
the theorem block needs no termination clause. Dropping `termination_by` was never necessary, and the
attempt that did drop it paid for the privilege in `Decidable`.

**Four things this cost, recorded because each one cost an attempt:**

  * **The `match` must be parenthesised.** Written bare, Lean reads its arms as further *equations* of
    the enclosing function: `incorrect number of patterns`, then `unknown identifier 'k'`.
  * **A multi-line `|`-alternation must repeat the trailing pattern** in every alternative —
    `| Expr.eneg p, k | Expr.enot p, k => …` works, `| Expr.eneg p | Expr.enot p, k =>` does not.
  * **`max_def` must stay out of the `simp` set.** It rewrites `max` into an `if`, and `omega` then
    treats the branches as opaque variables and cannot relate them. `omega` reads `max` natively.
  * **`maxHeartbeats 4000000`**, on *both* blocks — the definitions and the theorems. 200,000 times out
    on the 16-arm `walkExpr`; the scope of the earlier `1000000` did not cover the theorems at all.

**And the witness the row's falsifier rests on was wrong.** `notsDepth` was `List.replicate n (enot unit)`
— `n` *siblings*, so `parDepth` was `3` for every `n ≥ 1`, and at budget `maxAstDepth` both the walk and
every mutant of it accepted the term. The mutation test would have passed **vacuously** — the failure
law 22 records, in the cell that cites law 22. It now nests, `parDepth (notsDepth n) = 2 * n + 1`, and
`parDepth_notsDepth` proves that rather than asserting it.

**What clause a does not claim.** `rholang/src/parser.rs::exceeds_ast_depth` walks the surface `Proc`
tree, not the de Bruijn `Par` this file is about; the parser's own doc comment calls its count "a proxy
— and a deliberate one", since the `Par` "adds a small constant per construct". So
`walkPar_iff_parDepth` is an agreement *inside the model*, and the bridge to the parser's tree is a
modelling argument. The half no theorem can state — that the Rust walk descends into every `Proc`
constructor — is pinned by `rholang/src/parser.rs`'s every-constructor test.

**Clause b is discharged too, and it needed a second quantity to be.** Its statement as committed was
*not true*: it said the value route's walk gives an `Expr` node no level of its own where `parDepth`
counts it, and that soundness therefore carries a slack bounded by `MAX_PARSE_DEPTH` (128). The walk in
fact charges a level for **`Par` nodes only** — `models/src/types.rs`'s
`push_value_fields`/`push_value_expr`/`push_value_connective` push every *element* at the same depth
they were handed, so `Expr` and `Connective` are transparent along with
`Send`/`Receive`/`New`/`Match`/`Bundle`/`MatchCase` — and the counted quantity is the number of `Par`
nodes on the deepest `Par`-chain, which is not `parDepth`. The "no runtime path builds `Expr` nodes"
premise is false as well (`(a, b)` **is** `Expr::ETuple`, built by the reducer, and the fold test exists
for that shape), and the gap is a *factor*, not a constant: at `maxValueDepth` the guard admits
`pairsDepth 255`, whose `parDepth` is 511. **That second quantity and walk are `Rchain/ValueDepth.lean`
— `parNestDepth` and `walkValuePar`, with the agreement, the bridge `parDepth p ≤ 3 * parNestDepth p`
(three, and tight), and a machine-checked falsifier.** The register's last two `owed` entries closed
together; `spec/LAWS.md` reads **0 owed**.

**Why the walk had to be an independent recursion**, restated because the proof is what cashes it:
`decide (limit < parDepth p)` would make both directions `rfl` and prove nothing — the vacuity
`Rchain/Laws.lean` records for law 22.
-/

end Rchain
