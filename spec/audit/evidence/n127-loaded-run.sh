#!/usr/bin/env bash
# #127's controlled comparison, round 2: round 1's rig with the load it was missing.
#
# The protocol is `n127-loaded-preregistration.md`, frozen before this ran; round 1 is
# `n127-directed-results.md`, and the two paragraphs below are why there is a second round at all.
#
# **What is carried over, and from where.** Round 1 ran `n117-after-fix-run.sh` unmodified on both arms and
# reached a widest merge scope of 9 chains on every attempt on both trees, with every peak 45x below the
# threshold — a null, because the ramp's input was absent. The scope the ramp was measured at needs load.
# So this script is a *composition* of two frozen rigs and invents nothing:
#
#   the measurement half   `n117-after-fix-run.sh` — the shape, the 8 GiB cap, the 3000 MiB threshold with
#                          its clean stop, the 300 s window, the 1 Hz peak sampling, the census extraction;
#   the load half          `n127-campaign-run.sh` — four bounded deploys at T+30, `stop 2` at T+120, and
#                          the deploy accounting that says how many of the four actually landed.
#
# The load is applied from **inside** the sampling loop as an absolute offset from the attempt's zero, and
# the deploys run in a background subshell: a deploy that hits its 45 s bound would otherwise suspend the
# 1 Hz sampling for up to three minutes, and a gap in a memory ramp is a missing measurement exactly where
# this run is looking.
#
# Usage: spec/audit/evidence/n127-loaded-run.sh <arm> <image-tag> [arm-tree]
#   e.g. spec/audit/evidence/n127-loaded-run.sh control rnode:control-6eacc4969 6eacc4969
set -u
cd /home/patrick/RNodeRust

ARM=${1:?usage: n127-loaded-run.sh <arm> <image-tag> [arm-tree]}
IMAGE=${2:?usage: n127-loaded-run.sh <arm> <image-tag> [arm-tree]}
TREE=${3:-$(git rev-parse --short HEAD)}

ATTEMPTS=${ATTEMPTS:-3}
THRESHOLD_MB=${THRESHOLD_MB:-3000}
CAP=${CAP:-8g}
WINDOW_S=${WINDOW_S:-300}
DEPLOY_AT=${DEPLOY_AT:-30}
DEPLOYS=${DEPLOYS:-4}
DEPLOY_TIMEOUT=${DEPLOY_TIMEOUT:-45}
KILL_AT=${KILL_AT:-120}
MIN_SCOPE=${MIN_SCOPE:-20}

