#!/usr/bin/env bash
# n220-silent-join — a validator **bonds and never speaks**.
#
# The one unrun shape in criterion 2, and the shape #213 is named for. `n220-join-run.sh` proves a
# *healthy* joiner works; its own caveat says it does not prove a **silent** one cannot wedge the chain.
# This is that arm.
#
# **The rig is the join rig minus the node.** `examples/join-admit.rho` funds key 3's address and
# `examples/join-bond.rho` is *signed by key 3 but sent to the bootstrap* (deploys are not gossiped), so
# neither needs validator-3 to be running — the join rig starts it with `devnet.sh reset 3` only because
# it wants to watch the newcomer produce. Here that step is **omitted on purpose**: key 3 is bonded and
# its container is never created, so a validator sits in the bond pool that speaks to nobody.
#
# The question, stated as the falsifier: **the three live validators keep finalising past the bond.** If
# they do not, a bonded-but-absent validator wedges a net that holds three quarters of the stake — which
# is Law 52b's `Void` shape, and the failure would be the rule rather than the rig.
#
# Bounded: a timeout is a FAIL.
#
# Usage:  spec/audit/evidence/n220-silent-join-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
EPOCH=${EPOCH:-10}
OBSERVE_S=${OBSERVE_S:-420}
READ_S=${READ_S:-60}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-12}
GENESIS_PRIV=a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76
K3_PRIV=f484e1c24de228819213904c7da8999a94cfdd20e0c70f47a37ae0b6b8c9feb7
K3_PUB=04ea3ce04abbe780205eb5f94a82889600630ca73bd31e7a336efb6700e24c515b900dea427d3e0c0fc74a8a6d5e173331433f00df2de4b7f953c2922297c2c08e
PORTS=(40403 41403 42403)

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${OUT_ROOT:-target/n220-silent}/${TREE}-${STAMP}
mkdir -p "$OUT"
{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length $EPOCH --no-autopropose --propose-on-deploy"
  echo "# key 3 is admitted and bonded; its container is NEVER created (the silent joiner)"
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
# The bonds map a block carries, and whether it names key 3: `<blockNumber> <names-k3 0|1> <deployCount>`.
tip_blocks() {
  curl -fsS --max-time 5 "http://localhost:40403/api/blocks/${1:-30}" 2>/dev/null | python3 -c '
import json,sys
k3=sys.argv[1]
for b in json.load(sys.stdin):
    print(b["blockNumber"], int(any(x["validator"]==k3 for x in b["bonds"])), b["deployCount"])
' "$K3_PUB"
}
sample() { echo "$(date -u +%H:%M:%S) h=$(height 40403)/$(height 41403)/$(height 42403) fin=$(finalised)" | tee -a "$OUT/series.txt"; }
deploy_as() {
  timeout 45 docker exec "$1" rnode --grpc-host localhost deploy --phlo-limit 1000000 --phlo-price 1 \
    --private-key "$2" --shard-id /root --valid-after-block-number "$(height 40403)" "/contracts/$3" 2>&1 \
    | sed -n 's/^DeployId is: *//p'
}
deploy_status() {
  local p
  for p in "${PORTS[@]}"; do
    curl -fsS --max-time 3 "http://localhost:$p/api/v1/deploy-status/$1" 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin); k=next(iter(d)) if isinstance(d,dict) and d else ""
v=d.get(k,{}) if k else {}
if k=="ProcessedWithSuccess": print("ok", v["block"]["blockNumber"])
elif k=="ProcessedWithError": print("error", v.get("deployError","?").replace(" ","_"))
else: sys.exit(1)
' 2>/dev/null && return
  done
  echo pending
}

tools/devnet.sh down >/dev/null 2>&1
DEVNET_NODE_MEMORY="$CAP" timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
  --epoch-length "$EPOCH" --no-autopropose --propose-on-deploy 2>&1 | tail -1
for _ in $(seq 90); do [[ "$(height 42403)" -gt 0 ]] 2>/dev/null && break; sleep 2; done
deploy_as devnet-bootstrap "$GENESIS_PRIV" hello.rho >/dev/null; sleep 15; sample
echo "== three live validators, no fourth container: $(docker ps --format '{{.Names}}' | grep -c '^devnet-' || true) up"

echo "== admit and bond key 3, and never start it"
a=$(deploy_as devnet-bootstrap "$GENESIS_PRIV" join-admit.rho)
sleep 15; echo "  admit: $(deploy_status "$a")" | tee -a "$OUT/witness.txt"
b=$(deploy_as devnet-bootstrap "$K3_PRIV" join-bond.rho)
sleep 15; echo "  bond:  $(deploy_status "$b")" | tee -a "$OUT/witness.txt"
echo "  containers after the bond: $(docker ps --format '{{.Names}}' | grep -c '^devnet-' || true) (a fourth must NOT exist)"

# S1 — the bond lands: some block's bonds name key 3.
bonded_at=""; t_end=$(( $(date +%s) + 120 ))
while (( $(date +%s) < t_end )); do
  tip_blocks 30 > "$OUT/tip.txt"
  bonded_at=$(awk '$2==1{print $1}' "$OUT/tip.txt" | sort -n | head -1)
  [[ -n "$bonded_at" ]] && break
  sleep 5
done
if [[ -n "$bonded_at" ]]; then
  echo "  S1 PASS: the bond landed — a block's bonds name the silent validator from block $bonded_at" | tee -a "$OUT/witness.txt"
else
  echo "  S1 FAIL: no block's bonds name key 3 — the bond never took effect" | tee -a "$OUT/witness.txt"
fi

# S2 — the falsifier. The three live validators keep finalising past the bond.
base=$(finalised); [[ "$base" == none ]] && base=0
echo "  finality at the bond: $base" >> "$OUT/witness.txt"
advanced=""; t_end=$(( $(date +%s) + OBSERVE_S )); i=0
targets=(devnet-bootstrap devnet-validator-1 devnet-validator-2)
while (( $(date +%s) < t_end )); do
  deploy_as "${targets[i % 3]}" "$GENESIS_PRIV" hello.rho >/dev/null; i=$((i + 1))
  sleep "$DEPLOY_EVERY_S"; sample
  f=$(finalised)
  if [[ "$f" != none ]] && (( f > base )); then advanced="$f"; break; fi
done
if [[ -n "$advanced" ]]; then
  echo "  S2 PASS: the three live validators finalise past the bond — $base -> $advanced" | tee -a "$OUT/witness.txt"
else
  echo "  S2 FAIL: finality did not advance past the bond ($base -> $(finalised)) with a bonded validator that never speaks" | tee -a "$OUT/witness.txt"
fi

h0=$(height 40403); sleep "$READ_S"; h1=$(height 40403); sample
echo "  quiet read: height $h0 -> $h1 over ${READ_S}s" | tee -a "$OUT/witness.txt"

tip_blocks 40 > "$OUT/tip.txt"
awk '{print $2}' "$OUT/tip.txt" | sort | uniq -c > "$OUT/bonds-name-k3.txt" 2>/dev/null || true
for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
  docker logs "$c" > "$OUT/$c.log" 2>&1
  grep -o 'finality did not advance at tip [0-9]*: .*' "$OUT/$c.log" | sort -u >> "$OUT/stall-lines.txt" || true
done
tools/devnet.sh down >/dev/null 2>&1
echo
echo "=== witnesses ==="; cat "$OUT/witness.txt"
echo "=== the gate's own reason ==="; sort -u "$OUT/stall-lines.txt" 2>/dev/null | head -5
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
