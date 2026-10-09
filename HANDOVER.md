# Handover — the remediation programme, 2026-10-09

**Read this first, then delete it when the programme is done.**

Branch: `fix/293-genesis-replay-writer-window` → **PR #296**. `dev` is at `6844a40b8`; this branch is 15
commits ahead and pushed. Nothing is uncommitted.

---

## 0. How to work here — the rule this session broke twice

**Write code. Do not run hours of testing to test code that is not finished. Integration tests are for
finished code.**

Concretely, per unit:

1. Make the change.
2. Produce the **falsifier**: something that is demonstrably **red on the tree *before* the change** and
   green after. Three legitimate forms, and a passing test is not one of them:
   - a named unit test that **panics/fails on the pre-fix body** (revert the body, run, watch it fail,
     restore — this session did that four times and it takes seconds);
   - a **compile failure** (R1: the bad state is unrepresentable — the evidence is that the code which
     would create it does not build);
   - a **one-shot drill** or a committed run under `spec/audit/evidence/` for an outcome (R3).
3. `cargo fmt --all && cargo clippy -p <crate> --all-targets --no-deps -- $CLIPPY_DEBT -D warnings`
   and the **targeted** test path only.
4. Commit the code, then the register row, then push. **CI is the sweep.** Do not run the workspace
   suite, and do not run a devnet, to find out whether unfinished code is fine.

**Measured cost of not following it this session:** a 10-minute Docker image build plus a live
four-validator devnet, polled with `sleep`, to re-confirm that a suite which was already green is still
green. That is ~40 minutes and it proves nothing about the code. A devnet is for **an issue's close
condition**, once, at the end, never babysat.

If a unit cannot produce a red-before artifact, **say so** — it is a refactor, and it should be named as
one rather than dressed up.

---

## 1. What is landed (all on PR #296, all with falsifiers)

| unit | commit | closes | falsifier |
|---|---|---|---|
| the genesis replay's writer window | `d3b1554a9` | **#293** | `genesis_registry.rs::the_genesis_replays_to_itself` (red: the production error verbatim, `17 native write(s)`) |
| the node that gives up speaks | `071c14fb1` | C249's detect half | `interpreter_util::the_unrestorable_surface_names_the_block_it_gave_up_on` |
| `hexToBytes` reads bytes | `7913068a8` | **C252** | `reduce::hex_to_bytes_reads_bytes_…` (red: **panic**, `not a char boundary`) |
| a fault is not a verdict | `5c4aabbd7` | C253 E3+E5 | `an_internal_failure_is_a_fault_and_not_a_verdict` |
| the ceremony checks its writes | `19300bad1` | C253 E4 | `a_ceremony_that_cannot_write_fails_loudly_and_names_the_path` (red: returns `Ok` despite both writes failing) |
| a supervisor for detached tasks | `ab4decae0` | C254 E6/E6a | `supervise::a_panicking_task_and_a_returning_task_are_counted_apart` |
| a merge has one commit point | `bb4ea812a` | **C258** (new) | `root_repository::validate_known_root_refuses_without_publishing` |
| the gate says what it is not | `9d3286503` | C255 (partial) | negative control **run**: a production `.unwrap()` makes the gate fail |
| the mergeable codec refuses trailing bytes | `0dd5b021c` | C256 (safety half) | `a_mergeable_value_with_trailing_bytes_is_refused` |

