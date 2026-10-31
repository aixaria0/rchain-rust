import Rchain.Depth

/-!
# Law 50 clause b — the quantity the *space*'s bound is about

`rholang/src/storage.rs::MAX_VALUE_DEPTH` (256) bounds a **runtime-built value**: a contract folding its
accumulator into a deeper pair reaches depth `n` in `O(n)` reduce steps, and the parser never sees that
term. The guard is `models/src/types.rs::exceeds_value_depth`, called from
`rholang/src/storage.rs::ChargingRSpace::check_value_depth` on both produce paths.

**Why this is its own quantity and not `parDepth`.** Clause a's quantity counts *every* node: `parDepth`
is `1 + max` at each construct, so a `Send`, an `Expr` and a `Connective` each cost a level. The value
walk does not. `push_value_fields` pushes a `Par`'s field *elements* at the same depth `d`, and
`push_value_expr`/`push_value_connective` push their `Par` children at that same `d` too — so in the
value route only a **`Par` node** consumes a level, and the counted quantity is the number of `Par`
nodes on the deepest `Par`-chain. That is `parNestDepth` below: `+1` at `Par`, `max` with no `+1` at
every element and every list.

`spec/Rchain/Depth.lean`'s closing note records why the clause's first statement could not be proved:
it claimed the gap was the `Expr` node alone, bounded by `MAX_PARSE_DEPTH` (128) because "no runtime
path builds `Expr` nodes". Every clause of that is false. `Connective` is transparent too; `(a, b)`
**is** `Expr::ETuple`, built by the reducer (`rholang/src/reduce.rs:947,953,1107`), which is the shape
`rholang/tests/deep_value_bound.rs` exists to exercise; and the gap is a *factor* that grows with
nesting, not a constant — at `maxValueDepth` the guard admits `pairsDepth 255`, whose `parDepth` is
**511**. So the honest statement is in terms of `parNestDepth`, and `parDepth` is related to it by
`parDepth_le_three_mul_parNestDepth` rather than equated with it. **Three, not two, and it is tight**:
two element nodes can sit between consecutive `Par`s (`Receive`→`ReceiveBind`, `Match`→`MatchCase`), so
a `Match` ladder spends three levels per `Par` and the ratio tends to three. At `maxValueDepth` that
makes the bound `3 × 256 = 768` — the same number as `maxAstDepth`, which is what makes the two
clauses' constants a pair rather than two unrelated measurements.

**Two boundaries carried rather than papered over**, both from the Rust:

  * The walk checks a node **when it pops it**, and the root is never pushed — its *fields* are pushed
    at depth 2. So `exceeds_value_depth` at `limit = 0` accepts a value with no `Par` child, while
    `walkValuePar` below (which checks its own argument) refuses it. The two agree for every `limit ≥ 1`,
    and `maxValueDepth` is 256; `valueWalkExceeds_iff_value_depth` states the agreement at that side
    condition rather than hiding it.
  * `push_value_fields` walks a `New`'s `injections` map (`models/src/types.rs:713-717`), and the model's
    `New.mk` has no such field (`Rchain/Par.lean`). The guard is therefore *stricter* than the model
    there — the safe direction, and named rather than elided.
-/

namespace Rchain

/-! ## The quantity — a `Par` costs a level, an element does not

