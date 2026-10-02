#!/usr/bin/env bash
#
# **The #193 live arm: delegated stake end to end on a two-validator net (C204).**
#
# What is measured, in the order the primitive's own close condition names them:
#
#   1. **`delegate` on a live network.** Validator 1's vault holds REV (genesis funds the deployer,
#      which is validator 1 by default), and it delegates 40 of it to **validator 2's key** — a key it
#      does not control. The deploy's own reply is the first check: `(true, Nil)`.
#   2. **The aggregate reaches consensus.** The operator's `pos:bonds` entry goes 100 → 140 and, after a
#      boundary, so does the *active* set — which is what `compute_bonds`, finality and the block's bond
#      cache read. This is the "no Casper change was needed" claim observed on chain rather than
#      reasoned about.
#   3. **A boundary splits the operator's reward** across its own stake and the delegation, with the
#      operator keeping the remainder — `Σ delegators + operator == the reward` exactly.
#   4. **`undelegate` → boundary → quarantine payout.** The principal leaves the operator's aggregate at
#      the boundary and is paid, with the delegator's **accrued** reward, to the **delegator's own**
#      vault. Nothing of it goes to the operator, which is the sharpest failure mode in the primitive.
#   5. **The fork point** (`RUN_DIVERGENCE=1`, needs an unupgraded image): the same delegate deploy with
#      validator 2 running the **old binary**. Its `rho:rchain:pos` has no `delegate` arm, so it cannot
#      reproduce the block's post-state and must refuse it — a fork point at the first `delegate`
#      deploy, exactly as #193 specifies, rather than at genesis.
#
# Run from the repository root:
#
#   spec/audit/evidence/n193-delegation-live-run.sh              # phases 1-4 on the current build
#   RUN_DIVERGENCE=1 RNODE_OLD=rnode:old \
#     spec/audit/evidence/n193-delegation-live-run.sh            # …and phase 5, if `rnode:old` exists
#
# `rnode:old` is **not** built here: it is a full `cargo build --release` inside Docker (~10 min and
# ~2 GB) from a `dev`-HEAD source tree, and the decision to spend that is the operator's. To build it:
#
#   git archive --format=tar origin/dev | (mkdir -p /tmp/n193-old && tar -x -C /tmp/n193-old)
#   docker build -f docker/rnode/Dockerfile -t rnode:old /tmp/n193-old
#
# It tears the default devnet down first (`down -v`) and leaves the network up for inspection.

set -uo pipefail
cd "$(dirname "$0")/../../.."

PREFIX="${DEVNET_PREFIX:-devnet}"
BOOTSTRAP="${PREFIX}-bootstrap"
OPERATOR_NODE="${PREFIX}-validator-2"
DELEGATOR_NODE="${PREFIX}-validator-1"
OUT="spec/audit/evidence/n193-delegation-live.log.txt"
BLOCKS="spec/audit/evidence/n193-delegation-live-blocks.log.txt"
# A boundary every two blocks, so phases 3 and 4 do not wait for the devnet default of 10 000.
EPOCH_LENGTH="${EPOCH_LENGTH:-2}"

say() { echo "[n193] $*" | tee -a "$OUT"; }

# The substituted deploy files are **generated and never committed**: they carry the key this run's
# genesis happened to produce, so committing one would leave a file in `examples/` that describes a
# network nobody can start. A trap removes them however the run ends — including the `exit 1` refusals
# below, which is why it is a trap and not a line at the end.
LIVE_DELEGATE="examples/pos-delegate-live.rho"
LIVE_UNDELEGATE="examples/pos-undelegate-live.rho"
trap 'rm -f "$LIVE_DELEGATE" "$LIVE_UNDELEGATE"' EXIT
cli() { docker exec "$1" rnode --grpc-host localhost "${@:2}" 2>&1; }
pubkey_of() { docker exec "$BOOTSTRAP" cat /genesis/bonds.txt 2>/dev/null | sed -n "${1}p" | awk '{print $1}'; }
height_of() {
  cli "$1" status 2>/dev/null | sed -n 's/.*"latestBlockNumber": *\([0-9]*\).*/\1/p' | head -1
}

