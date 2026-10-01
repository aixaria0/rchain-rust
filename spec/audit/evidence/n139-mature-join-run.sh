#!/usr/bin/env bash
# #139, Arm D' — does a node that LFS-**syncs a mature chain** then disagree about a block's state hash?
#
# The protocol is `n139-fringe-divergence-preregistration.md`, frozen before this ran. Arm B reproduced
# the *input difference* in process (a restored block carries no fringe, and the DAG caches its state
# under the empty fringe) and Arm A proved the *consequence* (a node deriving a different fringe replays
# the same block to a different post-state, which `handle_errors` reports as `InvalidStateHash`). Neither
# is an end-to-end observation, and this arm is: the composition has to be run, not argued.
#
# **Why the rig is a reset rather than an `up`.** A node only LFS-syncs when its DAG is empty at start, so
# a joiner brought up beside a fresh devnet restores block 0 alone — and the genesis goes in through
# `insert_genesis` with the *correct* fringe, which is why this has never been observed. The experiment is
# therefore "let the chain get mature, wipe one node's store, bring it back" — `tools/devnet.sh reset`.
#
# Usage:  ATTEMPTS=3 spec/audit/evidence/n139-mature-join-run.sh
set -u
cd /home/patrick/RNodeRust

ATTEMPTS=${ATTEMPTS:-3}
CAP=${CAP:-8g}
# Past the boundary at 10 *and* at 20 before the reset, so the synced chain is genuinely mature and the
# joiner has boundaries on both sides of the join.
MATURE_HEIGHT=${MATURE_HEIGHT:-25}
# How long to watch after the joiner is back. It has to sync, then validate at least one more block, and
# the chain must cross the next boundary (30) for the close-block path to be exercised on a synced DAG.
WATCH_S=${WATCH_S:-240}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-20}

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="target/n139-mature-join/${TREE}-${STAMP}"
FLAGS="--validators 2 --stakes 1000,100 --epoch-length 10 --fresh"

mkdir -p "$OUT"
IMAGE=$(docker inspect rnode:local --format '{{.Id}}' 2>/dev/null || echo "none")

{
  echo "# tree=$TREE image=$IMAGE"
  echo "# shape: $FLAGS, cap=$CAP, mature_height=$MATURE_HEIGHT, watch=${WATCH_S}s, attempts=$ATTEMPTS"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# binary_sha256=$(docker run --rm --entrypoint sha256sum rnode:local /usr/local/bin/rnode 2>/dev/null | cut -d' ' -f1 || echo '?')"
  echo "# image_created=$(docker inspect rnode:local --format '{{.Created}}' 2>/dev/null || echo 'none')"
  echo "# rust_diff_vs_HEAD=$([ -n "$(git diff --stat HEAD -- '*.rs' 2>/dev/null)" ] && echo 'NON-EMPTY — the image may be neither' || echo 'empty — the Rust tree is HEAD')"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

height_of() { curl -s "http://localhost:$1/api/v1/status" 2>/dev/null | grep -o '"latestBlockNumber":[0-9]*' | tail -1 | cut -d: -f2; }

# **The two lines an operator can read**, and the two the acceptance row is written on. The disagreement
# line is `casper.interpreter.validate`'s (#139's instrumentation, which had to land before this run could
# name anything); the sync line says the joiner took the LFS path this arm exists to exercise.
DISAGREE='state-hash disagreement on'
SYNCED='LFS state is successfully restored'

for attempt in $(seq 1 "$ATTEMPTS"); do
  echo "=================== attempt $attempt of $ATTEMPTS  $(date -u +%H:%M:%S) UTC ==================="
  tools/devnet.sh down >/dev/null 2>&1
  DEVNET_NODE_MEMORY="$CAP" timeout 900 tools/devnet.sh up $FLAGS 2>&1 | tail -1

  if [[ "$(docker ps --format '{{.Names}}' | grep -c '^devnet-')" -lt 2 ]]; then
    echo "  VOID: fewer than 2 containers up"; continue
  fi

  # --- phase 1: mature the chain ---------------------------------------------------------------
  for _ in $(seq 1 120); do
    h=$(height_of 40403)
    [[ -n "${h:-}" && "$h" -ge "$MATURE_HEIGHT" ]] && break
    sleep 2
  done
  h=$(height_of 40403)
  echo "  chain matured to height ${h:-?} (wanted >= $MATURE_HEIGHT)"
  if [[ -z "${h:-}" || "$h" -lt "$MATURE_HEIGHT" ]]; then
    echo "  VOID: never reached a mature chain, so there is nothing for the joiner to sync"
    docker logs devnet-bootstrap > "$OUT/attempt$attempt-void-bootstrap.txt" 2>&1
    continue
  fi

  docker logs devnet-bootstrap  > "$OUT/attempt$attempt-pre-bootstrap.txt"  2>&1
  docker logs devnet-validator-1 > "$OUT/attempt$attempt-pre-validator1.txt" 2>&1

  # --- phase 2: wipe validator-1's store and bring it back -------------------------------------
  #
  # **`reset` and not `down`/`up`.** The first version of this arm took the whole network down and back
  # up, and that restarts the *bootstrap* too — which then replays its own store, so the joiner syncs a
  # chain that is still being rebuilt. Measured, 2026-10-01: the joiner logged
  # `LFS state is successfully restored` and the arm still said nothing, because what it restored was
  # the bootstrap's half-replayed chain. `reset` leaves the bootstrap running and recreates only the one
  # node, which is the precondition this arm actually needs.
  tools/devnet.sh reset 1 > "$OUT/attempt$attempt-reset.txt" 2>&1
  if [[ "$(docker ps --format '{{.Names}}' | grep -c '^devnet-')" -lt 2 ]]; then
    echo "  VOID: fewer than 2 containers up after the reset"; continue
  fi

  # --- phase 3: watch across the next boundary --------------------------------------------------
  #
  # Nothing is driven here on purpose: the devnet's autopropose is on by default, so the chain keeps
  # producing and crossing boundaries without a deploy. The joiner then does the two things this arm is
  # about — sync the mature chain, and validate what comes after it — and both happen under the load the
  # #105 observation had (blocks at the autopropose timer's rate) rather than under a synthetic storm
  # that would change the shape being measured.
  end=$(( $(date +%s) + WATCH_S ))
  while [[ $(date +%s) -lt $end ]]; do
    echo "    t=$(( end - $(date +%s) ))s  bootstrap=$(height_of 40403) validator-1=$(height_of 41403)"
    sleep "$DEPLOY_EVERY_S"
  done

  docker logs devnet-bootstrap   > "$OUT/attempt$attempt-post-bootstrap.txt"  2>&1
  docker logs devnet-validator-1 > "$OUT/attempt$attempt-post-validator1.txt" 2>&1
  echo "  heights: bootstrap=$(height_of 40403) validator-1=$(height_of 41403)"

  for n in bootstrap validator1; do
    f="$OUT/attempt$attempt-post-$n.txt"
    d=$(grep -c "$DISAGREE" "$f" || true)
    s=$(grep -c "$SYNCED" "$f" || true)
    echo "  $n: disagreement lines=$d  lfs-restored lines=$s"
    grep "$DISAGREE" "$f" | head -3 | sed 's/^/      /'
  done
done

tools/devnet.sh down >/dev/null 2>&1
echo "artifacts: $OUT"