Pass records: `spec/audit/passes.md` **§76–§80**. New register rows: **C257** (#293) and **C258** (the
merge's commit points). Register state: **293 findings, 7 `todo`** — `C215 C249 C250 C253 C254 C255 C256`.

**Also done:** the 2026-10-09 live run that closes #293 (`spec/audit/evidence/n293-genesis-replay-window/`)
— four validators, epoch length 10, a deploy included at block 28, finalised past both boundaries (24 →
31), the error counted 0 on all four nodes. That is issue-closure evidence, the one kind of heavy run
that was warranted. **Do not repeat it** to check anything else.

---

## 2. What remains — start at the top

### Unit 3 — C215, the merge's nondeterminism, **reproduced**
Not started. This is a **diagnosis**, not a fix: two nodes whose DAGs reached the same justifications in
different orders must be shown to compute the **same** pre-state. Its `owes` cell already carries the
shape (`latest_msgs` is the one order-sensitive map, and only on a tie). A chain split is the worst
outcome short of the unrecoverable one, which is why it ranks here. **The close condition is a
reproduction**, and there is an existing probe to build on:
`merging::the_merge_scope_does_not_depend_on_which_children_this_node_holds`.

### Unit 9 — C253's E2: route the poison recoveries through the counted accessors
Not started, and **read this before you try it**. The fix is real: `rlock`/`wlock`/`mlock` count a
poison recovery, but the raw `X.lock().unwrap_or_else(|p| p.into_inner())` pattern does not, so
`poisonRecoveries` can read 0 while recoveries happened. **77 production sites** across `rholang` (19),
`ocapn` (15), `rspace` (14), `casper` (8), `node` (8), `comm` (7), `shared` (6).

The accessors must move to `rchain-shared` first: `ocapn` and `comm` do **not** depend on `rspace`, and
`rchain-shared` is a dependency of every crate with a site. Then `rspace/src/lock.rs` becomes a
re-export (`pub use rchain_shared::lock::*;`) so `rchain_rspace::lock::poison_recoveries()` — read by
`node/src/api/web_api_impl.rs:577` — keeps resolving. Add a fourth accessor, `cwait(cv, guard)`: there
are two `Condvar` waits (`shared/src/sync_var.rs:38,46`) that recover a poison the same silent way.

**Do NOT do this with a regex rewrite.** I tried, twice, in this session: the receiver of `.lock()` has
to be found by walking back over the expression (including `self.a.b`, `(*x)`, multi-line chains), and
a regex cannot do it. The first pass produced `self.writermlock(self.writer)`; the second
`mlock(&*self.overlay)`; and it broke 30 files. The tree was restored to `d30031a6a`. **Go file by
file**, or write a parser, and land it **file by file** so each commit compiles. In `shared` itself the
import is `crate::lock::…`, not `rchain_shared::lock::…`.

Then add the hard `poison` class to `tools/audit-type-system.sh` (model it on `silent` — a
`scan_spanning` hard class with **no allow-list**, because its steady state is zero sites).

### Unit 2 — the rest of C249: the R3 reset
Half done (`071c14fb1` lands detect/log/count, dark; nothing acts). Remaining, in order:
(a) **persist the rewind anchor** — `last-finalized-block` is derived from the in-memory DAG
(`block-storage/src/dag/representation.rs:58-71`), so a wiped node has no local record of its own safe
state; the `ApprovedStore`'s `FinalizedFringe` (`block-storage/src/approved_store.rs:14-28`) is the
closest anchor. (b) **a DAG `drop_above(height)`** reusing `KeyValueTypedStore::delete`
(`shared/src/typed_store.rs:29`) and `RhoRuntime::reset` (`rholang/src/runtime.rs:440`). (c) the
detector that **acts** — it already detects; what is missing is the reset, and the plan says it ships
dark until an operator has seen what it would do. Formal home: law **53b** and `Unrestorable`
(`spec/Rchain/Progress.lean:120`); the guard it must redden by construction is
`the_refusal_is_persistent` (`spec/Rchain/Casper/Stranding.lean:66`). **#294/#287 owns the net-wide
half** — do not duplicate it.

### The partials
- **C256 — the mergeable mechanism.** The safety half landed. The **wiring** (`EvaluateResult.mergeable`
  ← the reducer's `merge_chs`, collected at `rholang/src/reduce.rs:3110` and never read) is deliberately
  **not done**: it changes `DeployMergeableData.channels` from `[]` to real data for every deploy, which
  changes every serialized block and therefore **every state hash** — a hard fork of #280's class. If you
  wire it, the `u16` truncation at `rholang/src/merging.rs:222` and the user-controlled per-deploy count
  (the audit priced 65 536 channels inside one deploy's budget) must be bounded **in the same unit**, and
  the fork must be registered and announced. Patrick chose "wire it" for this one; the sequencing, not
  the decision, is what was deferred.
- **C254 — the closed-channel half (E6b).** `let _ = tx.send(…)` on a shutdown channel is the same
  silence in a different shape: ~18 sites. `clippy::let_underscore_must_use` is in the `restriction`
  group (allow-by-default — CI's `-D warnings` does **not** turn it on); the proportionate fix is to
  scope it to `node`/`casper`/`comm` and log the `SendError` at those sites.
- **C255 — one clause.** The per-entry half of B3: an allow-list entry that covers several sites of
  identical text should **say** it covers a shape. Everything else in the row is closed or decided
  (§80).
- **C250** — a record correction, not code: needs #288 merged, and the witness's two defects fixed (a
  `#283` → `#287` mis-citation and rotated validator letters).

---

## 3. Mechanics that will bite you

- **The register is authored in two places.** Rows go in `spec/findings.tsv`; the pass account goes in
  `spec/audit/passes.md` as `## N. …`; `tools/emit-findings-register.sh` renders `spec/AUDIT.md`.
  `make check-register` runs the lot.
- **The emitter's evidence check reads `HEAD`, not the working tree.** Commit the code **and any
  evidence file** *before* the register row, or it refuses with `evidence naming nothing in the tree`.
  Same for a path you deleted: C255's evidence cell named `tools/type-system-baseline.tsv` and the check
  caught the stale citation — that is the check working.
- **`tools/check-staged-rows.sh <C-rows|\->` goes on the left of `&&`.** A commit that adds no *new*
  C-number declares `-`. Editing an existing row is not adding one.
- **A `todo` row's `owes` cell must name something that exists** (the emitter resolves it).
- **Changing `ApiStatus` moves five artifacts**: `node/src/api/dto.rs`, `node/src/api/conversion.rs`,
  the hand-written served schema in `node/src/web/http.rs`, `spec/Rchain/Envelope.lean`, and
  `spec/conformance/envelope.tsv` (plus the literals in `node/tests/lean_envelope_corpus.rs` and
  `node/src/web/http.rs`'s own test). The Lean corpus test refuses the DTO change until all of them move.
- **C-numbers and pass sections are shared.** This branch took C257/C258 and §76–§80 so it does not
  collide with the audit's C252–C256/§75 (PR #295, merged as `6844a40b8`). Next free is **C259**.
- **`gh` defaults to the parent repo.** Always `--repo rchain-community/rchain-rust` for PRs, issues
  and checks; a bare `gh pr view 295` reads `rchain/rchain`'s tracker.
- **Local clippy needs `-D warnings`** *and* CI's allow-list (`CLIPPY_DEBT` in
  `.github/workflows/ci.yml:52-90`), or a local run exits 0 on a tree CI calls red.
- `cargo fmt --all --check` is CI's **first** step, so an unformatted commit means Clippy never ran.

---

## 4. Traps hit today, so you do not repeat them

1. **The regex rewrite of Unit 9** (§2 above). The tree was restored; nothing was lost but time.
2. **A 40-minute devnet for a unit that was already green.** See §0.
3. **`#[must_use]` on the store traits** surfaces **0** sites — every production call is `.await`-ed and
   `Result` is already `#[must_use]` by type. It is not the structural half of C254.
4. **`--autopropose` breaks finality readings at N≥4**: an autopropose run produced 138 blocks with a
   finality stall, on the same image whose `--no-autopropose` arm finalised cleanly. Read finality from
   `tools/devnet.sh cli <node> last-finalized-block`, not from `latestBlockNumber` — and remember a
   "height" counts **rounds**, one block per bonded validator.
5. **`git restore`/`git checkout` of a file list is classified as destructive** by the local auto-mode
   and can be refused. Revert per file or with `git diff --name-only | xargs git restore`.

---

## 5. The one thing to carry forward

The audit read the code and found **shapes**; a live run found the **blocker** (#293). That is the third
time. A green suite is not evidence about a running node, and it is not evidence about an improvement.
Write the code, produce the red-before artifact, push, and let CI be CI.