Field-wise on the flat `Par` exactly as `parDepth` is, with one difference: no element construct and no
list adds a level. `gUnforgeableNestDepth` is `0` for the same reason `parDepth`'s is `1` — a
`GUnforgeable` carries ids and byte strings, never a `Par`.
-/
mutual
  /-- The number of `Par` nodes on the longest `Par`-chain below `p`, counting `p` itself as one. The
  quantity `models/src/types.rs::exceeds_value_depth` walks. -/
  def parNestDepth : Par → Nat
    | Par.mk s r nw e m u b c =>
        1 + max (listNestSend s)
              (max (listNestReceive r)
              (max (listNestNew nw)
              (max (listNestExpr e)
              (max (listNestMatch m)
              (max (listNestGUnforgeable u)
              (max (listNestBundle b)
                   (listNestConnective c)))))))
  termination_by p => sizeOf p

  def sendNestDepth : Send → Nat
    | Send.mk c d _ => max (parNestDepth c) (listNestPar d)
  termination_by s => sizeOf s

  def receiveBindNestDepth : ReceiveBind → Nat
    | ReceiveBind.mk ps s _ => max (listNestPar ps) (parNestDepth s)
  termination_by b => sizeOf b

  def receiveNestDepth : Receive → Nat
    | Receive.mk bs b _ _ => max (listNestReceiveBind bs) (parNestDepth b)
  termination_by r => sizeOf r

  def newNestDepth : New → Nat
    | New.mk _ b => parNestDepth b
  termination_by n => sizeOf n

  def matchCaseNestDepth : MatchCase → Nat
    | MatchCase.mk p s _ => max (parNestDepth p) (parNestDepth s)
  termination_by m => sizeOf m

  def matchNestDepth : Match → Nat
    | Match.mk t cs => max (parNestDepth t) (listNestMatchCase cs)
  termination_by m => sizeOf m

  /-- `Expr` is **transparent** in this quantity: `push_value_expr` pushes its `Par` children at the
  depth it was handed, so an `Expr` node costs nothing. -/
  def exprNestDepth : Expr → Nat
    | Expr.ground _ => 0
    | Expr.evar _ => 0
    | Expr.eneg p => parNestDepth p
    | Expr.enot p => parNestDepth p
    | Expr.eplus p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eminus p q => max (parNestDepth p) (parNestDepth q)
    | Expr.emult p q => max (parNestDepth p) (parNestDepth q)
    | Expr.ediv p q => max (parNestDepth p) (parNestDepth q)
    | Expr.emod p q => max (parNestDepth p) (parNestDepth q)
    | Expr.elt p q => max (parNestDepth p) (parNestDepth q)
    | Expr.ele p q => max (parNestDepth p) (parNestDepth q)
    | Expr.egt p q => max (parNestDepth p) (parNestDepth q)
    | Expr.ege p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eeq p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eneq p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eand p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eor p q => max (parNestDepth p) (parNestDepth q)
    | Expr.ematches p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eshortand p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eshortor p q => max (parNestDepth p) (parNestDepth q)
    | Expr.elist ps _ => listNestPar ps
    | Expr.etuple ps => listNestPar ps
    | Expr.eset ps _ => listNestPar ps
    | Expr.emap kvs _ => listNestPair kvs
    | Expr.ebigint _ => 0
    | Expr.emethod _ p args => max (parNestDepth p) (listNestPar args)
    | Expr.epercentPercent p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eplusPlus p q => max (parNestDepth p) (parNestDepth q)
    | Expr.eminusMinus p q => max (parNestDepth p) (parNestDepth q)
  termination_by e => sizeOf e

  def bundleNestDepth : Bundle → Nat
    | Bundle.mk p _ _ => parNestDepth p
  termination_by b => sizeOf b

  /-- A `GUnforgeable` carries no `Par`, so it contributes no level. -/
  def gUnforgeableNestDepth : GUnforgeable → Nat
    | GUnforgeable.gPrivate _ => 0
    | GUnforgeable.gDeployId _ => 0
    | GUnforgeable.gDeployerId => 0
    | GUnforgeable.gSysAuthToken => 0

  /-- `Connective` is transparent for the same reason `Expr` is. -/
  def connectiveNestDepth : Connective → Nat
    | Connective.connAnd ps => listNestPar ps
    | Connective.connOr ps => listNestPar ps
    | Connective.connNot p => parNestDepth p
    | Connective.connVarRef _ _ => 0
  termination_by c => sizeOf c

  def listNestPar : List Par → Nat
    | [] => 0
    | a :: as => max (parNestDepth a) (listNestPar as)
  termination_by l => sizeOf l

  def listNestPair : List (Par × Par) → Nat
    | [] => 0
    | (a, b) :: as => max (parNestDepth a) (max (parNestDepth b) (listNestPair as))
  termination_by l => sizeOf l

  def listNestSend : List Send → Nat
    | [] => 0
    | a :: as => max (sendNestDepth a) (listNestSend as)
  termination_by l => sizeOf l

  def listNestReceive : List Receive → Nat
    | [] => 0
    | a :: as => max (receiveNestDepth a) (listNestReceive as)
  termination_by l => sizeOf l

  def listNestReceiveBind : List ReceiveBind → Nat
    | [] => 0
    | a :: as => max (receiveBindNestDepth a) (listNestReceiveBind as)
  termination_by l => sizeOf l

  def listNestNew : List New → Nat
    | [] => 0
    | a :: as => max (newNestDepth a) (listNestNew as)
  termination_by l => sizeOf l

  def listNestMatch : List Match → Nat
    | [] => 0
    | a :: as => max (matchNestDepth a) (listNestMatch as)
  termination_by l => sizeOf l

  def listNestMatchCase : List MatchCase → Nat
    | [] => 0
    | a :: as => max (matchCaseNestDepth a) (listNestMatchCase as)
  termination_by l => sizeOf l

  def listNestExpr : List Expr → Nat
    | [] => 0
    | a :: as => max (exprNestDepth a) (listNestExpr as)
  termination_by l => sizeOf l

  def listNestBundle : List Bundle → Nat
    | [] => 0
    | a :: as => max (bundleNestDepth a) (listNestBundle as)
  termination_by l => sizeOf l

  def listNestGUnforgeable : List GUnforgeable → Nat
    | [] => 0
    | a :: as => max (gUnforgeableNestDepth a) (listNestGUnforgeable as)
  termination_by l => sizeOf l

  def listNestConnective : List Connective → Nat
    | [] => 0
    | a :: as => max (connectiveNestDepth a) (listNestConnective as)
  termination_by l => sizeOf l
end

/-! ## The value walk, as its own recursion

