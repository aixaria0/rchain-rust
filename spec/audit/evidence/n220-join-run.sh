#!/usr/bin/env bash
# n220 — a validator joins a running net with no autopropose, live.
#
# The question #214 leaves open after #219: on a `--no-autopropose --propose-on-deploy` net, can a new key
# be admitted, bonded and activated at an epoch boundary, and does the four-validator net then finalise —
# including a deploy sent to the newcomer — and go quiet?
#
#   (J1) the bond lands: a block's `bonds` names the newcomer.
#   (J2) the newcomer produces a block after it is active.
#   (J3) a deploy sent to the newcomer finalises.
#   (J4) finality advances past the activation boundary, and the chain is quiet afterwards.
#
# The newcomer is `tools/devnet.sh`'s key 3, started with `reset 3` (synced, unbonded), funded and trusted by
# the genesis deployer (`examples/join-admit.rho`), and bonding itself (`examples/join-bond.rho`, signed by
# key 3 and sent to the bootstrap, because deploys are not gossiped). Bounded: a timeout is a FAIL.
#
# Usage:  spec/audit/evidence/n220-join-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
EPOCH=${EPOCH:-10}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-12}
ACTIVATE_BUDGET_S=${ACTIVATE_BUDGET_S:-420}
FINAL_WAIT_S=${FINAL_WAIT_S:-120}
READ_S=${READ_S:-90}
DEPLOY_FILE=${DEPLOY_FILE:-examples/hello.rho}
K3_PRIV=f484e1c24de228819213904c7da8999a94cfdd20e0c70f47a37ae0b6b8c9feb7
K3_PUB=04ea3ce04abbe780205eb5f94a82889600630ca73bd31e7a336efb6700e24c515b900dea427d3e0c0fc74a8a6d5e173331433f00df2de4b7f953c2922297c2c08e
PORTS=(40403 41403 42403 43403)

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${OUT_ROOT:-target/n220-join}/${TREE}-${STAMP}
mkdir -p "$OUT"
{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length $EPOCH --no-autopropose --propose-on-deploy; newcomer key 3 bonds 50"
  echo "# image=$(docker inspect rnode:local --format '{{.Id}} {{.Created}}' 2>/dev/null || echo none)"
  echo "# started $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$OUT/manifest.txt"
cat "$OUT/manifest.txt"

height() { curl -fsS --max-time 3 "http://localhost:$1/api/v1/status" 2>/dev/null | sed -n 's/.*"latestBlockNumber":\([0-9]*\).*/\1/p'; }
finalised() {
  local best="" v p
  for p in "${PORTS[@]}"; do
    v=$(curl -fsS --max-time 3 "http://localhost:$p/api/last-finalized-block" 2>/dev/null \
        | sed -n 's/.*"blockNumber":\([0-9]*\).*/\1/p')
    if [[ -n "$v" ]] && { [[ -z "$best" ]] || (( v > best )); }; then best="$v"; fi
  done
  echo "${best:-none}"
}
# Blocks at the tip of the bootstrap's view: `<height> <sender prefix> <bonds names newcomer?>`.
tip_blocks() {
  curl -fsS --max-time 5 "http://localhost:40403/api/blocks/${1:-20}" 2>/dev/null | python3 -c '
import json,sys
k3=sys.argv[1]
for b in json.load(sys.stdin):
    print(b["blockNumber"], b["sender"][:8], int(any(x["validator"]==k3 for x in b["bonds"])), b["deployCount"])
' "$K3_PUB"
}
sample() { echo "$(date -u +%H:%M:%S) h=$(height 40403)/$(height 41403)/$(height 42403)/$(height 43403) fin=$(finalised)" | tee -a "$OUT/series.txt"; }
# deploy_as <container> <private key> <contract> — echoes the DeployId, or nothing on failure.
deploy_as() {
  timeout 45 docker exec "$1" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$2" --shard-id /root --valid-after-block-number "$(height 40403)" "/contracts/$3" 2>&1 \
    | sed -n 's/^DeployId is: *//p'
}
deploy_to() { deploy_as "$1" a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76 "$(basename "$DEPLOY_FILE")" >/dev/null; }
# The deploy's processed status, from any node that has it: `ok <block hash> <number>`, `error <reason>`, or `pending`.
deploy_status() {
  local p
  for p in "${PORTS[@]}"; do
    curl -fsS --max-time 3 "http://localhost:$p/api/v1/deploy-status/$1" 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin); k=next(iter(d)) if isinstance(d,dict) and d else ""
v=d.get(k,{}) if k else {}
if k=="ProcessedWithSuccess": print("ok", v["block"]["blockHash"], v["block"]["blockNumber"])
elif k=="ProcessedWithError": print("error", v.get("deployError","?").replace(" ","_"), v["block"]["blockNumber"])
else: sys.exit(1)
' 2>/dev/null && return
  done
  echo pending
}
is_finalised() { curl -fsS --max-time 3 "http://localhost:40403/api/is-finalized/$1" 2>/dev/null | grep -q true; }