phase() { say ""; say "=============== $* ==============="; }

: > "$OUT"
phase "setup: 2 validators, epoch length $EPOCH_LENGTH"

say "tearing down any running devnet"
tools/devnet.sh down -v >/dev/null 2>&1 || true

DIVERGE_FLAGS=""
if [[ "${RUN_DIVERGENCE:-0}" == "1" ]]; then
  OLD="${RNODE_OLD:-rnode:old}"
  if ! docker image inspect "$OLD" >/dev/null 2>&1; then
    say "RUN_DIVERGENCE=1 but no image '$OLD' — build it first (see this file's header). Refusing."
    exit 2
  fi
  # Phase 5's subject: validator 2 runs the **unupgraded** binary. `node_image` is the per-node image
  # hook this arm added to `tools/devnet.sh` for exactly this measurement.
  export RNODE_IMAGE_${OPERATOR_NODE//-/_}="$OLD"
  say "validator 2 will run $OLD (the other nodes run \$RNODE_IMAGE)"
fi
: "${DIVERGE_FLAGS:=}"

say "starting the network"
# shellcheck disable=SC2086
tools/devnet.sh up --validators 2 --epoch-length "$EPOCH_LENGTH" --fresh $DIVERGE_FLAGS 2>&1 | tail -12 | tee -a "$OUT"

DELEGATOR_PK="$(pubkey_of 1)"
OPERATOR_PK="$(pubkey_of 2)"
if [[ -z "$DELEGATOR_PK" || -z "$OPERATOR_PK" ]]; then
  say "REFUSING: the genesis bonds file did not yield two validator keys — nothing could be measured"
  exit 1
fi
say "delegator = validator 1 [${DELEGATOR_PK:0:16}…] (the genesis-funded deployer)"
say "operator  = validator 2 [${OPERATOR_PK:0:16}…] (a key validator 1 does not control)"

phase "1. delegate 40 of validator 1's vault to validator 2's key"
# The operator's key is generated per run, so the checked-in template's placeholder is substituted
# here. `examples/pos-delegate.rho` is the call shape; this is the run.
sed "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" examples/pos-delegate.rho > "$LIVE_DELEGATE"
sed "s/<OPERATOR_PUBKEY_HEX>/$OPERATOR_PK/" examples/pos-undelegate.rho > "$LIVE_UNDELEGATE"
tools/devnet.sh deploy pos-delegate-live.rho 2>&1 | tail -4 | tee -a "$OUT"
sleep 20

say "the deploy's own reply (\`(true, Nil)\` is the op accepting):"
tools/devnet.sh query pos-delegate 2>&1 | tail -6 | tee -a "$OUT"
DELEGATE_REPLY="$(tools/devnet.sh query pos-delegate 2>&1 | tail -20)"
if ! grep -q "true" <<<"$DELEGATE_REPLY"; then
  say "REFUSING to continue: the delegate deploy did not report success. Its reply was:"
  say "$DELEGATE_REPLY"
  exit 1
fi

phase "2. the aggregate on chain — validator 2's bond 100 → 140"
# **`show-blocks` is the readable surface for a bond, and the reading is stated rather than assumed.**
# The dump prints each block's own `bonds` map, so the operator's entry is what a block carries — which
# is the number `compute_bonds` recomputes and `Validate::bonds_cache` checks against.
cli "$BOOTSTRAP" show-blocks --depth 30 > "$BLOCKS" 2>&1
LINES="$(wc -l < "$BLOCKS")"
BLOCKS_N="$(grep -c '^------------- block' "$BLOCKS" || true)"
say "the dump: $LINES lines, $BLOCKS_N blocks"
if [[ "$LINES" -lt 100 ]]; then
  say "REFUSING to claim anything from a $LINES-line dump — the command failed:"
  head -3 "$BLOCKS"
  exit 1
fi
say "stakes the dump carries (\`140\` is validator 2's aggregate; \`100\` is validator 1's alone):"
grep -oE '"stake" *: *[0-9]+' "$BLOCKS" | grep -oE '[0-9]+$' | sort -n | uniq -c | tee -a "$OUT"
AGGREGATE="$(grep -oE '"stake" *: *140' "$BLOCKS" | head -1)"
if [[ -z "$AGGREGATE" ]]; then
  say "the dump carries no 140 stake — the delegation did NOT reach the on-chain bond map. Stakes seen:"
  grep -oE '"stake" *: *[0-9]+' "$BLOCKS" | sort -u | head -10
  exit 1
fi
say "validator 2 carries 140: the delegated 40 is in the aggregate the chain agrees on"

if [[ "${RUN_DIVERGENCE:-0}" == "1" ]]; then
  # In divergence mode the network is *expected* not to agree, so the phases that read a shared state
  # are meaningless — validator 2 is refusing blocks from the delegate onward — and running them would
  # produce numbers whose meaning the reader would have to supply. Straight to the fork point.
  phase "6. the fork point: validator 2 on the old binary cannot follow the delegate deploy"
  sleep 20
  say "validator 2's refusals (it has no \`delegate\` arm, so it cannot reproduce the post-state):"
  docker logs "$OPERATOR_NODE" 2>&1 | grep -iE "InvalidStateHash|unknown method|refus" | tail -6 | tee -a "$OUT"
  say "heights — a divergence is validator 2 falling behind the other two:"
  for n in "$BOOTSTRAP" "$DELEGATOR_NODE" "$OPERATOR_NODE"; do
    say "   $n $(height_of "$n")"
  done
  say ""
  say "done; the network is still up (tools/devnet.sh down -v to remove it)"
  exit 0
fi

phase "3. a boundary pays the operator and the delegator separately"
say "the boundary's own line, from the bootstrap:"
docker logs "$BOOTSTRAP" 2>&1 | grep "\[pos\] close_block" | tail -3 | tee -a "$OUT"
say "the split, from the delegator's and the operator's own views:"
for n in "$DELEGATOR_NODE" "$OPERATOR_NODE"; do
  say "-- $n height $(height_of "$n")"
done

phase "4. undelegate, then the quarantine payout to the delegator's own vault"
tools/devnet.sh deploy pos-undelegate-live.rho 2>&1 | tail -4 | tee -a "$OUT"
sleep 20
say "the undelegate reply:"
tools/devnet.sh query pos-undelegate 2>&1 | tail -6 | tee -a "$OUT"

say "letting the boundary and the quarantine elapse (epoch length $EPOCH_LENGTH, so this is quick)"
sleep 30
cli "$BOOTSTRAP" show-blocks --depth 30 > "$BLOCKS" 2>&1
say "stakes after the undelegation (validator 2 back to 100 = the principal left its aggregate):"
grep -oE '"stake" *: *[0-9]+' "$BLOCKS" | grep -oE '[0-9]+$' | sort -n | uniq -c | tee -a "$OUT"
say "the payout, from the boundary's log — a delegation claim paid to a delegator:"
docker logs "$BOOTSTRAP" 2>&1 | grep "\[pos\] delegation" | tail -3 | tee -a "$OUT"

phase "5. both nodes' views at the end"
for n in "$BOOTSTRAP" "$DELEGATOR_NODE" "$OPERATOR_NODE"; do
  say "-- $n height $(height_of "$n")"
done

if [[ "${RUN_DIVERGENCE:-0}" == "1" ]]; then
  phase "6. the fork point: validator 2 on the old binary cannot follow the delegate deploy"
  say "validator 2's refusals (it has no \`delegate\` arm, so it cannot reproduce the post-state):"
  docker logs "$OPERATOR_NODE" 2>&1 | grep -iE "InvalidStateHash|unknown method|refus" | tail -5 | tee -a "$OUT"
  say "heights — a divergence is validator 2 falling behind the other two:"
  for n in "$BOOTSTRAP" "$DELEGATOR_NODE" "$OPERATOR_NODE"; do
    say "   $n $(height_of "$n")"
  done
fi

say ""
say "done; the network is still up (tools/devnet.sh down -v to remove it)"
say "the log this run wrote: $OUT"