The same shape as `Rchain/Depth.lean`'s walk — term first, budget a parameter, the budget test in the
body — with the one difference that makes it the *value* route's walk: only `walkValuePar` tests the
budget. Every element member passes it straight through, so an element costs no level, which is what
`push_value_fields` does when it pushes its fields at the depth it was handed.
-/
set_option maxHeartbeats 4000000 in
mutual
  def walkValuePar : Par → Nat → Bool
    | Par.mk s r nw e m u b c, k =>
        (match k with
         | 0 => true
         | j + 1 =>
             walkValueListSend s j || walkValueListReceive r j || walkValueListNew nw j ||
             walkValueListExpr e j || walkValueListMatch m j || walkValueListGUnforgeable u j ||
             walkValueListBundle b j || walkValueListConnective c j)
  termination_by p _ => sizeOf p

  def walkValueSend : Send → Nat → Bool
    | Send.mk c d _, k => walkValuePar c k || walkValueListPar d k
  termination_by s _ => sizeOf s

  def walkValueReceiveBind : ReceiveBind → Nat → Bool
    | ReceiveBind.mk ps s _, k => walkValueListPar ps k || walkValuePar s k
  termination_by b _ => sizeOf b

  def walkValueReceive : Receive → Nat → Bool
    | Receive.mk bs b _ _, k => walkValueListReceiveBind bs k || walkValuePar b k
  termination_by r _ => sizeOf r

  def walkValueNew : New → Nat → Bool
    | New.mk _ b, k => walkValuePar b k
  termination_by n _ => sizeOf n

  def walkValueMatchCase : MatchCase → Nat → Bool
    | MatchCase.mk p s _, k => walkValuePar p k || walkValuePar s k
  termination_by m _ => sizeOf m

  def walkValueMatch : Match → Nat → Bool
    | Match.mk t cs, k => walkValuePar t k || walkValueListMatchCase cs k
  termination_by m _ => sizeOf m

  def walkValueExpr : Expr → Nat → Bool
    | Expr.ground _, _ | Expr.evar _, _ | Expr.ebigint _, _ => false
    | Expr.eneg p, k | Expr.enot p, k => walkValuePar p k
    | Expr.eplus p q, k | Expr.eminus p q, k | Expr.emult p q, k | Expr.ediv p q, k
    | Expr.emod p q, k | Expr.elt p q, k | Expr.ele p q, k | Expr.egt p q, k
    | Expr.ege p q, k | Expr.eeq p q, k | Expr.eneq p q, k | Expr.eand p q, k
    | Expr.eor p q, k | Expr.ematches p q, k | Expr.eshortand p q, k
    | Expr.eshortor p q, k => walkValuePar p k || walkValuePar q k
    | Expr.elist ps _, k | Expr.etuple ps, k | Expr.eset ps _, k => walkValueListPar ps k
    | Expr.emap kvs _, k => walkValueListPair kvs k
    | Expr.emethod _ p args, k => walkValuePar p k || walkValueListPar args k
    | Expr.epercentPercent p q, k | Expr.eplusPlus p q, k | Expr.eminusMinus p q, k =>
        walkValuePar p k || walkValuePar q k
  termination_by e _ => sizeOf e

  def walkValueBundle : Bundle → Nat → Bool
    | Bundle.mk p _ _, k => walkValuePar p k
  termination_by b _ => sizeOf b

  def walkValueGUnforgeable : GUnforgeable → Nat → Bool
    | _, _ => false

  def walkValueConnective : Connective → Nat → Bool
    | Connective.connAnd ps, k | Connective.connOr ps, k => walkValueListPar ps k
    | Connective.connNot p, k => walkValuePar p k
    | Connective.connVarRef _ _, _ => false
  termination_by c _ => sizeOf c

  def walkValueListPar : List Par → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValuePar a k || walkValueListPar as k
  termination_by l _ => sizeOf l

  def walkValueListPair : List (Par × Par) → Nat → Bool
    | [], _ => false
    | (a, b) :: as, k => walkValuePar a k || walkValuePar b k || walkValueListPair as k
  termination_by l _ => sizeOf l

  def walkValueListSend : List Send → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueSend a k || walkValueListSend as k
  termination_by l _ => sizeOf l

  def walkValueListReceive : List Receive → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueReceive a k || walkValueListReceive as k
  termination_by l _ => sizeOf l

  def walkValueListReceiveBind : List ReceiveBind → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueReceiveBind a k || walkValueListReceiveBind as k
  termination_by l _ => sizeOf l

  def walkValueListNew : List New → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueNew a k || walkValueListNew as k
  termination_by l _ => sizeOf l

  def walkValueListMatch : List Match → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueMatch a k || walkValueListMatch as k
  termination_by l _ => sizeOf l

  def walkValueListMatchCase : List MatchCase → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueMatchCase a k || walkValueListMatchCase as k
  termination_by l _ => sizeOf l

  def walkValueListExpr : List Expr → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueExpr a k || walkValueListExpr as k
  termination_by l _ => sizeOf l

  def walkValueListBundle : List Bundle → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueBundle a k || walkValueListBundle as k
  termination_by l _ => sizeOf l

  def walkValueListGUnforgeable : List GUnforgeable → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueGUnforgeable a k || walkValueListGUnforgeable as k
  termination_by l _ => sizeOf l

  def walkValueListConnective : List Connective → Nat → Bool
    | [], _ => false
    | a :: as, k => walkValueConnective a k || walkValueListConnective as k
  termination_by l _ => sizeOf l
end

/-! ## The agreement

One `mutual` block, mirroring `Rchain/Depth.lean`'s and, behind it, `Rchain/FreeVars.lean`'s: each
member is the walk's own recursion and each recursive call is discharged by `simp` with the *sibling
theorem's statement*. No `termination_by` and no `decreasing_by`.

**Only `walkValuePar` splits on the budget**, because only it tests it. Every element member's equation
holds for an arbitrary `k` — that is what "an element costs no level" means, and it is why this block's
element arms need no `cases k` where clause a's did.

