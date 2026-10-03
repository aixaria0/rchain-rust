#!/usr/bin/env bash
#
# **The #193 live arm: delegated stake end to end, and the fork point at the first `delegate` deploy.**
#
# Rewritten 2026-10-02 after the first version's readings turned out to be the *instrument's* fault
# (AUDIT C205, closed as an artifact — see `n203-native-write-probe.md`). Two things changed and both
# are load-bearing:
#
#   1. **Every answer is computed inside rholang and returned as a value** — `GInt`, `GBool` — instead
#      of publishing a structure and grepping the CLI's `Debug` rendering. `listen-data-at-name` prints
#      `{result:?}`, so a 65-byte key renders as `GByteArray([2, 2, …])`: **decimal**, invisible to any
#      hex grep. The first version reported a defect that did not exist because of exactly that.
#   2. **Every reading carries a control in the same reply.** The bond probe asks for the operator's
#      entry *and* an untouched validator's, so a reply that is wrong in the same direction as a broken
#      read is visible; a reading whose control is wrong is not a reading.
#
# What is measured:
#
#   1. `delegate` accepts (the op's own reply, off `@"pos-delegate"`).
#   2. **The aggregate reaches the chain**: the operator's entry goes 100 → 140, read by a *later*
#      block's deploy, with an untouched validator reading 100 in the same reply. Block placement is
#      taken from the node's own log — `--no-autopropose` does not disable `--propose-on-deploy`, the
#      deploy CLI returns before its block is built, and the pool is keyed by deploy **signature**, so
#      "later" is wall-clock order unless the block numbers say otherwise.
#   3. `undelegate` stages, and the boundary moves the principal out of the aggregate — back to 100.
#   4. **The quarantine payout**: with `--quarantine-length 1` on every node (a genesis parameter, so
#      all of them or none) the deadline is a few blocks away rather than 50 000. The delegator's
#      balance is read either side of it, and the *fee caveat* is stated rather than hidden: both reads
#      are deploys and deploys cost the delegator phlo, so the delta is the payout **minus** fees.
#   5. **The fork point** (`RUN_DIVERGENCE=1`, needs an unupgraded image): validator 2 runs the binary
#      from before this change, whose `rho:rchain:pos` has no `delegate` arm. It cannot reproduce the
#      block's post-state and refuses it, while the other nodes accept it.
#
# Run from the repository root:
#
#   spec/audit/evidence/n193-delegation-live-run.sh
#   RUN_DIVERGENCE=1 RNODE_OLD=rnode:old spec/audit/evidence/n193-delegation-live-run.sh
#
# **What this arm reaches, updated 2026-10-03 after AUDIT C207's fix.** Phases 1-3 are green and are
# the measurement: `delegate` accepted, the operator's aggregate reading 140 against an untouched
# control reading 100 *in the same reply from a later block's deploy*, and the wallet's HTTP route
# agreeing with the rholang read. Phase 4's *staging* is green too, read in the same block as the
# `undelegate` (a later read cannot see a staged state, because a `close_block` runs after a block's
# deploys and claims it in a boundary block).
#
# **Phase 4's payout has not been made to complete, and this file does not pretend otherwise.** The
# run's own account is `n193-delegation-live-results.md`: the operator's aggregate stayed at 140 after
# an `undelegate` that succeeded, measured twice by two instruments, so the principal never left the
# pool entry. The op and the block path are exonerated in process (see that file); where the live run
# loses it is *not* established, and the suspects are this arm's own read surfaces — wrong here four
# times — and the rig's block production, which kept proposing after both `--no-autopropose` and
# `--no-propose-on-deploy` were set.
#
# `rnode:old` is a second `cargo build --release` inside Docker (~10 min, ~2 GB) from the last `dev`
# before this change (`e55a117b4`). The driver **refuses** (`exit 2`) rather than skipping phase 5 when
# it is absent: a phase that quietly does not run is the failure mode this file exists to avoid.

set -uo pipefail
cd "$(dirname "$0")/../../.."

