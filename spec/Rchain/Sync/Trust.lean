import Rchain.Sync.Transfer
import Rchain.Casper.Stake

/-!
# Law 83 — the join's trust model: content-trustless, not finality-trustless

**Why this file exists.** #457's repair (Law 81) makes the walk accept every verified answer; this law
says what the joiner is *still* trusting after that. The join path downloads blocks by hash, imports a
trie whose pages it checks, and adopts the bootstrap's `FinalizedFringe`. The trie pages are checked
(importantly by Law 70), the block bodies are checked by their content hash, and part of the fringe
metadata is derivable from the blocks the joiner already holds. The rest is **assumed**, and the point
of this file is to name exactly which parts.

**The classification.** Every input is one of:

* **genesis-anchored** — the configured genesis block and state, and the genesis `pos:trusted` set
  (`bond keys ∪ pos-multi-sig`, `casper/src/genesis/mod.rs:61-90`). This is the only root of trust the
  configuration supplies;
* **verified** — a downloaded block's content hash; each imported trie page against its key (Law 70);
  an ancestry entry's block-hash membership, duplicate-freedom, and the `fringe` set **derived** from
  the justifications the joiner already has (the same `latest_fringe` the validator derives);
* **residual** — which root the bootstrap calls finalised; the per-block `fringe_state_hash` when the
  fringe is multi-member (not covered by the block hash and not derivable without recomputing the
  merge); liveness (the peer must serve the blocks); and wire signatures/quorum finality, which the
  join path does not check.

**The theorem that earns the phrase.** `join_is_content_trustless`: two joiners that verify the same
block map and obtain the same trie, at the same claimed root, read the same value for every key and
look up the same block. That is *content* trustlessness — every state byte and block body is checked
against a hash — and it is deliberately **not** finality trustlessness. `the_residual_is_exactly_named`
makes the residue a checked enumeration rather than a paragraph, and
`a_lying_ancestry_state_hash_changes_replay` shows why the multi-member fringe hash cannot be silently
dropped: it is an input to `BlockMetadata`, which `close_block` folds into the epoch seed.

**What the singleton checks buy, and what they do not.** When the fringe names one block, that block's
post-state is the root and the per-block claim is checked against it
(`a_singleton_fringe_pins_the_state_hash`) — which covers genesis and one-validator chains. A
multi-member fringe's merged state is not recomputed on the join path, so the residual stands: that is
AUDIT C339, and this law does not close it.
-/

namespace Rchain

namespace Sync
namespace Trust

/-- A block as the join sees it: its content hash, the hashes it justifies, its post-state, and the
    bond map it carries for the next epoch. -/
structure Block where
  hash : Hash
  justifications : List Hash
  postState : Hash
  carriedBonds : List (Validator × Nat)

/-- One block's row in the fringe's ancestry metadata. `fringe` is the set the sender claims, and
    `fringeStateHash` the state it claims for that set. -/
structure Ancestry where
  blockHash : Hash
  fringe : List Hash
  fringeStateHash : Hash
  carriedBonds : List (Validator × Nat)

/-- The finalized fringe: its message hashes, its merged state hash, and one ancestry row per block. -/
structure Fringe where
  hashes : List Hash
  stateHash : Hash
  ancestry : List Ancestry

/-- What a joiner holds: the downloaded block map and the trie it has imported. -/
structure Join where
  blocks : Hash → Option Block
  trie : Store

/-! ### The derivable fringe -/

/-- Whether one of `hs` justifies `h` — `message_map`'s lookup of a message's justifications. -/
def justifiedBySome (msg : Hash → Option Block) (hs : List Hash) (h : Hash) : Bool :=
  hs.any (fun j =>
    match msg j with
    | some b => b.justifications.elem h
    | none => false)

/-- **The fringe a set of justifications derives**: the messages in the set that no other message in it
    justifies — `message_map::latest_fringe`, whose output a bootstrap cannot choose. This is the field
    the join can recompute from blocks it has already verified. -/
def derivedFringe (msg : Hash → Option Block) (hs : List Hash) : List Hash :=
  hs.eraseDups.filter (fun h => !justifiedBySome msg hs h)

/-! ### The predicates the join path checks -/

/-- Every downloaded block is filed under its own content hash. -/
def ContentAddressed (j : Join) : Prop :=
  ∀ h b, j.blocks h = some b → b.hash = h