**The zero-budget arm is not an exception.** `walkValuePar p 0 = true` while `parNestDepth p ≥ 1`, so
both sides of the `↔` are false and it closes by `simp` — unlike clause a, whose zero arm needed the
same shape but with a `1 +` on the depth.
-/
set_option maxHeartbeats 4000000 in
mutual
  theorem walkValuePar_iff_parNestDepth :
      (p : Par) → ∀ k, walkValuePar p k = false ↔ parNestDepth p ≤ k
    | Par.mk s r nw e m u b c, k => by
      cases k with
      | zero => simp [walkValuePar, parNestDepth]
      | succ j =>
        simp only [walkValuePar, parNestDepth, Bool.or_eq_false_iff, and_assoc,
          walkValueListSend_iff_listNestSend, walkValueListReceive_iff_listNestReceive,
          walkValueListNew_iff_listNestNew, walkValueListExpr_iff_listNestExpr,
          walkValueListMatch_iff_listNestMatch,
          walkValueListGUnforgeable_iff_listNestGUnforgeable,
          walkValueListBundle_iff_listNestBundle, walkValueListConnective_iff_listNestConnective]
        omega

  theorem walkValueSend_iff_sendNestDepth :
      (s : Send) → ∀ k, walkValueSend s k = false ↔ sendNestDepth s ≤ k
    | Send.mk c d _, k => by
      simp only [walkValueSend, sendNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth, walkValueListPar_iff_listNestPar]
      omega

  theorem walkValueReceiveBind_iff_receiveBindNestDepth :
      (b : ReceiveBind) → ∀ k, walkValueReceiveBind b k = false ↔ receiveBindNestDepth b ≤ k
    | ReceiveBind.mk ps s _, k => by
      simp only [walkValueReceiveBind, receiveBindNestDepth, Bool.or_eq_false_iff,
        walkValueListPar_iff_listNestPar, walkValuePar_iff_parNestDepth]
      omega

  theorem walkValueReceive_iff_receiveNestDepth :
      (r : Receive) → ∀ k, walkValueReceive r k = false ↔ receiveNestDepth r ≤ k
    | Receive.mk bs b _ _, k => by
      simp only [walkValueReceive, receiveNestDepth, Bool.or_eq_false_iff,
        walkValueListReceiveBind_iff_listNestReceiveBind, walkValuePar_iff_parNestDepth]
      omega

  theorem walkValueNew_iff_newNestDepth :
      (nw : New) → ∀ k, walkValueNew nw k = false ↔ newNestDepth nw ≤ k
    | New.mk _ b, k => by
      simp only [walkValueNew, newNestDepth, walkValuePar_iff_parNestDepth]

  theorem walkValueMatchCase_iff_matchCaseNestDepth :
      (m : MatchCase) → ∀ k, walkValueMatchCase m k = false ↔ matchCaseNestDepth m ≤ k
    | MatchCase.mk p s _, k => by
      simp only [walkValueMatchCase, matchCaseNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth]
      omega

  theorem walkValueMatch_iff_matchNestDepth :
      (m : Match) → ∀ k, walkValueMatch m k = false ↔ matchNestDepth m ≤ k
    | Match.mk t cs, k => by
      simp only [walkValueMatch, matchNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth, walkValueListMatchCase_iff_listNestMatchCase]
      omega

  theorem walkValueExpr_iff_exprNestDepth :
      (e : Expr) → ∀ k, walkValueExpr e k = false ↔ exprNestDepth e ≤ k
    | Expr.ground _, k | Expr.evar _, k | Expr.ebigint _, k => by
      simp [walkValueExpr, exprNestDepth]
    | Expr.eneg p, k | Expr.enot p, k => by
      simp only [walkValueExpr, exprNestDepth, walkValuePar_iff_parNestDepth]
    | Expr.eplus p q, k | Expr.eminus p q, k | Expr.emult p q, k | Expr.ediv p q, k
    | Expr.emod p q, k | Expr.elt p q, k | Expr.ele p q, k | Expr.egt p q, k
    | Expr.ege p q, k | Expr.eeq p q, k | Expr.eneq p q, k | Expr.eand p q, k
    | Expr.eor p q, k | Expr.ematches p q, k | Expr.eshortand p q, k
    | Expr.eshortor p q, k => by
      simp only [walkValueExpr, exprNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth]
      omega
    | Expr.elist ps _, k | Expr.etuple ps, k | Expr.eset ps _, k => by
      simp only [walkValueExpr, exprNestDepth, walkValueListPar_iff_listNestPar]
    | Expr.emap kvs _, k => by
      simp only [walkValueExpr, exprNestDepth, walkValueListPair_iff_listNestPair]
    | Expr.emethod _ p args, k => by
      simp only [walkValueExpr, exprNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth, walkValueListPar_iff_listNestPar]
      omega
    | Expr.epercentPercent p q, k | Expr.eplusPlus p q, k | Expr.eminusMinus p q, k => by
      simp only [walkValueExpr, exprNestDepth, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth]
      omega

  theorem walkValueBundle_iff_bundleNestDepth :
      (b : Bundle) → ∀ k, walkValueBundle b k = false ↔ bundleNestDepth b ≤ k
    | Bundle.mk p _ _, k => by
      simp only [walkValueBundle, bundleNestDepth, walkValuePar_iff_parNestDepth]

  theorem walkValueGUnforgeable_iff_gUnforgeableNestDepth :
      (u : GUnforgeable) → ∀ k, walkValueGUnforgeable u k = false ↔ gUnforgeableNestDepth u ≤ k
    | GUnforgeable.gPrivate _, k => by simp [walkValueGUnforgeable, gUnforgeableNestDepth]
    | GUnforgeable.gDeployId _, k => by simp [walkValueGUnforgeable, gUnforgeableNestDepth]
    | GUnforgeable.gDeployerId, k => by simp [walkValueGUnforgeable, gUnforgeableNestDepth]
    | GUnforgeable.gSysAuthToken, k => by simp [walkValueGUnforgeable, gUnforgeableNestDepth]

  theorem walkValueConnective_iff_connectiveNestDepth :
      (c : Connective) → ∀ k, walkValueConnective c k = false ↔ connectiveNestDepth c ≤ k
    | Connective.connAnd ps, k | Connective.connOr ps, k => by
      simp only [walkValueConnective, connectiveNestDepth, walkValueListPar_iff_listNestPar]
    | Connective.connNot p, k => by
      simp only [walkValueConnective, connectiveNestDepth, walkValuePar_iff_parNestDepth]
    | Connective.connVarRef _ _, k => by
      simp [walkValueConnective, connectiveNestDepth]

  theorem walkValueListPar_iff_listNestPar :
      (l : List Par) → ∀ k, walkValueListPar l k = false ↔ listNestPar l ≤ k
    | [], k => by simp [walkValueListPar, listNestPar]
    | a :: as, k => by
      simp only [walkValueListPar, listNestPar, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth, walkValueListPar_iff_listNestPar]
      omega

  theorem walkValueListPair_iff_listNestPair :
      (l : List (Par × Par)) → ∀ k, walkValueListPair l k = false ↔ listNestPair l ≤ k
    | [], k => by simp [walkValueListPair, listNestPair]
    | (a, b) :: as, k => by
      simp only [walkValueListPair, listNestPair, Bool.or_eq_false_iff,
        walkValuePar_iff_parNestDepth, walkValueListPair_iff_listNestPair]
      omega

  theorem walkValueListSend_iff_listNestSend :
      (l : List Send) → ∀ k, walkValueListSend l k = false ↔ listNestSend l ≤ k
    | [], k => by simp [walkValueListSend, listNestSend]
    | a :: as, k => by
      simp only [walkValueListSend, listNestSend, Bool.or_eq_false_iff,
        walkValueSend_iff_sendNestDepth, walkValueListSend_iff_listNestSend]
      omega

  theorem walkValueListReceive_iff_listNestReceive :
      (l : List Receive) → ∀ k, walkValueListReceive l k = false ↔ listNestReceive l ≤ k
    | [], k => by simp [walkValueListReceive, listNestReceive]
    | a :: as, k => by
      simp only [walkValueListReceive, listNestReceive, Bool.or_eq_false_iff,
        walkValueReceive_iff_receiveNestDepth, walkValueListReceive_iff_listNestReceive]
      omega

  theorem walkValueListReceiveBind_iff_listNestReceiveBind :
      (l : List ReceiveBind) → ∀ k, walkValueListReceiveBind l k = false ↔ listNestReceiveBind l ≤ k
    | [], k => by simp [walkValueListReceiveBind, listNestReceiveBind]
    | a :: as, k => by
      simp only [walkValueListReceiveBind, listNestReceiveBind, Bool.or_eq_false_iff,
        walkValueReceiveBind_iff_receiveBindNestDepth,
        walkValueListReceiveBind_iff_listNestReceiveBind]
      omega

  theorem walkValueListNew_iff_listNestNew :
      (l : List New) → ∀ k, walkValueListNew l k = false ↔ listNestNew l ≤ k
    | [], k => by simp [walkValueListNew, listNestNew]
    | a :: as, k => by
      simp only [walkValueListNew, listNestNew, Bool.or_eq_false_iff,
        walkValueNew_iff_newNestDepth, walkValueListNew_iff_listNestNew]
      omega

  theorem walkValueListMatch_iff_listNestMatch :
      (l : List Match) → ∀ k, walkValueListMatch l k = false ↔ listNestMatch l ≤ k
    | [], k => by simp [walkValueListMatch, listNestMatch]
    | a :: as, k => by
      simp only [walkValueListMatch, listNestMatch, Bool.or_eq_false_iff,
        walkValueMatch_iff_matchNestDepth, walkValueListMatch_iff_listNestMatch]
      omega

  theorem walkValueListMatchCase_iff_listNestMatchCase :
      (l : List MatchCase) → ∀ k, walkValueListMatchCase l k = false ↔ listNestMatchCase l ≤ k
    | [], k => by simp [walkValueListMatchCase, listNestMatchCase]
    | a :: as, k => by
      simp only [walkValueListMatchCase, listNestMatchCase, Bool.or_eq_false_iff,
        walkValueMatchCase_iff_matchCaseNestDepth,
        walkValueListMatchCase_iff_listNestMatchCase]
      omega

  theorem walkValueListExpr_iff_listNestExpr :
      (l : List Expr) → ∀ k, walkValueListExpr l k = false ↔ listNestExpr l ≤ k
    | [], k => by simp [walkValueListExpr, listNestExpr]
    | a :: as, k => by
      simp only [walkValueListExpr, listNestExpr, Bool.or_eq_false_iff,
        walkValueExpr_iff_exprNestDepth, walkValueListExpr_iff_listNestExpr]
      omega

  theorem walkValueListBundle_iff_listNestBundle :
      (l : List Bundle) → ∀ k, walkValueListBundle l k = false ↔ listNestBundle l ≤ k
    | [], k => by simp [walkValueListBundle, listNestBundle]
    | a :: as, k => by
      simp only [walkValueListBundle, listNestBundle, Bool.or_eq_false_iff,
        walkValueBundle_iff_bundleNestDepth, walkValueListBundle_iff_listNestBundle]
      omega

  theorem walkValueListGUnforgeable_iff_listNestGUnforgeable :
      (l : List GUnforgeable) → ∀ k, walkValueListGUnforgeable l k = false ↔ listNestGUnforgeable l ≤ k
    | [], k => by simp [walkValueListGUnforgeable, listNestGUnforgeable]
    | a :: as, k => by
      simp only [walkValueListGUnforgeable, listNestGUnforgeable, Bool.or_eq_false_iff,
        walkValueGUnforgeable_iff_gUnforgeableNestDepth,
        walkValueListGUnforgeable_iff_listNestGUnforgeable]
      omega

  theorem walkValueListConnective_iff_listNestConnective :
      (l : List Connective) → ∀ k, walkValueListConnective l k = false ↔ listNestConnective l ≤ k
    | [], k => by simp [walkValueListConnective, listNestConnective]
    | a :: as, k => by
      simp only [walkValueListConnective, listNestConnective, Bool.or_eq_false_iff,
        walkValueConnective_iff_connectiveNestDepth,
        walkValueListConnective_iff_listNestConnective]
      omega