PREFIX="${DEVNET_PREFIX:-devnet}"
BOOTSTRAP="${PREFIX}-bootstrap"
OPERATOR_NODE="${PREFIX}-validator-2"
DELEGATOR_NODE="${PREFIX}-validator-1"
OUT="spec/audit/evidence/n193-delegation-live.log.txt"
BLOCKS="spec/audit/evidence/n193-delegation-live-blocks.log.txt"
EPOCH_LENGTH="${EPOCH_LENGTH:-2}"
QUARANTINE="${QUARANTINE:-1}"

say() { echo "[n193] $*" | tee -a "$OUT"; }
cli() { docker exec "$1" rnode --grpc-host localhost "${@:2}" 2>&1; }
pubkey_of() { docker exec "$BOOTSTRAP" cat /genesis/bonds.txt 2>/dev/null | sed -n "${1}p" | awk '{print $1}'; }
height_of() { cli "$1" status 2>/dev/null | sed -n 's/.*"latestBlockNumber": *\([0-9]*\).*/\1/p' | head -1; }
blocks_of() { docker logs "$BOOTSTRAP" 2>&1 | grep -c "proposed and added block"; }
HTTP_BASE="${HTTP_BASE:-40403}"
# The **wallet's** path, verbatim: it reaches the node over HTTP only, so this is the read the
# position screen will call.
position_of() { curl -s "http://localhost:${HTTP_BASE}/api/v1/pos/delegations?delegator=$1"; }
json_field() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)" 2>/dev/null || echo "PARSE-FAILED"; }

# **Force exactly one block and wait for it.** With `--no-autopropose --no-propose-on-deploy` nothing
# else proposes, so this is the only thing that ever advances the chain — which is what makes the run
# deterministic, and determinism is the whole of the rig's job here. The first version of this arm let
# the network propose and raced itself: two validators both propose on a gossiped deploy, a *boundary*
# height then has two blocks whose `close_block`s cannot compose, and the merge discards one whole —
# so a deploy's fate was a coin toss and the payout phase could never be measured. That is pre-existing
# boundary behaviour (`casper/src/merging.rs::sibling_boundaries_with_different_pots_merge` pins it),
# not this primitive's, and a deterministic rig steps around it rather than measuring it.
block() {
  local before after
  before="$(height_of "$BOOTSTRAP")"
  tools/devnet.sh propose >/dev/null 2>&1
  for _ in $(seq 1 60); do
    after="$(height_of "$BOOTSTRAP")"
    if [[ -n "$after" && "$after" != "$before" ]]; then
      return 0
    fi
    sleep 0.5
  done
  say "WARNING: propose produced no block (height stayed at $before)" >&2
  return 1
}

# Ask a live node a question, and read the answer **as a typed value**. `pattern` is matched against
# the CLI's rendering, which is why callers use `GInt`/`GBool` and never a hex string. The deploy does
# not make a block on its own here — `block` does — so the question is asked in a block this driver
# chose, and the answer is read at that block's state.
#
# **Every call publishes to a name of its own**, and that is a correctness fix rather than tidiness: a
# probe publishes with a plain send, which *accumulates*, and `listen-data-at-name` returns the data in
# the store's order rather than the newest first. So the second time a name is read the answer is the
# **first** reading back again — measured, not guessed: the payout phase compared the delegator's
# balance before and after eleven blocks and read the same number twice, byte for byte, while the
# deploys in between had each cost that account thousands of phlo. A per-call name makes each read the
# only datum at its channel.
ASK_N=0
ask() { # $1 = file under examples/ (already substituted), $2 = public name, $3 = grep -E pattern
  local uniq src
  ASK_N=$((ASK_N + 1))
  uniq="${2}-${ASK_N}"
  # Callers name the file the way `tools/devnet.sh deploy` does — relative to `examples/` — so the
  # same spelling has to resolve here, where `sed` reads from the repository root.
  src="$1"
  [[ -f "$src" ]] || src="examples/$1"
  sed "s/\"$2\"/\"$uniq\"/g" "$src" > "examples/n193-ask-${ASK_N}.rho"
  tools/devnet.sh deploy "n193-ask-${ASK_N}.rho" >/dev/null 2>&1
  block
  sleep 2
  timeout 90 docker exec "$BOOTSTRAP" rnode --grpc-host localhost \
    listen-data-at-name -t pub -c "\"$uniq\"" 2>&1 | grep -oE "$3" | head -4
  rm -f "examples/n193-ask-${ASK_N}.rho"
}

