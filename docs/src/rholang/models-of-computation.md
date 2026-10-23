# Models of computation, by capability

This page is the technical companion to [Why rholang](why-rholang.md). That page gives the intuition —
why a *reflective* process calculus is the right shape for a blockchain. This one sets the models side by
side, says what each of the six capabilities actually means, and points at the laws where the ones that
matter are made precise.

It is worth saying at the start what kind of claim the table makes. The scores are a comparison of
**native capability**: whether a property is built into the model and its theory, rather than supplied by
a library, a tool, or a convention on top. That is an argument about fitness for a purpose, not a theorem.
What this repository *proves* lives in [`spec/`](../../../spec/), and the law register
([`spec/LAWS.md`](../../../spec/LAWS.md)) is the authority for it; where this page says "the calculus
guarantees", that is the pointer to follow.

## The comparison

| Model | Complete | Compositional | Concurrent | Complexity | Reflection | Behavioural types | Score |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| Turing machines | ✅ | ❌ | ❌ | ✅ | ❌ | ❌ | **2 / 6** |
| Lambda calculus | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | **2 / 6** |
| CCS | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | **3 / 6** |
| Petri nets | ✅ | ❌ | ✅ | ✅ | ❌ | ❌ | **3 / 6** |
| π-calculus | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | **5 / 6** |
| **ρ-calculus** | ✅ | ✅ | ✅ | ✅ | **✅** | ✅ | **6 / 6** |

✅ has the capability · ❌ lacks it

## What the six capabilities mean

**Complete.** The model can express every computable function. Every model in this table has it — it is
the price of admission for a general-purpose language, and it is why "Turing-complete" is never by itself
a reason to choose one.

**Compositional.** The behaviour of a composite is determined by the behaviour of its parts, so a program
can be reasoned about by reasoning about pieces and their combination. For a process calculus this is the
**congruence** requirement: structural congruence `≡` must be preserved by every constructor, or a
substitution that is valid in isolation stops being valid in context. It is law 2 in the register, and it
is what makes the rest of the laws composable.

**Concurrent.** The model has concurrent composition as a primitive **and defines when two concurrent
steps commute**. This is the one worth being careful about, because "can run things at once" is not the
property — every model can be scheduled across cores. The property is that the *theory* says which
reorderings preserve behaviour, so parallel execution is **sound** rather than merely hoped-for: the
scheduler may run two processes at the same time exactly when the calculus says doing so is
indistinguishable. [Concurrency: the model, the calculus, and the soundness theorems](../formal/concurrency.md)
is where the four-level model and the per-law profile live; [Effect scheduling](../formal/scheduling.md)
is how a node exploits it; and [Determinism of the block state transition](../formal/determinism.md) is
why exploiting it does not cost agreement between nodes.

**Complexity.** The model carries structure whose *size* is part of the theory — an unbounded tape, an
unbounded marking, an unbounded number of processes, or a communication topology that can change.
Fixed-shape models lack it: a function over fixed data has no notion of "more structure than this", and a
calculus with a fixed set of channels cannot grow the network it describes.

**Reflection.** A name can be a **quoted process**, and a process can **dereference** a name back into a
process at run time. Code is data and data is code *in the calculus*, not in a serialization library
bolted onto it. This is the property that separates the last row from the one above it, and it is what
[Names are quoted processes](names-are-processes.md) is about.

**Behavioural types.** The type system can describe *behaviour* — a protocol, a sequence of interactions,
what a channel will be used for — and not only the shape of data. It is what lets a capability-carrying
language say "this handle speaks this protocol" and check it.

## The models, briefly

- **Turing machines.** A tape, a head, and a finite control. Complete and unbounded, but inherently
  sequential and shaped as one machine, so it composes poorly and describes a single thread of control.
- **Lambda calculus.** Substitution and application: the model every functional language descends from.
  Compositional and complete, and *the* model of sequential computation — but concurrency is not in it.
- **CCS** (calculus of communicating systems). The first big step: processes and channels, with
  concurrency as a primitive and a bisimulation theory of when two processes behave alike. It gives up
  unbounded structure — its topology is fixed by the term — and it has no reflection.