tools/devnet.sh down >/dev/null 2>&1
DEVNET_NODE_MEMORY="$CAP" timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
  --epoch-length "$EPOCH" --no-autopropose --propose-on-deploy 2>&1 | tail -1
for _ in $(seq 90); do [[ "$(height 42403)" -gt 0 ]] 2>/dev/null && break; sleep 2; done
# The genesis alone is height 0; one deploy gets every validator past it.
deploy_to devnet-bootstrap; sleep 15; sample

echo "== newcomer: reset 3 (sync, unbonded)"
tools/devnet.sh reset 3 >"$OUT/reset.txt" 2>&1
for _ in $(seq 90); do h=$(height 43403); [[ -n "$h" && "$h" -gt 0 ]] && break; sleep 2; done
sample
echo "== admit (fund + trust) and bond"
admit=$(deploy_as devnet-bootstrap a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76 join-admit.rho)
sleep 15; echo "  admit: $(deploy_status "$admit")" | tee -a "$OUT/admission.txt"
# Signed by key 3 and sent to the bootstrap: deploys are not gossiped, and key 3 is not yet a proposer.
bond=$(deploy_as devnet-bootstrap "$K3_PRIV" join-bond.rho)
sleep 15; echo "  bond:  $(deploy_status "$bond")" | tee -a "$OUT/admission.txt"
docker logs devnet-bootstrap 2>&1 | grep -E '^\[pos\] (ok|refused)' > "$OUT/pos-lines.txt"
cat "$OUT/pos-lines.txt"
sample

# Drive the chain with rotating deploys until the newcomer has produced a block, or the budget runs out.
echo "== driving to the boundary"
bonded_at="" active_block="" t_end=$(( $(date +%s) + ACTIVATE_BUDGET_S )) i=0
targets=(devnet-bootstrap devnet-validator-1 devnet-validator-2)
while (( $(date +%s) < t_end )); do
  deploy_to "${targets[i % 3]}"; i=$((i + 1)); sleep "$DEPLOY_EVERY_S"; sample
  tip_blocks 30 > "$OUT/tip.txt"
  [[ -z "$bonded_at" ]] && bonded_at=$(awk '$3==1{print $1}' "$OUT/tip.txt" | sort -n | head -1)
  active_block=$(awk '$2=="04ea3ce0"{print $1}' "$OUT/tip.txt" | sort -n | head -1)
  [[ -n "$active_block" ]] && break
done
[[ -n "$bonded_at" ]] && echo "  J1 PASS: bonds name the newcomer from block $bonded_at" \
  || echo "  J1 FAIL: no block's bonds name the newcomer"
[[ -n "$active_block" ]] && echo "  J2 PASS: the newcomer produced block $active_block" \
  || echo "  J2 FAIL: the newcomer never produced a block within ${ACTIVATE_BUDGET_S}s"

echo "== deploy to the newcomer"
floor=$(finalised); [[ "$floor" == none ]] && floor=0
dep=$(deploy_as devnet-validator-3 a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76 "$(basename "$DEPLOY_FILE")")
[[ -n "$dep" ]] && echo "  deployed to the newcomer" || echo "  deploy to the newcomer FAILED"
# The witness is the deploy's own block being finalised, never a height comparison.
t_end=$(( $(date +%s) + FINAL_WAIT_S )) j3="" st=""
while (( $(date +%s) < t_end )); do
  sleep 3; st=$(deploy_status "$dep")
  if [[ "$st" == ok* ]] && is_finalised "$(echo "$st" | cut -d' ' -f2)"; then j3=$st; break; fi
done
sample
[[ -n "$j3" ]] && echo "  J3 PASS: the newcomer's deploy is in a finalised block ($j3)" \
  || echo "  J3 FAIL: the newcomer's deploy is not finalised (status: ${st:-none})"
# `bonded_at` is the first block whose bonds name the newcomer: the activation boundary itself.
boundary=${bonded_at:-999999}
f=$(finalised)
[[ "$f" != none ]] && (( f >= boundary )) && echo "  J4a PASS: finalised $f, past the activation boundary $boundary" \
  || echo "  J4a FAIL: finalised $f, not past the activation boundary $boundary"

echo "== quiet read (${READ_S}s)"
h0=$(height 40403); sleep "$READ_S"; h1=$(height 40403); sample
(( h1 - h0 <= 1 )) && echo "  J4b PASS: quiet — height $h0 -> $h1 over ${READ_S}s" \
  || echo "  J4b FAIL: not quiet — height $h0 -> $h1 over ${READ_S}s"

tip_blocks 50 > "$OUT/tip.txt"
awk '{print $2}' "$OUT/tip.txt" | sort | uniq -c > "$OUT/senders.txt"; cat "$OUT/senders.txt"
for c in devnet-bootstrap devnet-validator-1 devnet-validator-2 devnet-validator-3; do
  docker logs "$c" > "$OUT/$c.log" 2>&1
  grep -o 'round gate escaped[^"]*' "$OUT/$c.log" | sed "s/^/$c /" >> "$OUT/escape-lines.txt" || true
done
gzip -f "$OUT"/devnet-*.log
tools/devnet.sh down >/dev/null 2>&1
echo "run root: $OUT"