# The three probes, generated per run because each carries this run's keys. `trap` removes them however
# the run ends, including the `exit` refusals below.
LIVE_PROBES=(examples/n193-bonds-live.rho examples/n193-balance-live.rho
             examples/n193-delegate-live.rho examples/n193-undelegate-read-live.rho
             examples/n193-delegations-live.rho)
trap 'rm -f "${LIVE_PROBES[@]}"' EXIT

: > "$OUT"
say "=============== setup ==============="

if [[ "${RUN_DIVERGENCE:-0}" == "1" ]]; then
  OLD="${RNODE_OLD:-rnode:old}"
  if ! docker image inspect "$OLD" >/dev/null 2>&1; then
    say "RUN_DIVERGENCE=1 but no image '$OLD' — build it first (see this file's header). Refusing."
    exit 2
  fi
  export RNODE_IMAGE_${OPERATOR_NODE//-/_}="$OLD"
  say "validator 2 runs the unupgraded $OLD; the other nodes run \$RNODE_IMAGE"
fi

tools/devnet.sh down -v >/dev/null 2>&1 || true
say "starting 2 validators, epoch length $EPOCH_LENGTH, quarantine $QUARANTINE on every node"
# `--quarantine-length` is a *genesis* parameter, so it goes to every node or none (AUDIT C46): a
# per-node difference here is a different chain. `DEVNET_EXTRA_FLAGS` is the every-node channel.
#
# **And neither of the two things that make a node propose on its own.** `--no-autopropose` alone is
# not enough: with `--propose-on-deploy` on, *both* validators propose on every gossiped deploy, so a
# *boundary* height (`epoch $EPOCH_LENGTH` is 2 here, so every second height) still has two blocks whose
# `close_block`s carry different absolute snapshots of the same PoS leaves — the merge cannot compose
# them and discards one whole, deploy included. That is pre-existing boundary behaviour
# (`casper/src/merging.rs::sibling_boundaries_with_different_pots_merge` pins it) and running the arm
# into it measures *it* rather than the delegation. With both off, every block in this run is one this
# driver asked for, so no two are concurrent and the boundary race cannot arise.
DEVNET_EXTRA_FLAGS="--quarantine-length $QUARANTINE" \
  tools/devnet.sh up --validators 2 --epoch-length "$EPOCH_LENGTH" \
  --no-autopropose --no-propose-on-deploy --fresh 2>&1 | tail -6 | tee -a "$OUT"

DELEGATOR_PK="$(pubkey_of 1)"
OPERATOR_PK="$(pubkey_of 2)"
if [[ -z "$DELEGATOR_PK" || -z "$OPERATOR_PK" ]]; then
  say "REFUSING: the genesis bonds file yielded no validator keys — nothing could be measured"
  exit 1
fi
say "delegator (validator 1, the genesis-funded deployer) [${DELEGATOR_PK:0:16}…]"
say "operator  (validator 2, a key validator 1 does not control) [${OPERATOR_PK:0:16}…]"

# --- the probes ---------------------------------------------------------------------------------
#
# Each asks its question in rholang and returns a value. `getBonds` returns a Map, so the probe asks it
# for the two entries that matter; `getBalance` returns a number; the delegate/undelegate ops return
# their `(Bool, Either)`.
sed -e "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" -e "s/<DELEGATOR_PUBKEY_HEX>/$DELEGATOR_PK/" \
  examples/pos-bonds-check.rho > examples/n193-bonds-live.rho
sed "s/<DELEGATOR_PUBKEY_HEX>/$DELEGATOR_PK/" examples/pos-balance.rho > examples/n193-balance-live.rho
sed "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" examples/pos-delegate.rho > examples/n193-delegate-live.rho
sed "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" examples/pos-undelegate.rho > examples/n193-undelegate-live.rho
sed -e "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" -e "s/<DELEGATOR_PUBKEY_HEX>/$DELEGATOR_PK/" \
  examples/pos-undelegate-read.rho > examples/n193-undelegate-read-live.rho

before="$(blocks_of)"
say ""
say "=============== 1. delegate 40 to the operator ==============="
tools/devnet.sh deploy n193-delegate-live.rho 2>&1 | tail -1 | tee -a "$OUT"
block
say "the op's own reply (a refusal returns a tuple too, so the *reply* is the evidence, not the log):"
DELEGATE_REPLY="$(timeout 90 docker exec "$BOOTSTRAP" rnode --grpc-host localhost \
  listen-data-at-name -t pub -c '"pos-delegate"' 2>&1)"
grep -oE 'GBool\([a-z]+\)' <<<"$DELEGATE_REPLY" | head -1 | tee -a "$OUT"
if ! grep -q 'GBool(true)' <<<"$DELEGATE_REPLY"; then
  say "REFUSING to continue: the delegate deploy did not report success."
  exit 1
fi

say "blocks produced so far: $(blocks_of) (was $before)"
if [[ "$(blocks_of)" -le "$before" ]]; then
  say "REFUSING: the delegate did not produce a block, so nothing after it is a *later* block."
  exit 1
fi

DELEGATE_BLOCK="$(blocks_of)"
say ""
say "=============== 2. the aggregate, read by a later block's deploy ==============="
say "the operator's entry and an untouched validator's, in one reply:"
BONDS_AFTER="$(ask n193-bonds-live.rho pos-bonds-check 'GInt\([0-9]+\)')"
echo "$BONDS_AFTER" | tee -a "$OUT"
OPERATOR_STAKE="$(echo "$BONDS_AFTER" | sed -n 1p | grep -oE '[0-9]+')"
CONTROL_STAKE="$(echo "$BONDS_AFTER" | sed -n 2p | grep -oE '[0-9]+')"
say "operator=$OPERATOR_STAKE control=$CONTROL_STAKE (blocks: $DELEGATE_BLOCK -> $(blocks_of))"
if [[ "$CONTROL_STAKE" != "100" ]]; then
  say "REFUSING: the control is wrong (an untouched validator must read 100), so the *instrument* is"
  say "broken and the operator's reading says nothing."
  exit 1
fi
if [[ "$OPERATOR_STAKE" != "140" ]]; then
  say "the operator reads $OPERATOR_STAKE, not 140 — the delegation did NOT reach the aggregate."
  exit 1
fi
say "OK: the aggregate is 140 where the operator's own bond is 100, seen by a later block's deploy."

say ""
say "=============== 3. the delegator reads its own position (the wallet's path) ==============="
POSITION="$(position_of "$DELEGATOR_PK")"
say "GET /api/v1/pos/delegations?delegator=<delegator> -> $POSITION"
AMOUNT="$(json_field 'd[0]["amount"] if d else "EMPTY"' <<<"$POSITION")"
ACCRUED="$(json_field 'd[0]["accruedRewards"] if d else "EMPTY"' <<<"$POSITION")"
say "amount=$AMOUNT accruedRewards=$ACCRUED"
# **A control in the same shape as the write probe's**: a key that has never delegated must read an
# empty list, so a handler that ignored `delegator=` and returned whatever it found fails here.
OTHER="$(position_of "$OPERATOR_PK")"
say "control — the operator's own key as delegator -> $OTHER"
if [[ "$(json_field 'len(d)' <<<"$OTHER")" != "0" ]]; then
  say "REFUSING: a key that has never delegated read a non-empty list — the read is not scoped."
  exit 1
fi
if [[ "$AMOUNT" != "40" ]]; then
  say "the delegator's position does not read 40 (got '$AMOUNT') — the read does not see the write."
  exit 1
fi
say "OK: the delegator reads amount=40 on the operator's key, and a stranger's key reads []."

say "and the same read through rholang, for a contract rather than an operator:"
sed "s/<DELEGATOR_PUBKEY_HEX>/$DELEGATOR_PK/" examples/pos-delegations.rho \
  > examples/n193-delegations-live.rho
ROPOS="$(ask n193-delegations-live.rho pos-delegations 'GInt\([0-9]+\)')"
echo "$ROPOS" | head -4 | tee -a "$OUT"
if ! grep -q "GInt(40)" <<<"$ROPOS"; then
  say "REFUSING: pos!(\"getDelegations\") did not report the 40 the HTTP read did."
  exit 1
fi
say "OK: the rholang read agrees with the HTTP read."

say ""
say "=============== 4. the payout: balance before, undelegate, boundary, balance after ==============="
# **Three readings, and the third is the instrument's own control.** The balance probe is a deploy, so
# every reading of it is taken *after* that deploy's own phlo was charged — and the account may also
# have been paid for proposing. So two consecutive readings differ by a fixed, repeatable amount, and
# what the payout adds is measured *against that amount* rather than against zero: `f = B1 − B2` is the
# cost of one reading, `B0 − B1` must equal it (the control — if they differ, the reading is not
# repeatable and nothing below it means anything), and the payout is `(B3 − B2) + f`. Without the
# subtraction the assertion is unsatisfiable: the deploys between two reads cost more than the
# principal being returned, which is exactly what the earlier version of this phase measured and
# mistook for "the payout did not arrive".
BALANCE_BEFORE="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
BALANCE_MID="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
BALANCE_MID2="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
say "delegator balance, three readings before the undelegate: $BALANCE_BEFORE / $BALANCE_MID / $BALANCE_MID2"
BAL_N0="${BALANCE_BEFORE//[!0-9]/}"
BAL_N1="${BALANCE_MID//[!0-9]/}"
BAL_N2="${BALANCE_MID2//[!0-9]/}"
READING_COST_0=$((BAL_N0 - BAL_N1))
READING_COST_1=$((BAL_N1 - BAL_N2))
if [[ "$READING_COST_0" != "$READING_COST_1" ]]; then
  say "REFUSING: two identical readings cost $READING_COST_0 and $READING_COST_1, so a reading is not"
  say "repeatable and the payout measurement below would be noise."
  exit 1
fi
say "OK: one reading of this probe costs $READING_COST_1, repeatably — that is the instrument's floor."

# **The reply and the ledger are read in the same block as the staging, and that is the instrument.**
# A `close_block` runs *after* a block's deploys, so a staged undelegation in a boundary block is
# claimed by the same block and its staged state is never observable from a *later* read: the first
# version of this phase slept and then asked the HTTP route, and read `[]` on a chain whose every
# other height is a boundary. That was the arm being wrong about its subject. One deploy that stages
# and then reads cannot race the boundary, because the boundary has not run yet — and it is the
# sharper assertion anyway: an `undelegate` that dropped the ledger entry immediately would publish a
# count of `0` right here.
say "the undelegate's reply and the ledger count, read in the same block:"
UNDELEGATE_PAIR="$(ask n193-undelegate-read-live.rho pos-undelegate-read 'GBool\([a-z]+\)|GInt\([0-9]+\)')"
echo "$UNDELEGATE_PAIR" | tee -a "$OUT"
if ! grep -q 'GBool(true)' <<<"$UNDELEGATE_PAIR"; then
  say "REFUSING: the undelegate did not report success, so nothing after it is about a staged exit."
  exit 1
fi
if ! grep -q 'GInt(1)' <<<"$UNDELEGATE_PAIR"; then
  say "REFUSING: the ledger does not still carry the delegation in the block that staged its exit —"
  say "an undelegation is *staged* (law 47's shape), so the entry stays until the boundary."
  exit 1
fi
say "OK: the exit is staged — the op succeeded and the ledger still carries the entry in that block."

POSITION_STAGED="$(position_of "$DELEGATOR_PK")"
say "and the read surface reports the staged exit while it exists: $POSITION_STAGED"

# **The chain is advanced by forcing blocks, not by sleeping.** With `--no-autopropose` nothing
# proposes on its own, so a `sleep` waits for a block that will never come — which is what the first
# run of this phase did, for 25 seconds that produced nothing.
say "forcing blocks past the boundary and the quarantine (epoch $EPOCH_LENGTH, quarantine $QUARANTINE)"
for _ in $(seq 1 $((2 * EPOCH_LENGTH + QUARANTINE + 4))); do
  block
done
say "height after advancing: $(height_of "$BOOTSTRAP")"
BALANCE_AFTER="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
say "delegator balance after the payout: $BALANCE_AFTER"
BAL_N3="${BALANCE_AFTER//[!0-9]/}"
# The interval B2 -> B3 contains one reading and no other deploy, exactly like B1 -> B2 — so the same
# cost cancels and what is left is the payout.
PAYOUT=$(( BAL_N3 - BAL_N2 + READING_COST_1 ))
say "**The fee caveat, stated rather than hidden**: every balance read is itself a deploy by this"
say "account, so the raw delta is the payout *minus* that reading's cost plus any executor pay. The"
say "subtraction above removes it: the payout measured this way is $PAYOUT."
say "**And this phase does not assert on that number, because on this rig it cannot attribute it.**"
say "The reading is taken by a deploy of the *delegator's own* account, and on this devnet three"
say "consecutive readings across three blocks came back byte-identical — so the instrument's resolution"
say "for a payout of this size is the floor of zero shown above, and a value of 0 is consistent with"
say "both \"the payout arrived\" and \"it did not\". Saying that is the honest form; asserting either"
say "way from this number would not be. **The payout is measured in process instead**, by"
say "`rholang/src/native_state.rs::an_undelegation_is_staged_then_paid_to_the_delegator`, which drives"
say "the stage, the boundary move, the quarantine and the payment against a real store — and what this"
say "run asserts below is the fee-free half of the same event: the principal leaves the operator's"
say "aggregate and the ledger entry is gone."

BONDS_FINAL="$(ask n193-bonds-live.rho pos-bonds-check 'GInt\([0-9]+\)')"
echo "$BONDS_FINAL" | tee -a "$OUT"
FINAL_OPERATOR="$(echo "$BONDS_FINAL" | sed -n 1p | grep -oE '[0-9]+')"
say "the operator's entry after the payout: $FINAL_OPERATOR (100 = the principal left the aggregate)"
if [[ "$FINAL_OPERATOR" != "100" ]]; then
  say "REFUSING: the operator still reads $FINAL_OPERATOR after the undelegation was paid out, so the"
  say "principal never left the aggregate."
  exit 1
fi
say "OK: the aggregate is 140 while the delegation is live and 100 once it has been paid out, so the"
say "principal moved out of the operator's pool entry at the boundary."

say ""
say "=============== 5. the dump, for inspection ==============="
cli "$BOOTSTRAP" show-blocks --depth 30 > "$BLOCKS" 2>&1
say "the dump: $(wc -l < "$BLOCKS") lines, $(grep -c '^------------- block' "$BLOCKS" || true) blocks"

if [[ "${RUN_DIVERGENCE:-0}" == "1" ]]; then
  say ""
  say "=============== 6. the fork point: validator 2 on the unupgraded binary ==============="
  say "its refusals — it has no \`delegate\` arm, so it cannot reproduce the block's post-state:"
  docker logs "$OPERATOR_NODE" 2>&1 | grep -iE "InvalidStateHash|unknown method|refus|invalid state" \
    | tail -6 | tee -a "$OUT"
  say "heights — a divergence is validator 2 behind the other two:"
  for n in "$BOOTSTRAP" "$DELEGATOR_NODE" "$OPERATOR_NODE"; do
    say "   $n $(height_of "$n")"
  done
fi

say ""
say "done; the network is still up (tools/devnet.sh down -v to remove it)"
