import Rchain.Par

/-!
# Law 64 — where the references disagree, the port speaks the implementations' reading

The OCapN draft is not the oracle of this port: the *implementations* are, because they are what every
peer actually speaks. That sentence is in `ocapn/src/captp.rs`'s module doc, and AUDIT C216 is why —
the draft defines `op:start-session` with **five** fields and contradicts itself about one of them,
while the test suite's `OpStartSession` carries **four**. Following the prose would have failed against
every implementation.

**And the implementations disagree with each other too.** AUDIT C217: `@endo/ocapn` sends a swiss number
as a Syrup *String* — which is what the draft says it is — and the Python suite sends a *byte array*.
A port built to either one alone refuses the other, which is how the divergence was found: by applying
C216's own rule. So the law has two halves, and they are different claims:

- **64a — the port *speaks* the implementations' reading.** Where the draft and the implementations
  differ, what goes on the wire is the implementations' (C216, and C218's reply channel, where the port
  had written a reading no implementation produces at all).
- **64b — the port *accepts* every reading a reference produces.** Where the implementations disagree
  with each other there is no single reading to speak, so the reading side is the **union**, and a
  reference's reading is never refused (C217; C224's locator hints and swiss-number spellings).

**Why this is a law and not a convention.** "Prefer the implementation" is untestable on its own — it
says nothing about which shapes are at issue or what happens when the implementations part. Stated with
the union on the reading side it is a property with a falsifier: a port that speaks one reading refuses
another, which is exactly what this port did before C217 and what `one_reading_refuses_another` states.

The evidence is the known-answer corpus (`ocapn/tests/reference_vectors.rs`, whose vectors are produced
by the suite's own `contrib/syrup.py` encoder) and the divergence rows this law enumerates in the
findings register.

Layer `Wire`; the Rust is `ocapn/src/captp.rs`, `ocapn/src/session.rs` (`peer.rs`'s locator hints),
`ocapn/src/bootstrap.rs` (the swiss number's two types) and `casper/src/shard_invoke.rs` (the reply
channel).
-/

namespace Rchain

/-- Who produced a reading: the draft's prose, or one of the two reference implementations. -/
inductive Source where
  /-- `draft-specifications/` — the prose, which is *not* the oracle. -/
  | draft
  /-- `ocapn-test-suite`, the Python reference. -/
  | suite
  /-- `@endo/ocapn`, Agoric's. -/
  | endo
deriving DecidableEq, Repr

/-- What a source produces for one wire shape, as a small enumerated form — a record's field count, or
the Syrup type a value arrives as. Enumerated rather than modelled in full because the law is about
*which* readings are spoken and accepted, not about what the bytes are. -/
abbrev Form := Nat

/-- A shape the sources disagree about: one reading per source that has one. -/
structure Disagreement where
  /-- The draft's reading, when it has one — recorded so that "the prose differs" is a fact about the
  row rather than a claim in a comment. -/
  prose : Option Form
  /-- The readings the implementations produce. The *first* is the one the suite produces, which is the
  one this port speaks where they agree. -/
  implementations : List Form

/-- **64a** — what the port puts on the wire: the implementations' reading, when they agree. A shape
whose implementations disagree has no reading to speak, and `none` says so rather than picking one. -/
def speaks (d : Disagreement) : Option Form :=
  if d.implementations.length = 1 then d.implementations.head? else none

/-- **64b** — what the port accepts: **every** reading a reference produces, the union rather than any
one of them. -/
def accepts (d : Disagreement) (f : Form) : Bool := decide (f ∈ d.implementations)

/-- **64a's clause.** Where the implementations agree, the port speaks *their* reading — and where the
prose differs, the port speaks the implementations' anyway. Stated as an inequality of the two
readings so that "the draft is not the oracle" is a theorem about the row rather than a preference. -/
theorem the_port_speaks_the_implementations_reading (d : Disagreement) (f : Form)
    (agree : d.implementations = [f]) : speaks d = some f := by
  simp [speaks, agree]

/-- And the **draft is not the oracle**: a shape whose prose reading differs from the implementations'
is still spoken as the implementations'. The two readings differ, so speaking one is not speaking the
other — which is the sentence `ocapn/src/captp.rs` states in prose and this states as a theorem. -/
theorem the_draft_is_not_the_oracle (d : Disagreement) (f g : Form)
    (agree : d.implementations = [f]) (prose : d.prose = some g) (different : g ≠ f) :
    speaks d ≠ d.prose := by
  simp [speaks, agree, prose, different.symm]

/-- **64b's clause** — no reference's reading is refused. This is the half C217 is: a string swiss
number and a byte-array one are both readings some reference produces, so both must be accepted. -/
theorem every_reference_reading_is_accepted (d : Disagreement) (f : Form)
    (h : f ∈ d.implementations) : accepts d f = true := by
  simp [accepts, h]

/-- **The falsifier, and it is C217's case.** The shape the fix replaces: the port spoke **one**
reading — the suite's, which is what "the reference implementation is the oracle" was taken to mean
before it was noticed that there are two — and refused the other. A port that accepts one reading
refuses another exactly when the readings differ, which is a fact about the rule rather than about the
peers. -/
def oneReading (d : Disagreement) (f : Form) : Bool :=
  match d.implementations with
  | g :: _ => decide (g = f)
  | [] => false

theorem one_reading_refuses_another (a b : Form) (different : a ≠ b) :
    oneReading ⟨none, [a, b]⟩ b = false := by
  simp [oneReading, different]

/-- And the contrast, stated so the pair is a claim: the union's rule accepts `b` on the same row. -/
theorem the_union_accepts_it (a b : Form) : accepts ⟨none, [a, b]⟩ b = true := by
  simp [accepts]

end Rchain
