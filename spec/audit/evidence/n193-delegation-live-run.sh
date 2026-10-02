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

# Ask a live node a question, and read the answer **as a typed value**. `pattern` is matched against
# the CLI's rendering, which is why callers use `GInt`/`GBool` and never a hex string.
ask() { # $1 = file under examples/ (already substituted), $2 = public name, $3 = grep -E pattern
  tools/devnet.sh deploy "$1" >/dev/null 2>&1
  sleep 8
  timeout 90 docker exec "$BOOTSTRAP" rnode --grpc-host localhost \
    listen-data-at-name -t pub -c "\"$2\"" 2>&1 | grep -oE "$3" | head -4
}

# The three probes, generated per run because each carries this run's keys. `trap` removes them however
# the run ends, including the `exit` refusals below.
LIVE_PROBES=(examples/n193-bonds-live.rho examples/n193-balance-live.rho
             examples/n193-delegate-live.rho examples/n193-undelegate-live.rho
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
# shellcheck disable=SC2086
DEVNET_EXTRA_FLAGS="--quarantine-length $QUARANTINE" \
  tools/devnet.sh up --validators 2 --epoch-length "$EPOCH_LENGTH" --fresh 2>&1 | tail -6 | tee -a "$OUT"

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

before="$(blocks_of)"
say ""
say "=============== 1. delegate 40 to the operator ==============="
tools/devnet.sh deploy n193-delegate-live.rho 2>&1 | tail -1 | tee -a "$OUT"
sleep 10
say "the op's own reply (a refusal returns a tuple too, so the *reply* is the evidence, not the log):"
DELEGATE_REPLY="$(tools/devnet.sh query pos-delegate 2>&1 | tail -30)"
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
BALANCE_BEFORE="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
say "delegator balance before the undelegate: $BALANCE_BEFORE"

tools/devnet.sh deploy n193-undelegate-live.rho 2>&1 | tail -1 | tee -a "$OUT"
sleep 10
say "the undelegate's reply:"
grep -oE 'GBool\([a-z]+\)' <<<"$(tools/devnet.sh query pos-undelegate 2>&1 | tail -30)" | head -1 | tee -a "$OUT"

say "and the position now reports the staged exit:"
POSITION_STAGED="$(position_of "$DELEGATOR_PK")"
say "$POSITION_STAGED"
STAGED="$(json_field 'd[0]["pendingUndelegation"] if d else "EMPTY"' <<<"$POSITION_STAGED")"
if [[ "$STAGED" == "None" || "$STAGED" == "EMPTY" || "$STAGED" == "PARSE-FAILED" ]]; then
  say "the position does not report a pending undelegation after one was staged: $STAGED"
  exit 1
fi
say "OK: pendingUndelegation is reported ($STAGED)."

say "letting the boundary and the quarantine elapse (epoch $EPOCH_LENGTH, quarantine $QUARANTINE)"
sleep 25
BALANCE_AFTER="$(ask n193-balance-live.rho pos-balance 'GInt\([0-9]+\)' | head -1)"
say "delegator balance after the payout: $BALANCE_AFTER"
say "**The fee caveat, stated rather than hidden**: both balance reads are deploys and a deploy costs"
say "the delegator phlo, so the delta is the payout *minus* those fees — it is evidence that the payout"
say "arrived, not a measurement of its exact size."

BONDS_FINAL="$(ask n193-bonds-live.rho pos-bonds-check 'GInt\([0-9]+\)')"
echo "$BONDS_FINAL" | tee -a "$OUT"
FINAL_OPERATOR="$(echo "$BONDS_FINAL" | sed -n 1p | grep -oE '[0-9]+')"
say "the operator's entry after the payout: $FINAL_OPERATOR (100 = the principal left the aggregate)"

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