end

/-! ## The bridge — what the value bound says about `parDepth`

`parNestDepth` is the quantity the *space* enforces; clause b's statement is about `parDepth`. The two
differ by a factor of at most **three**, and that is the honest form of what the note used to call a
slack of `MAX_PARSE_DEPTH` (128) — which was never a slack at all, since it is a constant where the gap
is a multiple.

**Three, and it is tight.** Along a path, `Par` and element nodes are *almost* alternating — but two
element nodes can sit between consecutive `Par`s, because a `Receive` holds `ReceiveBind`s and a `Match`
holds `MatchCase`s. So a ladder `Par → Match → MatchCase → Par → …` spends three levels per `Par`, and
at `n` rungs has `parDepth = 3n + 1` against `parNestDepth = n + 1`: the ratio tends to three and the
`s` bound fails from `n = 2` (`parDepth 7` against `2 × 3`). The tilt is exactly what the factor is for.

The element members therefore carry different offsets, and the offsets are *not* free: an element's
offset is `1 +` the largest offset among its children, a `Par` child contributes `0`, and a list inherits
its element's. That fixes the two families — `Send`/`ReceiveBind`/`New`/`MatchCase`/`Bundle`/`Expr`/
`Connective`/`GUnforgeable` at `+ 1` (`Receive`, `Match`) at `+ 2` — and it is why a uniform `+ 1` cannot
work: `Receive`'s children include the `ReceiveBind` list, so its offset is one more than theirs.
-/
set_option maxHeartbeats 4000000 in
mutual
  theorem parDepth_le_three_mul_parNestDepth : (p : Par) → parDepth p ≤ 3 * parNestDepth p
    | Par.mk s r nw e m u b c => by
      have h1 := listDepthSend_le_three_mul_listNestSend s
      have h2 := listDepthReceive_le_three_mul_listNestReceive r
      have h3 := listDepthNew_le_three_mul_listNestNew nw
      have h4 := listDepthExpr_le_three_mul_listNestExpr e
      have h5 := listDepthMatch_le_three_mul_listNestMatch m
      have h6 := listDepthGUnforgeable_le_three_mul_listNestGUnforgeable u
      have h7 := listDepthBundle_le_three_mul_listNestBundle b
      have h8 := listDepthConnective_le_three_mul_listNestConnective c
      simp only [parDepth, parNestDepth]
      omega

  theorem sendDepth_le_three_mul_sendNestDepth :
      (s : Send) → sendDepth s ≤ 3 * sendNestDepth s + 1
    | Send.mk c d _ => by
      have h1 := parDepth_le_three_mul_parNestDepth c
      have h2 := listDepthPar_le_three_mul_listNestPar d
      simp only [sendDepth, sendNestDepth]
      omega

  theorem receiveBindDepth_le_three_mul_receiveBindNestDepth :
      (b : ReceiveBind) → receiveBindDepth b ≤ 3 * receiveBindNestDepth b + 1
    | ReceiveBind.mk ps s _ => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      have h2 := parDepth_le_three_mul_parNestDepth s
      simp only [receiveBindDepth, receiveBindNestDepth]
      omega

  theorem receiveDepth_le_three_mul_receiveNestDepth :
      (r : Receive) → receiveDepth r ≤ 3 * receiveNestDepth r + 2
    | Receive.mk bs b _ _ => by
      have h1 := listDepthReceiveBind_le_three_mul_listNestReceiveBind bs
      have h2 := parDepth_le_three_mul_parNestDepth b
      simp only [receiveDepth, receiveNestDepth]
      omega

  theorem newDepth_le_three_mul_newNestDepth :
      (nw : New) → newDepth nw ≤ 3 * newNestDepth nw + 1
    | New.mk _ b => by
      have h1 := parDepth_le_three_mul_parNestDepth b
      simp only [newDepth, newNestDepth]
      omega

  theorem matchCaseDepth_le_three_mul_matchCaseNestDepth :
      (m : MatchCase) → matchCaseDepth m ≤ 3 * matchCaseNestDepth m + 1
    | MatchCase.mk p s _ => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth s
      simp only [matchCaseDepth, matchCaseNestDepth]
      omega

  theorem matchDepth_le_three_mul_matchNestDepth :
      (m : Match) → matchDepth m ≤ 3 * matchNestDepth m + 2
    | Match.mk t cs => by
      have h1 := parDepth_le_three_mul_parNestDepth t
      have h2 := listDepthMatchCase_le_three_mul_listNestMatchCase cs
      simp only [matchDepth, matchNestDepth]
      omega

  theorem exprDepth_le_three_mul_exprNestDepth :
      (e : Expr) → exprDepth e ≤ 3 * exprNestDepth e + 1
    | Expr.ground _ => by simp [exprDepth, exprNestDepth]
    | Expr.evar _ => by simp [exprDepth, exprNestDepth]
    | Expr.ebigint _ => by simp [exprDepth, exprNestDepth]
    | Expr.eneg p => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.enot p => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eplus p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eminus p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.emult p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.ediv p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.emod p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.elt p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.ele p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.egt p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.ege p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eeq p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eneq p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eand p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eor p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.ematches p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eshortand p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eshortor p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.elist ps _ => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.etuple ps => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eset ps _ => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.emap kvs _ => by
      have h1 := listDepthPair_le_three_mul_listNestPair kvs
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.emethod _ p args => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := listDepthPar_le_three_mul_listNestPar args
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.epercentPercent p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eplusPlus p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega
    | Expr.eminusMinus p q => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      have h2 := parDepth_le_three_mul_parNestDepth q
      simp only [exprDepth, exprNestDepth]; omega

  theorem bundleDepth_le_three_mul_bundleNestDepth :
      (b : Bundle) → bundleDepth b ≤ 3 * bundleNestDepth b + 1
    | Bundle.mk p _ _ => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      simp only [bundleDepth, bundleNestDepth]; omega

  theorem gUnforgeableDepth_le_three_mul_gUnforgeableNestDepth :
      (u : GUnforgeable) → gUnforgeableDepth u ≤ 3 * gUnforgeableNestDepth u + 1
    | GUnforgeable.gPrivate _ => by simp [gUnforgeableDepth, gUnforgeableNestDepth]
    | GUnforgeable.gDeployId _ => by simp [gUnforgeableDepth, gUnforgeableNestDepth]
    | GUnforgeable.gDeployerId => by simp [gUnforgeableDepth, gUnforgeableNestDepth]
    | GUnforgeable.gSysAuthToken => by simp [gUnforgeableDepth, gUnforgeableNestDepth]

  theorem connectiveDepth_le_three_mul_connectiveNestDepth :
      (c : Connective) → connectiveDepth c ≤ 3 * connectiveNestDepth c + 1
    | Connective.connAnd ps => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      simp only [connectiveDepth, connectiveNestDepth]; omega
    | Connective.connOr ps => by
      have h1 := listDepthPar_le_three_mul_listNestPar ps
      simp only [connectiveDepth, connectiveNestDepth]; omega
    | Connective.connNot p => by
      have h1 := parDepth_le_three_mul_parNestDepth p
      simp only [connectiveDepth, connectiveNestDepth]; omega
    | Connective.connVarRef _ _ => by simp [connectiveDepth, connectiveNestDepth]

  theorem listDepthPar_le_three_mul_listNestPar :
      (l : List Par) → listDepthPar l ≤ 3 * listNestPar l
    | [] => by simp [listDepthPar, listNestPar]
    | a :: as => by
      have h1 := parDepth_le_three_mul_parNestDepth a
      have h2 := listDepthPar_le_three_mul_listNestPar as
      simp only [listDepthPar, listNestPar]; omega

  theorem listDepthPair_le_three_mul_listNestPair :
      (l : List (Par × Par)) → listDepthPair l ≤ 3 * listNestPair l
    | [] => by simp [listDepthPair, listNestPair]
    | (a, b) :: as => by
      have h1 := parDepth_le_three_mul_parNestDepth a
      have h2 := parDepth_le_three_mul_parNestDepth b
      have h3 := listDepthPair_le_three_mul_listNestPair as
      simp only [listDepthPair, listNestPair]; omega

  theorem listDepthSend_le_three_mul_listNestSend :
      (l : List Send) → listDepthSend l ≤ 3 * listNestSend l + 1
    | [] => by simp [listDepthSend, listNestSend]
    | a :: as => by
      have h1 := sendDepth_le_three_mul_sendNestDepth a
      have h2 := listDepthSend_le_three_mul_listNestSend as
      simp only [listDepthSend, listNestSend]; omega

  theorem listDepthReceive_le_three_mul_listNestReceive :
      (l : List Receive) → listDepthReceive l ≤ 3 * listNestReceive l + 2
    | [] => by simp [listDepthReceive, listNestReceive]
    | a :: as => by
      have h1 := receiveDepth_le_three_mul_receiveNestDepth a
      have h2 := listDepthReceive_le_three_mul_listNestReceive as
      simp only [listDepthReceive, listNestReceive]; omega

  theorem listDepthReceiveBind_le_three_mul_listNestReceiveBind :
      (l : List ReceiveBind) → listDepthReceiveBind l ≤ 3 * listNestReceiveBind l + 1
    | [] => by simp [listDepthReceiveBind, listNestReceiveBind]
    | a :: as => by
      have h1 := receiveBindDepth_le_three_mul_receiveBindNestDepth a
      have h2 := listDepthReceiveBind_le_three_mul_listNestReceiveBind as
      simp only [listDepthReceiveBind, listNestReceiveBind]; omega

  theorem listDepthNew_le_three_mul_listNestNew :
      (l : List New) → listDepthNew l ≤ 3 * listNestNew l + 1
    | [] => by simp [listDepthNew, listNestNew]
    | a :: as => by
      have h1 := newDepth_le_three_mul_newNestDepth a
      have h2 := listDepthNew_le_three_mul_listNestNew as
      simp only [listDepthNew, listNestNew]; omega

  theorem listDepthMatch_le_three_mul_listNestMatch :
      (l : List Match) → listDepthMatch l ≤ 3 * listNestMatch l + 2
    | [] => by simp [listDepthMatch, listNestMatch]
    | a :: as => by
      have h1 := matchDepth_le_three_mul_matchNestDepth a
      have h2 := listDepthMatch_le_three_mul_listNestMatch as
      simp only [listDepthMatch, listNestMatch]; omega

  theorem listDepthMatchCase_le_three_mul_listNestMatchCase :
      (l : List MatchCase) → listDepthMatchCase l ≤ 3 * listNestMatchCase l + 1
    | [] => by simp [listDepthMatchCase, listNestMatchCase]
    | a :: as => by
      have h1 := matchCaseDepth_le_three_mul_matchCaseNestDepth a
      have h2 := listDepthMatchCase_le_three_mul_listNestMatchCase as
      simp only [listDepthMatchCase, listNestMatchCase]; omega

  theorem listDepthExpr_le_three_mul_listNestExpr :
      (l : List Expr) → listDepthExpr l ≤ 3 * listNestExpr l + 1
    | [] => by simp [listDepthExpr, listNestExpr]
    | a :: as => by
      have h1 := exprDepth_le_three_mul_exprNestDepth a
      have h2 := listDepthExpr_le_three_mul_listNestExpr as
      simp only [listDepthExpr, listNestExpr]; omega

  theorem listDepthBundle_le_three_mul_listNestBundle :
      (l : List Bundle) → listDepthBundle l ≤ 3 * listNestBundle l + 1
    | [] => by simp [listDepthBundle, listNestBundle]
    | a :: as => by
      have h1 := bundleDepth_le_three_mul_bundleNestDepth a
      have h2 := listDepthBundle_le_three_mul_listNestBundle as
      simp only [listDepthBundle, listNestBundle]; omega

  theorem listDepthGUnforgeable_le_three_mul_listNestGUnforgeable :
      (l : List GUnforgeable) → listDepthGUnforgeable l ≤ 3 * listNestGUnforgeable l + 1
    | [] => by simp [listDepthGUnforgeable, listNestGUnforgeable]
    | a :: as => by
      have h1 := gUnforgeableDepth_le_three_mul_gUnforgeableNestDepth a
      have h2 := listDepthGUnforgeable_le_three_mul_listNestGUnforgeable as
      simp only [listDepthGUnforgeable, listNestGUnforgeable]; omega

  theorem listDepthConnective_le_three_mul_listNestConnective :
      (l : List Connective) → listDepthConnective l ≤ 3 * listNestConnective l + 1
    | [] => by simp [listDepthConnective, listNestConnective]
    | a :: as => by
      have h1 := connectiveDepth_le_three_mul_connectiveNestDepth a
      have h2 := listDepthConnective_le_three_mul_listNestConnective as
      simp only [listDepthConnective, listNestConnective]; omega