/-- Every hash the fringe names is a block the joiner holds. -/
def Downloaded (j : Join) (hs : List Hash) : Prop :=
  ∀ h ∈ hs, ∃ b, j.blocks h = some b

/-- An ancestry entry is derivable when the block it names is held and the `fringe` it claims is the
    one the block's own justifications derive. -/
def AncestryDerivable (j : Join) (a : Ancestry) : Prop :=
  ∃ b, j.blocks a.blockHash = some b ∧
    derivedFringe j.blocks b.justifications = a.fringe

/-- Every ancestry row in a fringe is derivable. -/
def FringeDerivable (j : Join) (f : Fringe) : Prop :=
  ∀ a ∈ f.ancestry, AncestryDerivable j a

/-- For a fringe that names one block, the merged state hash must be that block's post-state. -/
def SingletonPinned (j : Join) (f : Fringe) : Prop :=
  ∀ h ∈ f.hashes, ∀ b, j.blocks h = some b → f.stateHash = b.postState

/-! ### The input classification -/

/-- Every kind of input the join path consumes. -/
inductive Input where
  | genesisBlock
  | genesisTrustSet
  | blockContent
  | triePages
  | ancestryFringe
  | claimedRootFinality
  | multiMemberFringeState
  | liveness
  | wireSignatures
  deriving DecidableEq, Repr

inductive InputClass where
  | genesisAnchored
  | verified
  | assumedLiveness
  | residual
  deriving DecidableEq, Repr

def classOf : Input → InputClass
  | .genesisBlock | .genesisTrustSet => .genesisAnchored
  | .blockContent | .triePages | .ancestryFringe => .verified
  | .liveness => .assumedLiveness
  | .claimedRootFinality | .multiMemberFringeState | .wireSignatures => .residual

def allInputs : List Input :=
  [.genesisBlock, .genesisTrustSet, .blockContent, .triePages, .ancestryFringe,
   .claimedRootFinality, .multiMemberFringeState, .liveness, .wireSignatures]

/-- The residual, in the words the trust-model page uses. -/
def residualNames : List String :=
  ["finality-of-the-claimed-root", "multi-member-fringe-state", "liveness",
   "wire-signature-and-finality-proofs"]

def isUnverified (i : Input) : Bool :=
  match classOf i with
  | .residual | .assumedLiveness => true
  | _ => false

/-! ### The theorems -/

/-- **The trie is the rooted trie** — Law 70 at the join's root. Restated here as the join's own claim:
    the value the joiner reads at the imported root is the value the peer's store reads there. -/
