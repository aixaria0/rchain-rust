import Rchain.Par


/-!
# Law 59 — wire fidelity: the CapTP value bridge is a section

The OCapN bridge translates Rholang values to Syrup and back (`ocapn/src/par_value.rs`). It was built
without a law, and the register's open rows are the edge cases that produced: a tuple that does not
come back, a URI that leaves and is refused on return, and — underneath both — no statement of what
the map *is*.

This module states it the way Law 42 states the JSON round trip: an encode, a decode, a domain
predicate, and three clauses.

- **59a — the round trip.** A `Par` the bridge decoded is a `Par` it encodes and decodes again. Over
  the values the decoder answers, so a shape outside the domain carries no claim.
- **59b — injectivity on the domain.** Two wireable Syrup values that decode to the same `Par` are
  the same wire form. This is the clause a *rendering rule* violates: under "an inbound list becomes
  a tuple", a peer's list and a peer's tuple would be two wire forms for one value.
- **59c — no lossy map.** The encoder never emits a shape outside the domain — §1.6's "no silent
  partiality" applied to a wire: a value with no counterpart is **refused**, never approximated by one
  that nearly fits. 59c is what makes the URI's "deliberate asymmetry" a violation rather than a
  preference: a URI encoded to a Symbol is a shape from which a URI does not come back.