end

/-! ## The guard, and the falsifier

`exceeds_value_depth` is the Rust walk this models. `valueWalkExceeds` is it over the model, and the two
agree for every `limit ≥ 1` — the caveat is the root: the Rust never pushes it (its *fields* go on the
worklist at depth 2), so at `limit = 0` it accepts a value with no `Par` child while the model refuses
it. `maxValueDepth` is 256, so the caveat is not reachable from `check_value_depth`.
-/
/-- The space's guard over the model — `models/src/types.rs::exceeds_value_depth`'s walk, and the
caller's constant is `rholang/src/storage.rs::MAX_VALUE_DEPTH`. -/
def valueWalkExceeds (limit : Nat) (p : Par) : Bool := walkValuePar p limit

/-- **Soundness**: a value the space accepts has `Par`-nesting no deeper than the bound. -/
theorem valueWalkExceeds_sound (limit : Nat) (p : Par) :
    valueWalkExceeds limit p = false → parNestDepth p ≤ limit :=
  (walkValuePar_iff_parNestDepth p limit).mp

/-- **Completeness**: the guard refuses exactly what is too deeply nested and nothing else. -/
theorem valueWalkExceeds_complete (limit : Nat) (p : Par) :
    parNestDepth p ≤ limit → valueWalkExceeds limit p = false :=
  (walkValuePar_iff_parNestDepth p limit).mpr