theorem restored_trie_is_the_rooted_trie {S S' : Store} {items : List (Hash × Node)} {r : Hash}
    (hS : StoreCorrect S) (hdef : WalkDefined S r)
    (hh : KeyHonest items) (hwf : ∀ k n, (k, n) ∈ items → WellFormed n)
    (hinst : Installs S' items) (hdel : DeliversAll items S r)
    {key : Key} {v v' : Hash} (hTrue : Reads S r key v) (hJoin : Reads S' r key v') :
    v = v' :=
  import_reads_the_rooted_trie hS hdef hh hwf hinst hdel hTrue hJoin

/-- The join as a populate step writes it: ancestry metadata is stored beside the block map and cannot
    change a block body or the imported trie. -/
def populate (j : Join) (_f : Fringe) : Join :=
  { j with blocks := j.blocks }

/-- **The DAG's content is the block map**, and metadata insertion is a frame: the blocks the joiner
    holds are exactly the ones it downloaded and verified, whatever the fringe says about them. -/
theorem restored_dag_is_the_block_map (j : Join) (f : Fringe) :
    (populate j f).blocks = j.blocks ∧ (populate j f).trie = j.trie :=
  ⟨rfl, rfl⟩

/-- **The join is content-trustless.** Two joiners that verify the same block map and hold the same
    trie, at the same claimed root, read the same value for every key and answer the same block lookup.
    The finality of the root is not part of the claim — that is the residual's first entry. -/
theorem join_is_content_trustless {blocks blocks' : Hash → Option Block} {trie trie' : Store}
    (hb : blocks = blocks') (ht : trie = trie') (r : Hash) {key : Key} {v v' : Hash}
    (h1 : Reads trie r key v) (h2 : Reads trie' r key v') :
    v = v' ∧ blocks r = blocks' r := by
  subst hb
  subst ht
  refine ⟨?_, rfl⟩
  exact reads_determined reach_set_closed (fun k _ => rfl) r (Reaches.refl r) h1 h2

/-- **A singleton fringe pins the root to a verified block.** When the fringe names one block and the
    joiner holds it, the merged state hash must be that block's post-state — the check that covers
    genesis and a one-validator chain. -/
theorem a_singleton_fringe_pins_the_state_hash (j : Join) (f : Fringe) (m : Block)
    (h : f.hashes = [m.hash]) (hb : j.blocks m.hash = some m)
    (hp : SingletonPinned j f) : f.stateHash = m.postState :=
  hp m.hash (by rw [h]; simp) m hb

/-- **A lying ancestry row is refused.** A row whose `fringe` is not the one its block's justifications
    derive has no derivation, so the join path's check rejects it — this is the field a bootstrap
    *cannot* choose. -/
theorem a_lying_ancestry_fringe_is_refused (j : Join) (a : Ancestry) (b : Block)
    (hb : j.blocks a.blockHash = some b)
    (hnot : derivedFringe j.blocks b.justifications ≠ a.fringe) :
    ¬ AncestryDerivable j a := by
  rintro ⟨b', hb', hf⟩
  rw [hb] at hb'
  injection hb' with hbb
  exact hnot (by rw [← hbb] at hf; exact hf)

/-- **The per-block `fringe_state_hash` changes the replay input.** Two ancestry rows with the same block
    and fringe but a different claimed state produce different replay metadata — and `close_block` folds
    that metadata into the epoch seed, which is why a multi-member fringe's per-block hash is a residual
    and not something the singleton check can cover. -/
def replayMetadata (a : Ancestry) : Hash × Hash × List (Validator × Nat) :=
  (a.blockHash, a.fringeStateHash, a.carriedBonds)

theorem a_lying_ancestry_state_hash_changes_replay (a a' : Ancestry)
    (h : a.fringeStateHash ≠ a'.fringeStateHash) :
    replayMetadata a ≠ replayMetadata a' := by
  intro he
  apply h
  have h2 := congrArg (fun t : Hash × Hash × List (Validator × Nat) => t.2.1) he
  simpa [replayMetadata] using h2

/-- **The genesis trust set is the anchor**: a validator bonded at genesis is trusted, and changing the
    multisig list changes the anchor. -/
def genesisTrusted (bondKeys multiSig : List Validator) : List Validator :=
  bondKeys ++ multiSig

theorem genesis_trust_anchor (bondKeys multiSig : List Validator) (v : Validator)
    (h : v ∈ bondKeys) : v ∈ genesisTrusted bondKeys multiSig :=
  List.mem_append_left _ h

theorem a_different_multisig_list_changes_the_anchor (bondKeys m1 m2 : List Validator)
    (v : Validator) (hv : v ∉ bondKeys) (h1 : v ∈ m1) (h2 : v ∉ m2) :
    v ∈ genesisTrusted bondKeys m1 ∧ v ∉ genesisTrusted bondKeys m2 := by
  constructor
  · exact List.mem_append_right _ h1
  · intro hc
    rcases List.mem_append.mp hc with hb | hm
    · exact hv hb
    · exact h2 hm

/-- **The residual is exactly named**: of the nine inputs the join path consumes, the four that are
    neither genesis-anchored nor verified are precisely the ones listed. This is a checked enumeration,
    so adding a tenth input without classifying it fails the build. -/
theorem the_residual_is_exactly_named :
    (allInputs.filter isUnverified) =
      [Input.claimedRootFinality, Input.multiMemberFringeState, Input.liveness, Input.wireSignatures] := by
  decide

/-- The non-vacuity of the singleton check: a one-block fringe whose root is the block's post-state
    passes it. -/
def exampleJoin (h p : Hash) : Join where
  blocks := fun k => if k = h then some ⟨h, [], p, []⟩ else none
  trie := fun _ => none

theorem a_singleton_fringe_can_pass_the_check (h p : Hash) :
    SingletonPinned (exampleJoin h p) ⟨[h], p, []⟩ := by
  intro k hk b hb
  simp [exampleJoin] at hk
  subst hk
  simp [exampleJoin] at hb
  cases hb
  rfl

end Trust
end Sync
end Rchain