[[ -f spec/audit/evidence/n127-loaded-preregistration.md ]] || {
  echo "no preregistration — the protocol must be frozen before this runs" >&2; exit 2; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || { echo "no such image: $IMAGE" >&2; exit 2; }

STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="target/n127-loaded/${ARM}-${STAMP}"
mkdir -p "$OUT"

docker tag "$IMAGE" rnode:local
{
  echo "arm=$ARM"
  echo "image=$IMAGE"
  echo "image_id=$(docker inspect rnode:local --format '{{.Id}}')"
  echo "arm_tree=$TREE"
  echo "harness_tree=$(git rev-parse --short HEAD)"
  echo "rig=n127-loaded-run.sh cap=$CAP threshold=${THRESHOLD_MB}MiB window=${WINDOW_S}s attempts=$ATTEMPTS \
deploys=$DEPLOYS at T+${DEPLOY_AT}s kill=v2 at T+${KILL_AT}s min_scope=$MIN_SCOPE"
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

declare -A CONTAINERS=([bootstrap]=devnet-bootstrap [v1]=devnet-validator-1 [v2]=devnet-validator-2)
declare -A PORTS=([bootstrap]=40403 [v1]=41403 [v2]=42403)

for attempt in $(seq 1 "$ATTEMPTS"); do
  echo "=================== attempt $attempt of $ATTEMPTS  $(date -u +%H:%M:%S) UTC ==================="
  tools/devnet.sh down >/dev/null 2>&1
  DEVNET_NODE_MEMORY="$CAP" timeout 900 \
    tools/devnet.sh up --validators 3 --stakes 100,100,50 --epoch-length 10 --fresh 2>&1 | tail -1

  up=$(docker ps --format '{{.Names}}' 2>/dev/null | grep -c '^devnet-' || true)
  if [[ "$up" -lt 3 ]]; then
    echo "  VOID: $up of 3 containers up"; tools/devnet.sh down >/dev/null 2>&1; continue
  fi
  sleep 5

  python3 spec/audit/evidence/n117-queue-depth.py "$OUT/queue-a${attempt}.tsv" &
  sampler=$!

  t0=$(date +%s)
  deployed=""; killed=""
  declare -A stopped=()
  declare -A peak=()

  for _ in $(seq 1 $((WINDOW_S + 60))); do
    now=$(( $(date +%s) - t0 ))

    # --- the load, at absolute offsets, without suspending the sampling -----------------------------
    if [[ -z "$deployed" && "$now" -ge "$DEPLOY_AT" ]]; then
      deployed=1
      (
        submitted=0; timed_out=0
        for _ in $(seq 1 "$DEPLOYS"); do
          if timeout "$DEPLOY_TIMEOUT" tools/devnet.sh deploy examples/hello.rho >/dev/null 2>&1; then
            submitted=$((submitted + 1))
          else
            timed_out=$((timed_out + 1))
          fi
        done
        printf 'submitted\ttimed_out\ttimeout_s\n%s\t%s\t%s\n' \
          "$submitted" "$timed_out" "$DEPLOY_TIMEOUT" > "$OUT/deploys-a${attempt}.txt"
      ) &
      echo "  load: $DEPLOYS bounded deploys started at T+${now}s"
    fi
    if [[ -z "$killed" && "$now" -ge "$KILL_AT" ]]; then
      killed=1
      tools/devnet.sh stop 2 >/dev/null 2>&1
      echo "  load: validator-2 stopped at T+${now}s"
    fi

    # --- the round-1 measurement -------------------------------------------------------------------
    for n in bootstrap v1 v2; do
      [[ -n "${stopped[$n]:-}" ]] && continue
      c=${CONTAINERS[$n]}
      pid=$(docker inspect "$c" --format '{{.State.Pid}}' 2>/dev/null)
      [[ -z "$pid" || "$pid" == 0 ]] && continue
      cg="/sys/fs/cgroup$(sed -n 's/^0:://p' "/proc/$pid/cgroup" 2>/dev/null)"
      anon=$(awk '/^anon /{print int($2/1048576)}' "$cg/memory.stat" 2>/dev/null)
      if [[ -n "$anon" && "$anon" -gt "${peak[$n]:-0}" ]]; then peak[$n]=$anon; fi
      if [[ -n "$anon" && "$anon" -gt "$THRESHOLD_MB" ]]; then
        echo "  $n CROSSED at anon=${anon} MiB at T+${now}s — stopping cleanly"
        docker stop -t 180 "$c" >/dev/null
        stopped[$n]=1
      fi
    done
    [[ ${#stopped[@]} -ge 3 ]] && break
    kill -0 $sampler 2>/dev/null || break
    sleep 1
  done
  wait $sampler 2>/dev/null

  echo "  peaks: bootstrap=${peak[bootstrap]:-?} v1=${peak[v1]:-?} v2=${peak[v2]:-?} MiB (threshold ${THRESHOLD_MB})"
  echo "  crossed: ${#stopped[@]} of 3"

  # The census, from the node's own log — the close condition's own currency.
  widest=0
  for n in bootstrap v1 v2; do
    docker logs "${CONTAINERS[$n]}" > "$OUT/log-${n}-a${attempt}.txt" 2>&1
    line=$(grep 'merge search' "$OUT/log-${n}-a${attempt}.txt" | tail -1)
    echo "  $n census: ${line:-<none>}"
    w=$(printf '%s' "$line" | sed -n 's/.*widest scope \([0-9]*\) chains.*/\1/p')
    [[ -n "$w" && "$w" -gt "$widest" ]] && widest=$w
    # #141's cap, read off the same run (arm C of the preregistration).
    grep -o 'index cache [0-9]* entries, [0-9]* pruned, cap 64, [0-9]* capacity evicted' \
      "$OUT/log-${n}-a${attempt}.txt" | tail -1 | sed "s/^/  $n cache: /"
  done
  echo "  widest scope: ${widest} chains (the comparison needs >= ${MIN_SCOPE})"
  if [[ "$widest" -lt "$MIN_SCOPE" ]]; then
    echo "  VOID for the census comparison: the widest scope never reached ${MIN_SCOPE} chains"
  fi

  tools/devnet.sh down >/dev/null 2>&1
  echo "  attempt $attempt done"
done

echo "finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$OUT/manifest.txt"
echo "artifacts: $OUT"