/-- **What the space's bound says about `parDepth`**, which is what clause a's quantity is: a value the
space accepts has `parDepth` at most three times the bound. At `maxValueDepth` that is 768 — the same
number as `maxAstDepth`, which is what makes the two clauses' constants a pair rather than two
unrelated measurements. -/
theorem valueWalkExceeds_sound_parDepth (limit : Nat) (p : Par) :
    valueWalkExceeds limit p = false → parDepth p ≤ 3 * limit := by
  intro h
  have h1 := valueWalkExceeds_sound limit p h
  have h2 := parDepth_le_three_mul_parNestDepth p
  omega

/-! ## The witness shape, and the falsifier

`pairsDepth` is the shape a folding contract builds and an attacker builds: nested pairs, one `ETuple`
level per rung. Its `Par`-nesting is `n + 1` and its `parDepth` is `2n + 1`, so at `maxValueDepth` the
guard admits `pairsDepth 255` — `parDepth` 511 — and refuses `pairsDepth 256`, whose `parDepth` is 513.
-/

/-- `pairsDepth`'s `Par`-nesting: one `Par` per rung, plus the base. -/
theorem parNestDepth_pairsDepth (n : Nat) : parNestDepth (pairsDepth n) = n + 1 := by
  induction n with
  | zero => simp [pairsDepth, parNestDepth, listNestSend, listNestReceive, listNestNew,
      listNestExpr, listNestMatch, listNestGUnforgeable, listNestBundle, listNestConnective]
  | succ k ih =>
      simp only [pairsDepth, parNestDepth, exprNestDepth, listNestExpr, listNestPar, listNestSend,
        listNestReceive, listNestNew, listNestMatch, listNestGUnforgeable, listNestBundle,
        listNestConnective, ih]
      omega

