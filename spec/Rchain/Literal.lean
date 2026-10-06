import Rchain.Par

/-!
# Law 65 — a value with no faithful literal is refused, not written

The pretty printer writes a value into a term as text, and the term is **parsed back as code** — in two
production paths, and in a deploy the node's own key signs. So a literal that does not read back as the
value it was written for is not a formatting wart: it is a term nobody wrote, in a signed deploy.

AUDIT C220: `PrettyPrinter` wrote a string as `"…"` with **no escaping**, and Rholang's literal grammar
has no escape at all (`rholang/src/parser.rs`), so a string containing a quote has no faithful literal —
the reader stops at the quote. Two paths printed values into terms they then parsed: the OCapN bridge's
builders, where the values arrive from a **peer**, and the same. The fix is `check_renderable`: the
printer refuses a value it cannot write faithfully, and the caller reports the argument rather than
signing a term whose body the peer chose.

**The shape of the law is Law 59's clause c, for the printer rather than the bridge**: an encoder never
emits a shape outside its domain — here, a literal that does not read back. That is why the statement is
about the *guard* rather than about the output: what the printer writes is what it checked, and a value
with no faithful literal is refused instead of approximated.

Layer `Rholang`; the Rust is `rholang/src/pretty_printer.rs`'s `check_renderable` and its two call
sites, `casper/src/shard_invoke.rs`'s builders.
-/

namespace Rchain

/-- A literal the printer would write: the value it was written for, and the value a reader gets back.
One `Nat` where the port has a `Par`, because the law is about the *guard* and not about the encoding. -/
structure Literal where
  /-- The value the printer meant to write. -/
  meant : Nat
  /-- The value reading the text back yields. -/
  reads : Nat

/-- A literal is **faithful** when it reads back as the value it was written for. -/
def Faithful (l : Literal) : Prop := l.reads = l.meant

/-- What the printer lets through: only a literal that reads back. -/
def mayWrite (l : Literal) : Bool := decide (l.reads = l.meant)

/-- **Law 65** — every literal the printer writes reads back as the value it was written for. -/
theorem the_printer_writes_only_what_reads_back (l : Literal) (writes : mayWrite l = true) :
    Faithful l := by
  simpa [mayWrite, Faithful] using writes

/-- And the other half of the same guard: a value with **no faithful literal** is refused rather than
written as one that reads back as something else. This is the clause the fix added — before it, the
printer had no such test and `check_renderable` did not exist. -/
theorem an_unfaithful_literal_is_refused (l : Literal) (unfaithful : ¬ Faithful l) :
    mayWrite l = false := by
  unfold Faithful at unfaithful
  simpa [mayWrite] using decide_eq_false unfaithful

/-- **The falsifier, and it is AUDIT C220's case.** A string containing a quote was written between
quotes, and reads back as the three characters before the quote: the printer meant seven units and a
reader got three, in a term the node signed. -/
def theQuotedStringCase : Literal := ⟨7, 3⟩

/-- The case is unfaithful, and the guard refuses it — stated as the pair, so that "the fix refuses the
right thing" is a theorem about this value rather than a description of the code. -/
theorem the_quoted_string_case_is_refused :
    ¬ Faithful theQuotedStringCase ∧ mayWrite theQuotedStringCase = false := by
  refine ⟨?_, ?_⟩
  · simp [Faithful, theQuotedStringCase]
  · exact an_unfaithful_literal_is_refused _ (by simp [Faithful, theQuotedStringCase])

end Rchain
