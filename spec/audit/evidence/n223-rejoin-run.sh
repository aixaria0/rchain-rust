#!/usr/bin/env bash
# n223 — a validator that leaves and returns rejoins: the #223 close condition, on a live net.
#
# #223 measured on the testnet: a validator killed and restarted came back, meshed, retrieved blocks, and
# stayed at the height it died at, with two `block summary failed: missing justification` refusals and no
# retry. The close condition: kill one of three, let the survivors move on, restart it, and it reaches the
# tip and takes part — **a deploy submitted to it afterwards finalises**.
#
#   (R1) the returner reaches the survivors' tip.
#   (R2) a deploy sent to the returner afterwards is in a finalised block (its own deploy status).
#   (R3) no `missing justification` refusal on the returner.
#
# The absence is long on purpose: #213's arm restarted 18 s after the kill and never left the returner a
# backlog; #223's node was away for ~13 heights. Bounded: a timeout is a FAIL.
#
# Usage:  spec/audit/evidence/n223-rejoin-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
STOP_NODE=${STOP_NODE:-2}           # the 50-stake validator: the survivors keep 80 %
ABSENT_DEPLOYS=${ABSENT_DEPLOYS:-6}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-12}
CATCHUP_S=${CATCHUP_S:-180}
FINAL_WAIT_S=${FINAL_WAIT_S:-120}
DEPLOYER=a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76
PORTS=(40403 41403 42403)
RET_PORT=${PORTS[$STOP_NODE]}
RET=devnet-validator-$STOP_NODE

TREE=$(git rev-parse --short HEAD)
OUT=${OUT_ROOT:-target/n223-rejoin}/${TREE}-$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$OUT"
{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy; stop $RET for $ABSENT_DEPLOYS deploys"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

height() { curl -fsS --max-time 3 "http://localhost:$1/api/v1/status" 2>/dev/null | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'; }
finalised() { curl -fsS --max-time 3 "http://localhost:40403/api/last-finalized-block" 2>/dev/null | sed -n 's/.*"blockNumber":\([0-9]*\).*/\1/p'; }
sample() { echo "$(date -u +%H:%M:%S) h=$(height 40403)/$(height 41403)/$(height 42403) fin=$(finalised) $*" | tee -a "$OUT/series.txt"; }
deploy_as() { # deploy_as <container> — echoes the DeployId
  timeout 45 docker exec "$1" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$DEPLOYER" --shard-id /root --valid-after-block-number "$(height 40403)" \
    /contracts/hello.rho 2>&1 | sed -n 's/^DeployId is: *//p'
}
deploy_block() { # the block hash holding a processed deploy, or nothing
  curl -fsS --max-time 3 "http://localhost:40403/api/v1/deploy-status/$1" 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin)
v=d.get("ProcessedWithSuccess") if isinstance(d,dict) else None
if v: print(v["block"]["blockHash"])' 2>/dev/null
}
is_finalised() { curl -fsS --max-time 3 "http://localhost:40403/api/is-finalized/$1" 2>/dev/null | grep -q true; }

tools/devnet.sh down >/dev/null 2>&1
DEVNET_NODE_MEMORY="$CAP" timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
  --epoch-length 10 --no-autopropose --propose-on-deploy 2>&1 | tail -1
for _ in $(seq 90); do [[ "$(height 42403)" -gt 0 ]] 2>/dev/null && break; sleep 2; done
deploy_as devnet-bootstrap >/dev/null; sleep 15; sample "baseline"

tools/devnet.sh stop "$STOP_NODE" >/dev/null 2>&1; sample "stopped $RET"
for i in $(seq "$ABSENT_DEPLOYS"); do
  deploy_as devnet-bootstrap >/dev/null; sleep "$DEPLOY_EVERY_S"; sample "absent deploy $i"
done
tip_at_return=$(height 40403)

tools/devnet.sh start "$STOP_NODE" >/dev/null 2>&1; sample "restarted $RET (survivors at $tip_at_return)"
t_end=$(( $(date +%s) + CATCHUP_S )) r1=""
while (( $(date +%s) < t_end )); do
  sleep 3; h=$(height "$RET_PORT")
  [[ -n "$h" ]] && (( h >= tip_at_return )) && { r1=$h; break; }
done
sample "catch-up"
[[ -n "$r1" ]] && echo "  R1 PASS: $RET reached height $r1 (survivors were at $tip_at_return)" \
  || echo "  R1 FAIL: $RET at $(height "$RET_PORT"), survivors at $tip_at_return, after ${CATCHUP_S}s"

dep=$(deploy_as "$RET")
t_end=$(( $(date +%s) + FINAL_WAIT_S )) r2=""
while [[ -n "$dep" ]] && (( $(date +%s) < t_end )); do
  sleep 3; b=$(deploy_block "$dep")
  [[ -n "$b" ]] && is_finalised "$b" && { r2=$b; break; }
done
sample "after the returner's deploy"
[[ -n "$r2" ]] && echo "  R2 PASS: the deploy sent to $RET is in finalised block ${r2:0:8}" \
  || echo "  R2 FAIL: the deploy sent to $RET is not finalised (id ${dep:0:12})"

docker logs "$RET" > "$OUT/returner.log" 2>&1
n=$(grep -c 'missing justification' "$OUT/returner.log")
(( n == 0 )) && echo "  R3 PASS: no missing-justification refusals on $RET" \
  || { echo "  R3 FAIL: $n missing-justification refusal(s) on $RET"; grep 'missing justification' "$OUT/returner.log" | head -4 | tee "$OUT/refusals.txt"; }
curl -fsS --max-time 10 http://localhost:40403/api/blocks/50 2>/dev/null | python3 -c '
import json,sys
for b in sorted(json.load(sys.stdin), key=lambda b: (b["blockNumber"], b["sender"])):
    print(b["blockNumber"], b["sender"][:8], b["seqNum"], b["deployCount"], b["blockHash"][:8], " ".join(j[:8] for j in b["justifications"]))
' > "$OUT/blocks.txt"
for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do docker logs "$c" > "$OUT/$c.log" 2>&1; done
grep -h 'failed validation\|processing error' "$OUT"/devnet-*.log | sort | uniq -c | sort -rn | head -8 | tee "$OUT/refused.txt"
gzip -f "$OUT"/*.log
[[ -n "${KEEP_UP:-}" ]] || tools/devnet.sh down >/dev/null 2>&1
echo "run root: $OUT"