/-- `pairsDepth`'s `parDepth`, so the factor between the two quantities is a theorem here rather than a
claim in a comment. -/
theorem parDepth_pairsDepth (n : Nat) : parDepth (pairsDepth n) = 2 * n + 1 := by
  induction n with
  | zero => simp [pairsDepth, parDepth, listDepthSend, listDepthReceive, listDepthNew,
      listDepthExpr, listDepthMatch, listDepthGUnforgeable, listDepthBundle, listDepthConnective]
  | succ k ih =>
      simp only [pairsDepth, parDepth, exprDepth, listDepthExpr, listDepthPar, listDepthSend,
        listDepthReceive, listDepthNew, listDepthMatch, listDepthGUnforgeable, listDepthBundle,
        listDepthConnective, ih]
      omega

/-- **The value walk with its `exprs` arm dropped** — the mutation clause b's falsifier is about, as a
definition rather than a described edit. The other seven children arms are the real value walk's own
list functions: the witness below has an empty `Send`/`Receive`/`New`/`Match`/`GUnforgeable`/`Bundle`/
`Connective` field, so its value does not depend on that delegation — what it depends on is that the
`e` field is never visited, which is exactly the arm deleted. -/
def walkValueParDroppingExpr : Par → Nat → Bool
  | Par.mk s r nw _ m u b c, k =>
      (match k with
       | 0 => true
       | j + 1 =>
           walkValueListSend s j || walkValueListReceive r j || walkValueListNew nw j ||
           walkValueListMatch m j || walkValueListGUnforgeable u j || walkValueListBundle b j ||
           walkValueListConnective c j)

/-- The mutant accepts `pairsDepth 256` at the bound — the nesting it is blind to is the whole of the
witness. -/
theorem value_mutant_accepts_the_witness :
    walkValueParDroppingExpr (pairsDepth 256) 256 = false := by
  change walkValueParDroppingExpr
    (Par.mk [] [] [] [Expr.etuple [pairsDepth 255]] [] [] [] []) 256 = false
  simp [walkValueParDroppingExpr, walkValueListSend, walkValueListReceive, walkValueListNew,
    walkValueListMatch, walkValueListGUnforgeable, walkValueListBundle, walkValueListConnective]

/-- **The falsifier.** The mutant accepts a value past the bound, so its soundness is false there —
and the real walk refuses the same value (`the_value_walk_refuses_the_mutant_witness`). So the
agreement above is a check of the walk rather than a restatement of a definition. -/
theorem a_dropped_value_arm_breaks_soundness :
    walkValueParDroppingExpr (pairsDepth 256) 256 = false ∧
      ¬ (parNestDepth (pairsDepth 256) ≤ 256) := by
  refine ⟨value_mutant_accepts_the_witness, ?_⟩
  rw [parNestDepth_pairsDepth]
  omega

/-- The contrast: the real walk refuses the very value the mutant admits, at the same bound. -/
theorem the_value_walk_refuses_the_mutant_witness :
    ¬ (walkValuePar (pairsDepth 256) 256 = false) := by
  intro h
  have hd := (walkValuePar_iff_parNestDepth (pairsDepth 256) 256).mp h
  rw [parNestDepth_pairsDepth] at hd
  omega

end Rchain