- **Petri nets.** Places, tokens, transitions: a concurrency model with genuine unbounded state (a marking
  can grow) and a mature analysis theory. Concurrency and complexity, but not compositionality in the
  process-calculus sense, and no reflection.
- **π-calculus.** CCS's successor. Names are values that travel over channels, so the communication
  topology itself can change — mobility. Complete, compositional, concurrent, unbounded, with behavioural
  (session) type theories built on it. It lacks exactly one thing: a name cannot be a process.
- **ρ-calculus.** The π-calculus with that one step taken. A name *is* a quoted process, and `*x`
  evaluates a name back into a process. Meredith and Radestock, 2005.

## What "6 / 6" actually claims

The honest version of the claim is narrow and precise, and it is worth stating in that form because the
table invites a stronger reading than it supports.

> **ρ = π + reflection.**

Five of the six columns ρ has because it is a π-calculus. The sixth it has because of reflection. The
table is not a claim that ρ is better at concurrency than π, or at types — those columns are shared — and
it is not a claim that Turing machines or the λ-calculus are weak. It is a claim about a *specific
capability that a blockchain language needs and that only one of these models has natively*: the ability
to treat a name as code, so that a running system can be told about a new contract, a new protocol, or a
new rule without a serialization boundary in the way.

Reflection is not free, and it is worth saying what it costs. If a name is a quoted process, then
comparing two names is comparing two processes — so α-equivalence and canonicalization stop being
convenient normalizations and become load-bearing (laws 1 and 2). It also means the type discipline has to
say what a quoted process *is*: the port embeds ρ as the base sort of a Calculus of Constructions, with
sorts for names, patterns and processes, and proves that classification is functional and decidable
([Grammar and sorts](../formal/grammar-sorts.md),
[Closedness and the Calculus of Constructions](../formal/closedness-coc.md)). Those theorems are the
price of the sixth column, and the reason the earlier five are still worth having in the same model.

## Safe by construction

The phrase is used in two different senses in this repository, and they should not be run together.

**In the model**, "safe by construction" means the rules are stated once and the implementation has to
match them: a read is a function of the version it names; two contents that are congruent are the same
name; a reset is a projection onto its root. Each of those is a law, tied to the code by a conformance
corpus or a named test, and refused stale by a gate. A behaviour that is derived from a law cannot drift
from it without something going red.

**In the implementation**, it means classes of defect are *unwritable* rather than tested for. `unsafe` is
forbidden across the crate graph, so use-after-free, double free, out-of-bounds access and data races do
not compile. Refinement newtypes carry invariants that would otherwise be comments: a non-negative amount
is a `NonNegI64`, not an `i64` with a check somewhere. The type discipline borrows the calculus's own
sorts, so a term that classifies as a name cannot be run as a process.

What that does **not** claim is worth stating, because it is where this framing is usually oversold. A
leak is safe code (`mem::forget`, a reference cycle, an unbounded cache) — Rust makes lifetime visible in
the types, not impossible to get wrong. Shipping no garbage collector is a property of *this binary*, not
of the language: a stop-the-world collector is ordinary safe Rust. And "no silent partiality" is a
reviewed discipline backed by a gate that refuses `unwrap`/`expect`/`panic!` in production paths, not a
theorem of the type system. [Why Rust](../contributor/why-rust.md) sets out the three claims with their
limits, and [`spec/TYPE-SYSTEM.md`](../../../spec/TYPE-SYSTEM.md) is the machine-checked half.

## Where to go next

- [Why rholang](why-rholang.md) — the same landscape, from the intuition end.
- [Names are quoted processes](names-are-processes.md) — reflection, concretely.
- [Concurrency: the model, the calculus, and the soundness theorems](../formal/concurrency.md) — the
  concurrency column, made precise.
- [The laws](../formal/laws.md) — the catalog, with the register as its authority.
- [Why Rust](../contributor/why-rust.md) — the implementation-side argument and its limits.