**What a tuple crosses as, and why this shape.** A Syrup `record` is *labelled*, and a label must be
a selector, string or byte string (`@endo/ocapn`'s `decode.js`), so a Rholang tuple — unlabelled —
cannot be a bare record: `(true, 0)` would need the label `true`, which Endo refuses outright. Nor is
a record even passable in an argument: Endo's CapTP passable union is `{list, struct, tagged}` with no
record arm. The references have one extension point for exactly this — OCapN's **tagged** value,
`<desc:tagged :tagName value>` (`codecs/passable.js`'s `OcapnTaggedCodec`), whose `value` is *any*
passable. So a tuple crosses as `taggedRecord ss := <desc:tagged 'rho:tuple' [ss…]>`, which Endo
reads, holds, and writes back.

`Sy.tuple` is that shape **named**: a record of one specific label and arity, carried as its own
constructor so the round trip descends structurally rather than through a label test the equation
compiler cannot see (`taggedRecord` below is the same shape spelled as a record). `Sy.record` is then
what every *other* labelled record is — a peer's — and it has no Rholang counterpart, so it is
refused rather than read as a tuple.

**Two things the law puts outside the domain, named rather than implied.**

- a `Float64` — Syrup has one, Rholang has no float, so the bridge refuses it inbound and nothing
  claims a round trip for it;
- a **small** `GBigInt` (one that fits in an `i64`) — Syrup's integer is one type where Rholang has
  two, so `GBigInt 5` and `GInt 5` share a wire form. The domain keeps `GInt`, whose image is exact,
  and excludes the ambiguous `GBigInt`; a `GBigInt` outside the machine word is unambiguous and is
  kept. (Reachable: the normalizer converts a parse-level bigint ground into `Expr::GBigInt`, so `5n`
  is the excluded shape.)

`Sy` mirrors `ocapn/src/syrup.rs`'s `Value` except in the model's conventions for text (code points,
as `Rchain.Ground` has it), so the two sides still compare.
-/

namespace Rchain

/-- A string's code points — the model's spelling for text (`Rchain.Ground.str`). -/
def syChars (s : String) : List Nat := s.toList.map Char.toNat

/-- Code points as a string. -/
def syString (l : List Nat) : String := (l.map Char.ofNat).asString

/-- The label of the record a Rholang tuple crosses as: `desc:tagged`, the CapTP union's extension
point, read as a selector. -/
def taggedLabel : List Nat := syChars "desc:tagged"

/-- The tag that says the payload is a Rholang tuple. -/
def tupleTag : List Nat := syChars "rho:tuple"

/-- A Syrup value: `ocapn/src/syrup.rs`'s `Value`, with the model's conventions. The label of a record
is its first element, and a tuple is the tagged record below, named. -/
inductive Sy where
  | bool   : Bool → Sy
  | int    : Int → Sy
  | str    : List Nat → Sy
  | sym    : List Nat → Sy
  | bytes  : List Nat → Sy
  | float  : Sy
  | list   : List Sy → Sy
  | struct : List (String × Sy) → Sy
  | record : List Sy → Sy
  | tuple  : List Sy → Sy

/-- The wire form a tuple crosses as, spelled as the record it is. `Sy.tuple` is this shape named; the
prose in the module doc says why the model needs the name. -/
def taggedRecord (ss : List Sy) : Sy := .record [.sym taggedLabel, .sym tupleTag, .list ss]

/-- A `Par` holding one expression. -/
def syOne (e : Expr) : Par := Par.mk [] [] [] [e] [] [] [] []

/-- Whether an integer fits a machine word. -/
def fitsI64 (n : Int) : Bool := -9223372036854775808 ≤ n && n ≤ 9223372036854775807

/-- A Syrup integer as the narrow Rholang expression when it fits, the wide one otherwise — the choice
`par_value.rs`'s `value_to_par` makes, and the reason the domain excludes a *small* `GBigInt`. -/
def intExpr (n : Int) : Expr := if fitsI64 n then .ground (.int n) else .ebigint n

mutual
  /-- The **encode**: a `Par`, if it is a value, as a Syrup value. Mirrors `par_value.rs`'s
  `par_to_value`, refusal for refusal. -/
  def parToSy : Par → Option Sy
    | .mk [] [] [] [x] [] [] [] [] => exprToSy x
    | _ => none

  def parsToSy : List Par → Option (List Sy)
    | [] => some []
    | p :: ps => (parToSy p).bind (fun s => (parsToSy ps).map (fun ss => s :: ss))

  /-- A map key, which `par_to_value` takes only from a string. -/
  def keyToSy : Par → Option String
    | .mk [] [] [] [.ground (.str l)] [] [] [] [] => some (syString l)
    | _ => none

  def kvsToSy : List (Par × Par) → Option (List (String × Sy))
    | [] => some []
    | (k, v) :: kvs =>
      (keyToSy k).bind (fun kk => (parToSy v).bind (fun vv =>
        (kvsToSy kvs).map (fun rest => (kk, vv) :: rest)))

  /-- A tuple leaves as `taggedRecord`, the one record shape a peer can send back. -/
  def exprToSy : Expr → Option Sy
    | .ground (.bool b) => some (.bool b)
    | .ground (.int n) => some (.int n)
    | .ebigint n => some (.int n)
    | .ground (.str l) => some (.str l)
    | .ground (.uri l) => some (.sym l)
    | .ground (.bytes l) => some (.bytes l)
    | .etuple ps => (parsToSy ps).map (fun ss => .tuple ss)
    | .elist ps none => (parsToSy ps).map (fun ss => .list ss)
    | .emap kvs none => (kvsToSy kvs).map (fun ss => .struct ss)
    | _ => none
end

mutual
  /-- The **decode**: a Syrup value, if it has a `Par` shape, as one. -/
  def syToPar : Sy → Option Par
    | .bool b => some (syOne (.ground (.bool b)))
    | .int n => some (syOne (intExpr n))
    | .str l => some (syOne (.ground (.str l)))
    | .sym l => some (syOne (.ground (.uri l)))
    | .bytes l => some (syOne (.ground (.bytes l)))
    | .float => none
    | .list ss => (ssToPars ss).map (fun ps => syOne (.elist ps none))
    | .struct kvs => (kvsSyToPars kvs).map (fun kvs => syOne (.emap kvs none))
    | .tuple ss => (ssToPars ss).map (fun ps => syOne (.etuple ps))
    | .record _ => none

  def ssToPars : List Sy → Option (List Par)
    | [] => some []
    | s :: ss => (syToPar s).bind (fun p => (ssToPars ss).map (fun ps => p :: ps))

  def kvsSyToPars : List (String × Sy) → Option (List (Par × Par))
    | [] => some []
    | (k, v) :: kvs =>
      (syToPar v).bind (fun vv => (kvsSyToPars kvs).map (fun rest =>
        (syOne (.ground (.str (syChars k))), vv) :: rest))
end

mutual
  /-- The round trip's **domain**, over the wire side: the Syrup values from which a value comes back.
  A `float` is not one (Rholang has no float), and neither is a record that is not the tagged tuple —
  a peer's labelled record has no Rholang counterpart, so it is refused rather than read as a tuple. -/
  def wireable : Sy → Bool
    | .bool _ => true
    | .int _ => true
    | .str _ => true
    | .sym _ => true
    | .bytes _ => true
    | .float => false
    | .list ss => wireableList ss
    | .struct kvs => wireableKvs kvs
    | .tuple ss => wireableList ss
    | .record _ => false

  def wireableList : List Sy → Bool
    | [] => true
    | s :: ss => wireable s && wireableList ss

  def wireableKvs : List (String × Sy) → Bool
    | [] => true
    | (_, v) :: kvs => wireable v && wireableKvs kvs
end

/-! ## The arms, as `rfl` equations

`Json.lean`'s note applies: naming each arm keeps the round-trip proofs a `simp` over equations rather
than an unfolding of a large `mutual` block, which does not survive the heartbeat budget. -/

@[simp] theorem syOne_eq (e : Expr) : syOne e = Par.mk [] [] [] [e] [] [] [] [] := rfl
@[simp] theorem parToSy_mk (x : Expr) :
    parToSy (Par.mk [] [] [] [x] [] [] [] []) = exprToSy x := rfl
@[simp] theorem parsToSy_nil : parsToSy ([] : List Par) = some [] := rfl
@[simp] theorem parsToSy_cons (p : Par) (ps : List Par) :
    parsToSy (p :: ps) = (parToSy p).bind (fun s => (parsToSy ps).map (fun ss => s :: ss)) := rfl
@[simp] theorem keyToSy_str (l : List Nat) :
    keyToSy (Par.mk [] [] [] [.ground (.str l)] [] [] [] []) = some (syString l) := rfl
@[simp] theorem kvsToSy_nil : kvsToSy ([] : List (Par × Par)) = some [] := rfl
@[simp] theorem kvsToSy_cons (k v : Par) (kvs : List (Par × Par)) :
    kvsToSy ((k, v) :: kvs) =
      (keyToSy k).bind (fun kk => (parToSy v).bind (fun vv =>
        (kvsToSy kvs).map (fun rest => (kk, vv) :: rest))) := rfl

@[simp] theorem exprToSy_bool (b : Bool) : exprToSy (.ground (.bool b)) = some (.bool b) := rfl
@[simp] theorem exprToSy_int (n : Int) : exprToSy (.ground (.int n)) = some (.int n) := rfl
@[simp] theorem exprToSy_bigint (n : Int) : exprToSy (.ebigint n) = some (.int n) := rfl
@[simp] theorem exprToSy_str (l : List Nat) : exprToSy (.ground (.str l)) = some (.str l) := rfl
@[simp] theorem exprToSy_uri (l : List Nat) : exprToSy (.ground (.uri l)) = some (.sym l) := rfl
@[simp] theorem exprToSy_bytes (l : List Nat) : exprToSy (.ground (.bytes l)) = some (.bytes l) := rfl
@[simp] theorem exprToSy_etuple (ps : List Par) :
    exprToSy (.etuple ps) = (parsToSy ps).map (fun ss => .tuple ss) := rfl
@[simp] theorem exprToSy_elist (ps : List Par) :
    exprToSy (.elist ps none) = (parsToSy ps).map (fun ss => .list ss) := rfl
@[simp] theorem exprToSy_emap (kvs : List (Par × Par)) :
    exprToSy (.emap kvs none) = (kvsToSy kvs).map (fun ss => .struct ss) := rfl

@[simp] theorem syToPar_bool (b : Bool) :
    syToPar (.bool b) = some (syOne (.ground (.bool b))) := rfl
@[simp] theorem syToPar_int (n : Int) : syToPar (.int n) = some (syOne (intExpr n)) := rfl
@[simp] theorem syToPar_str (l : List Nat) :
    syToPar (.str l) = some (syOne (.ground (.str l))) := rfl
@[simp] theorem syToPar_sym (l : List Nat) :
    syToPar (.sym l) = some (syOne (.ground (.uri l))) := rfl
@[simp] theorem syToPar_bytes (l : List Nat) :
    syToPar (.bytes l) = some (syOne (.ground (.bytes l))) := rfl
@[simp] theorem syToPar_float : syToPar .float = none := rfl
@[simp] theorem syToPar_list (ss : List Sy) :
    syToPar (.list ss) = (ssToPars ss).map (fun ps => syOne (.elist ps none)) := rfl
@[simp] theorem syToPar_struct (kvs : List (String × Sy)) :
    syToPar (.struct kvs) = (kvsSyToPars kvs).map (fun kvs => syOne (.emap kvs none)) := rfl
@[simp] theorem syToPar_tuple (ss : List Sy) :
    syToPar (.tuple ss) = (ssToPars ss).map (fun ps => syOne (.etuple ps)) := rfl
@[simp] theorem syToPar_record (xs : List Sy) : syToPar (.record xs) = none := rfl
@[simp] theorem ssToPars_nil : ssToPars ([] : List Sy) = some [] := rfl
@[simp] theorem ssToPars_cons (s : Sy) (ss : List Sy) :
    ssToPars (s :: ss) = (syToPar s).bind (fun p => (ssToPars ss).map (fun ps => p :: ps)) := rfl
@[simp] theorem kvsSyToPars_nil : kvsSyToPars ([] : List (String × Sy)) = some [] := rfl
@[simp] theorem kvsSyToPars_cons (k : String) (v : Sy) (kvs : List (String × Sy)) :
    kvsSyToPars ((k, v) :: kvs) =
      (syToPar v).bind (fun vv => (kvsSyToPars kvs).map (fun rest =>
        (syOne (.ground (.str (syChars k))), vv) :: rest)) := rfl

@[simp] theorem wireable_bool (b : Bool) : wireable (.bool b) = true := rfl
@[simp] theorem wireable_int (n : Int) : wireable (.int n) = true := rfl
@[simp] theorem wireable_str (l : List Nat) : wireable (.str l) = true := rfl
@[simp] theorem wireable_sym (l : List Nat) : wireable (.sym l) = true := rfl
@[simp] theorem wireable_bytes (l : List Nat) : wireable (.bytes l) = true := rfl
@[simp] theorem wireable_float : wireable .float = false := rfl
@[simp] theorem wireable_list (ss : List Sy) : wireable (.list ss) = wireableList ss := rfl
@[simp] theorem wireable_struct (kvs : List (String × Sy)) :
    wireable (.struct kvs) = wireableKvs kvs := rfl
@[simp] theorem wireable_tuple (ss : List Sy) : wireable (.tuple ss) = wireableList ss := rfl
@[simp] theorem wireable_record (xs : List Sy) : wireable (.record xs) = false := rfl
@[simp] theorem wireableList_nil : wireableList ([] : List Sy) = true := rfl
@[simp] theorem wireableList_cons (s : Sy) (ss : List Sy) :
    wireableList (s :: ss) = (wireable s && wireableList ss) := rfl
@[simp] theorem wireableKvs_nil : wireableKvs ([] : List (String × Sy)) = true := rfl
@[simp] theorem wireableKvs_cons (k : String) (v : Sy) (kvs : List (String × Sy)) :
    wireableKvs ((k, v) :: kvs) = (wireable v && wireableKvs kvs) := rfl

/-- The model's code points round-trip: what makes a `str` or `sym` leaf lossless, and what makes a
map key come back as itself. -/
@[simp] theorem syString_syChars (s : String) : syString (syChars s) = s := by
  simp only [syString, syChars, List.map_map, Function.comp_def, Char.ofNat_toNat, List.map_id']
  exact String.asString_toList s

/-- A string-keyed map key, as the decoder builds it, reads back to the same string. -/
theorem keyToSy_of_str (s : String) :
    keyToSy (syOne (.ground (.str (syChars s)))) = some s := by
  simp [keyToSy, syOne]

/-! ## The round trip

`Json.lean`'s decomposition, one statement per shape the recursion descends through; they are mutual
because the decoder is, since a list's elements hold values whose own round trip goes back through the
list lemma. -/

/-- What `syToPar` answers, or `Nil` where it answers nothing. -/
def syDecoded (s : Sy) : Par := (syToPar s).getD nilPar

/-! ### The decoder answers on the domain

`Json.lean`'s `jeToPar_isSome`. A wireable value always decodes, which is what makes the `none` arms
of the round trip unreachable rather than a case to reason about. -/
mutual
  theorem syToPar_isSome : (s : Sy) → wireable s = true → (syToPar s).isSome = true
    | .bool b, _ => rfl
    | .int n, _ => rfl
    | .str l, _ => rfl
    | .sym l, _ => rfl
    | .bytes l, _ => rfl
    | .float, hf => by simp at hf
    | .list ss, hf => by
        rw [wireable_list] at hf
        simp [syToPar_list, Option.isSome_map, ssToPars_isSome ss hf]
    | .struct kvs, hf => by
        rw [wireable_struct] at hf
        simp [syToPar_struct, Option.isSome_map, kvsSyToPars_isSome kvs hf]
    | .tuple ss, hf => by
        rw [wireable_tuple] at hf
        simp [syToPar_tuple, Option.isSome_map, ssToPars_isSome ss hf]
    | .record xs, hf => by simp at hf

  theorem ssToPars_isSome : (ss : List Sy) → wireableList ss = true → (ssToPars ss).isSome = true
    | [], _ => rfl
    | s :: ss, hf => by
        simp only [wireableList_cons, Bool.and_eq_true] at hf
        cases hs : syToPar s with
        | none =>
            have h := syToPar_isSome s hf.1
            rw [hs] at h; simp at h
        | some p =>
            cases hss : ssToPars ss with
            | none =>
                have h := ssToPars_isSome ss hf.2
                rw [hss] at h; simp at h
            | some ps => simp [ssToPars_cons, hs, hss]

  theorem kvsSyToPars_isSome : (kvs : List (String × Sy)) → wireableKvs kvs = true →
      (kvsSyToPars kvs).isSome = true
    | [], _ => rfl
    | (k, v) :: kvs, hf => by
        simp only [wireableKvs_cons, Bool.and_eq_true] at hf
        cases hv : syToPar v with
        | none =>
            have h := syToPar_isSome v hf.1
            rw [hv] at h; simp at h
        | some p =>
            cases hk : kvsSyToPars kvs with
            | none =>
                have h := kvsSyToPars_isSome kvs hf.2
                rw [hk] at h; simp at h
            | some rest => simp [kvsSyToPars_cons, hv, hk]
end

mutual
  /-- **59a's encode half**: a wireable Syrup value encodes back to itself. -/
  theorem parToSy_decoded : (s : Sy) → wireable s = true → parToSy (syDecoded s) = some s
    | .bool b, _ => by simp [syDecoded]
    | .int n, _ => by
        simp only [syDecoded, syToPar_int, Option.getD_some, intExpr]
        split <;> simp
    | .str l, _ => by simp [syDecoded]
    | .sym l, _ => by simp [syDecoded]
    | .bytes l, _ => by simp [syDecoded]
    | .float, hf => by simp at hf
    | .list ss, hf => by
        rw [wireable_list] at hf
        cases hss : ssToPars ss with
        | none =>
            have hsome := ssToPars_isSome ss hf
            rw [hss] at hsome; simp at hsome
        | some ps => simp [syDecoded, hss, ssToPars_round ss ps hf hss]
    | .struct kvs, hf => by
        rw [wireable_struct] at hf
        cases hks : kvsSyToPars kvs with
        | none =>
            have hsome := kvsSyToPars_isSome kvs hf
            rw [hks] at hsome; simp at hsome
        | some kvs' => simp [syDecoded, hks, kvsSyToPars_round kvs kvs' hf hks]
    | .tuple ss, hf => by
        rw [wireable_tuple] at hf
        cases hss : ssToPars ss with
        | none =>
            have hsome := ssToPars_isSome ss hf
            rw [hss] at hsome; simp at hsome
        | some ps => simp [syDecoded, hss, ssToPars_round ss ps hf hss]
    | .record xs, hf => by simp at hf

  /-- The list legs: a wireable list of values that decodes to `ps` encodes back to itself. -/
  theorem ssToPars_round : (ss : List Sy) → (ps : List Par) → wireableList ss = true →
      ssToPars ss = some ps → parsToSy ps = some ss
    | [], ps, _, h => by
        simp only [ssToPars_nil, Option.some.injEq] at h
        rw [← h]; rfl
    | s :: ss, ps, hf, h => by
        simp only [wireableList_cons, Bool.and_eq_true] at hf
        obtain ⟨hfs, hfss⟩ := hf
        cases hs : syToPar s with
        | none => simp [ssToPars_cons, hs] at h
        | some p =>
            cases hss : ssToPars ss with
            | none => simp [ssToPars_cons, hs, hss] at h
            | some ps' =>
                have hinj : p :: ps' = ps := by simpa [ssToPars_cons, hs, hss] using h
                rw [← hinj]
                have h1 : parToSy p = some s := by
                  have hd := parToSy_decoded s hfs
                  rwa [syDecoded, hs, Option.getD_some] at hd
                have h2 := ssToPars_round ss ps' hfss hss
                simp [parsToSy_cons, h1, h2]

  /-- The map legs: keys are strings by construction and values round-trip as values. -/
  theorem kvsSyToPars_round : (kvs : List (String × Sy)) → (kvs' : List (Par × Par)) →
      wireableKvs kvs = true → kvsSyToPars kvs = some kvs' → kvsToSy kvs' = some kvs
    | [], kvs', _, h => by
        simp only [kvsSyToPars_nil, Option.some.injEq] at h
        rw [← h]; rfl
    | (k, v) :: kvs, kvs', hf, h => by
        simp only [wireableKvs_cons, Bool.and_eq_true] at hf
        obtain ⟨hfv, hfk⟩ := hf
        cases hv : syToPar v with
        | none => simp [kvsSyToPars_cons, hv] at h
        | some p =>
            cases hk : kvsSyToPars kvs with
            | none => simp [kvsSyToPars_cons, hv, hk] at h
            | some rest =>
                have hinj : (syOne (.ground (.str (syChars k))), p) :: rest = kvs' := by
                  simpa [kvsSyToPars_cons, hv, hk] using h
                rw [← hinj]
                have h1 : parToSy p = some v := by
                  have hd := parToSy_decoded v hfv
                  rwa [syDecoded, hv, Option.getD_some] at hd
                have h2 := kvsSyToPars_round kvs rest hfk hk
                have h3 := keyToSy_of_str k
                simp [kvsToSy_cons, h1, h2, h3]
end

/-- **59a** — a `Par` the bridge decoded is a `Par` the bridge encodes, and decodes again. Stated as
`Json.lean` states its round trip: over the values `syToPar` answers, so a shape outside the domain
carries no claim. -/
theorem syrup_decode_encode (s : Sy) (p : Par) (h : syToPar s = some p) (hw : wireable s = true) :
    (parToSy p).bind syToPar = some p := by
  have h1 := parToSy_decoded s hw
  rw [syDecoded, h, Option.getD_some] at h1
  rw [h1]
  simp [h]

/-- **59b** — two wireable Syrup values that decode to the same `Par` *are* the same wire form. This is
the clause a rendering rule violates: under "an inbound list becomes a tuple", a peer's list and a
peer's tuple would be two wire forms for one value. -/
theorem syToPar_injective (s s' : Sy) (p : Par) (hw : wireable s = true) (hw' : wireable s' = true)
    (h : syToPar s = some p) (h' : syToPar s' = some p) : s = s' := by
  have h1 := parToSy_decoded s hw
  have h2 := parToSy_decoded s' hw'
  rw [syDecoded, h, Option.getD_some] at h1
  rw [syDecoded, h', Option.getD_some] at h2
  rw [h1] at h2
  exact Option.some.inj h2

/-- An expression the encoder refuses. `exprToSy`'s arms are definitional, so each of these is `rfl`;
naming them keeps the fall-through of `exprToSy_wireable` a `simp` over equations. -/
@[simp] theorem exprToSy_evar (v : Var) : exprToSy (.evar v) = none := rfl
@[simp] theorem exprToSy_eneg (p : Par) : exprToSy (.eneg p) = none := rfl
@[simp] theorem exprToSy_enot (p : Par) : exprToSy (.enot p) = none := rfl
@[simp] theorem exprToSy_eplus (a b : Par) : exprToSy (.eplus a b) = none := rfl
@[simp] theorem exprToSy_eminus (a b : Par) : exprToSy (.eminus a b) = none := rfl
@[simp] theorem exprToSy_emult (a b : Par) : exprToSy (.emult a b) = none := rfl
@[simp] theorem exprToSy_ediv (a b : Par) : exprToSy (.ediv a b) = none := rfl
@[simp] theorem exprToSy_emod (a b : Par) : exprToSy (.emod a b) = none := rfl
@[simp] theorem exprToSy_elt (a b : Par) : exprToSy (.elt a b) = none := rfl
@[simp] theorem exprToSy_ele (a b : Par) : exprToSy (.ele a b) = none := rfl
@[simp] theorem exprToSy_egt (a b : Par) : exprToSy (.egt a b) = none := rfl
@[simp] theorem exprToSy_ege (a b : Par) : exprToSy (.ege a b) = none := rfl
@[simp] theorem exprToSy_eeq (a b : Par) : exprToSy (.eeq a b) = none := rfl
@[simp] theorem exprToSy_eneq (a b : Par) : exprToSy (.eneq a b) = none := rfl
@[simp] theorem exprToSy_eand (a b : Par) : exprToSy (.eand a b) = none := rfl
@[simp] theorem exprToSy_eor (a b : Par) : exprToSy (.eor a b) = none := rfl
@[simp] theorem exprToSy_ematches (a b : Par) : exprToSy (.ematches a b) = none := rfl
@[simp] theorem exprToSy_eshortand (a b : Par) : exprToSy (.eshortand a b) = none := rfl
@[simp] theorem exprToSy_eshortor (a b : Par) : exprToSy (.eshortor a b) = none := rfl
@[simp] theorem exprToSy_eset (ps : List Par) (r : Option Var) : exprToSy (.eset ps r) = none := rfl
@[simp] theorem exprToSy_emethod (m : List Nat) (t : Par) (a : List Par) :
    exprToSy (.emethod m t a) = none := rfl
@[simp] theorem exprToSy_epercentPercent (a b : Par) : exprToSy (.epercentPercent a b) = none := rfl
@[simp] theorem exprToSy_eplusPlus (a b : Par) : exprToSy (.eplusPlus a b) = none := rfl
@[simp] theorem exprToSy_eminusMinus (a b : Par) : exprToSy (.eminusMinus a b) = none := rfl
@[simp] theorem exprToSy_elist_rem (ps : List Par) (r : Var) :
    exprToSy (.elist ps (some r)) = none := rfl
@[simp] theorem exprToSy_emap_rem (kvs : List (Par × Par)) (r : Var) :
    exprToSy (.emap kvs (some r)) = none := rfl

mutual
  /-- **59c**, the `Par` leg: the encoder answers only for the shapes it can read back. -/
  theorem parToSy_wireable (p : Par) (s : Sy) (h : parToSy p = some s) : wireable s = true := by
    match p with
    | .mk send recv newn exprs mtchs unforgs bundles conns =>
    -- `parToSy` reduces only when every field is in constructor form, so each branch splits them all
    -- and then reads the answer off definitionally: `exprToSy x` on the one canonical shape, `none`
    -- (hence `Option.noConfusion`) everywhere else.
    cases exprs with
    | nil =>
        (cases send <;> cases recv <;> cases newn <;> cases mtchs <;>
          cases unforgs <;> cases bundles <;> cases conns) <;> exact Option.noConfusion h
    | cons x rest =>
      cases rest with
      | cons y ys =>
          (cases send <;> cases recv <;> cases newn <;> cases mtchs <;>
            cases unforgs <;> cases bundles <;> cases conns) <;> exact Option.noConfusion h
      | nil =>
          (cases send <;> cases recv <;> cases newn <;> cases mtchs <;>
            cases unforgs <;> cases bundles <;> cases conns) <;>
          first
            | exact exprToSy_wireable x s h
            | exact Option.noConfusion h

  /-- **59c**, the expression leg. -/
  theorem exprToSy_wireable : (e : Expr) → (s : Sy) → exprToSy e = some s → wireable s = true
    | .ground (.bool b), s, h => by
        simp only [exprToSy_bool] at h; rw [← Option.some.inj h]; rfl
    | .ground (.int n), s, h => by
        simp only [exprToSy_int] at h; rw [← Option.some.inj h]; rfl
    | .ebigint n, s, h => by
        simp only [exprToSy_bigint] at h; rw [← Option.some.inj h]; rfl
    | .ground (.str l), s, h => by
        simp only [exprToSy_str] at h; rw [← Option.some.inj h]; rfl
    | .ground (.uri l), s, h => by
        simp only [exprToSy_uri] at h; rw [← Option.some.inj h]; rfl
    | .ground (.bytes l), s, h => by
        simp only [exprToSy_bytes] at h; rw [← Option.some.inj h]; rfl
    | .etuple ps, s, h => by
        rw [exprToSy_etuple] at h
        cases hps : parsToSy ps with
        | none => simp [hps] at h
        | some ss =>
            simp only [hps] at h
            have hs : Sy.tuple ss = s := Option.some.inj h
            rw [← hs, wireable_tuple]
            exact parsToSy_wireable ps ss hps
    | .elist ps none, s, h => by
        rw [exprToSy_elist] at h
        cases hps : parsToSy ps with
        | none => simp [hps] at h
        | some ss =>
            simp only [hps] at h
            have hs : Sy.list ss = s := Option.some.inj h
            rw [← hs, wireable_list]
            exact parsToSy_wireable ps ss hps
    | .emap kvs none, s, h => by
        rw [exprToSy_emap] at h
        cases hks : kvsToSy kvs with
        | none => simp [hks] at h
        | some ss =>
            simp only [hks] at h
            have hs : Sy.struct ss = s := Option.some.inj h
            rw [← hs, wireable_struct]
            exact kvsToSy_wireable kvs ss hks
    | .evar _, s, h => by simp at h
    | .eneg _, s, h => by simp at h
    | .enot _, s, h => by simp at h
    | .eplus _ _, s, h => by simp at h
    | .eminus _ _, s, h => by simp at h
    | .emult _ _, s, h => by simp at h
    | .ediv _ _, s, h => by simp at h
    | .emod _ _, s, h => by simp at h
    | .elt _ _, s, h => by simp at h
    | .ele _ _, s, h => by simp at h
    | .egt _ _, s, h => by simp at h
    | .ege _ _, s, h => by simp at h
    | .eeq _ _, s, h => by simp at h
    | .eneq _ _, s, h => by simp at h
    | .eand _ _, s, h => by simp at h
    | .eor _ _, s, h => by simp at h
    | .ematches _ _, s, h => by simp at h
    | .eshortand _ _, s, h => by simp at h
    | .eshortor _ _, s, h => by simp at h
    | .eset _ _, s, h => by simp at h
    | .emethod _ _ _, s, h => by simp at h
    | .epercentPercent _ _, s, h => by simp at h
    | .eplusPlus _ _, s, h => by simp at h
    | .eminusMinus _ _, s, h => by simp at h
    | .elist _ (some _), s, h => by simp at h
    | .emap _ (some _), s, h => by simp at h

  /-- **59c**, the list leg. -/
  theorem parsToSy_wireable : (ps : List Par) → (ss : List Sy) → parsToSy ps = some ss →
      wireableList ss = true
    | [], ss, h => by
        simp only [parsToSy_nil] at h
        rw [← Option.some.inj h]; rfl
    | p :: ps, ss, h => by
        rw [parsToSy_cons] at h
        cases hp : parToSy p with
        | none => simp [hp] at h
        | some s' =>
            simp only [hp] at h
            cases hs : parsToSy ps with
            | none => simp [hs] at h
            | some ss' =>
                simp only [hs] at h
                have hcons : s' :: ss' = ss := Option.some.inj h
                rw [← hcons, wireableList_cons, Bool.and_eq_true]
                exact ⟨parToSy_wireable p s' hp, parsToSy_wireable ps ss' hs⟩

  /-- **59c**, the map leg. -/
  theorem kvsToSy_wireable : (kvs : List (Par × Par)) → (ss : List (String × Sy)) →
      kvsToSy kvs = some ss → wireableKvs ss = true
    | [], ss, h => by
        simp only [kvsToSy_nil] at h
        rw [← Option.some.inj h]; rfl
    | (k, v) :: kvs, ss, h => by
        rw [kvsToSy_cons] at h
        cases hk : keyToSy k with
        | none => simp [hk] at h
        | some kk =>
            simp only [hk] at h
            cases hv : parToSy v with
            | none => simp [hv] at h
            | some vv =>
                simp only [hv] at h
                cases hr : kvsToSy kvs with
                | none => simp [hr] at h
                | some rest =>
                    simp only [hr] at h
                    have hcons : (kk, vv) :: rest = ss := Option.some.inj h
                    rw [← hcons, wireableKvs_cons, Bool.and_eq_true]
                    exact ⟨parToSy_wireable v vv hv, kvsToSy_wireable kvs rest hr⟩
end

