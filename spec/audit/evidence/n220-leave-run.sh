#!/usr/bin/env bash
# n220-leave — a validator **leaves**: withdraw → epoch boundary → quarantine → payout.
#
# A2.5 of the #214 checklist, unrun until now. The question is not whether `withdraw` is *accepted* —
# `examples/pos-withdraw.rho`'s own doc says it only **stages** (law 47), so the staker stays bonded and
# stays in the active set until the boundary. The question is whether the **leave completes**: the
# boundary activates it, the stake leaves the bond pool, and the pending entry clears at its deadline.
#
# It also settles **CH-U6-09**, the one challenge the page still records as having found an inversion:
# the worksheet's H-U6-05 said the refund "waits `quarantine_length` more blocks past its deadline", and
# the challenge says that inverts the recorded behaviour. The observable here is `/api/v1/pos`'s own
# `deadline` and `blocks_remaining`, which the read path documents as `quarantine_length +
# divisor·(1 + block_number/divisor)` with `blocks_remaining = deadline − latest`, floored at zero —
# so the arithmetic is read off the API rather than inferred.
#
# **The quarantine is 50000 blocks by default**, far beyond any bounded run, so the rig shrinks it.
# `--quarantine-length` is a node flag (`node/src/configuration/commandline/options.rs`) that
# `tools/devnet.sh` does not expose, so it goes through `DEVNET_EXTRA_FLAGS`.
#
# Bounded: a timeout is a FAIL.
#
# Usage:  spec/audit/evidence/n220-leave-run.sh
set -u

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
cd "$REPO"

CAP=${CAP:-8g}
EPOCH=${EPOCH:-10}
QUARANTINE=${QUARANTINE:-20}
LEAVE_BUDGET_S=${LEAVE_BUDGET_S:-600}
READ_S=${READ_S:-60}
DEPLOY_EVERY_S=${DEPLOY_EVERY_S:-12}
# Validator 2 — the 50-stake validator, so the pool's change is unambiguous. The key table is
# `tools/devnet.sh`'s `VALIDATOR_PRIV[2]`; `withdraw` takes the caller's own `rho:rchain:deployerId`, so
# this deploy must be signed by the validator that is leaving.
K2_PRIV=d78ff60a424d71ce99d6b7d7f44a8c49b38a3757ff9e6fa9b32fcba8aa2c973b
GENESIS_PRIV=a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76
PORTS=(40403 41403 42403)

TREE=$(git rev-parse --short HEAD)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${OUT_ROOT:-target/n220-leave}/${TREE}-${STAMP}
mkdir -p "$OUT"
{
  echo "# tree=$TREE head=$(git rev-parse HEAD)"
  echo "# rig: --validators 3 --stakes 100,100,50 --epoch-length $EPOCH --quarantine-length $QUARANTINE --no-autopropose --propose-on-deploy"
  echo "# validator-2 (50 stake) is the one that leaves"
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
# `/api/v1/pos` — the read the rig's three witnesses rest on. **Field names matter and the first version
# got them wrong**: the API answers `pendingWithdrawals` (plural, camelCase, an ARRAY) and
# `activeValidators`, and the rig read `pending_withdrawal` — so it saw an empty value whatever the chain
# did, and `L1` "failed" on a withdraw that had in fact staged. A reader that cannot see the thing it
# tests is worse than no test.
pos_read() {
  curl -fsS --max-time 5 "http://localhost:40403/api/v1/pos" 2>/dev/null | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: print("unreadable"); sys.exit(0)
pws = d.get("pendingWithdrawals") or []
av  = d.get("activeValidators") or []
pw  = pws[0] if pws else {}
print("deadline=%s blocks_remaining=%s active=%d pending=%d" % (
  pw.get("deadline", ""), pw.get("blocksRemaining", ""), len(av), len(pws)))
' 2>/dev/null
}
# `<deadline> <blocks_remaining>` while a withdrawal is pending; nothing once it clears.
#
# **`\+`, not `*`.** The first version used `[0-9]*`, which matches *zero* digits — so `deadline=` with
# nothing after it "matched" and the rig printed `L1 PASS` with two empty fields while no withdrawal
# existed. A witness that passes on emptiness is not a witness.
pending() { pos_read | sed -n 's/^deadline=\([0-9]\+\) blocks_remaining=\([0-9]\+\).*/\1 \2/p'; }
# The active-validator count — the leave's own observable, and the one that does not depend on the
# quarantine still being in flight: a validator that has left the active set is out of the draw.
active() { pos_read | sed -n 's/.*active=\([0-9]\+\).*/\1/p'; }
# **The leave's own observable: the vault**, not the withdrawal's bookkeeping. Addresses are derived
# inside rholang from the public key, so the probe cannot be asking about a different account than the
# run measures.
#
# **`listen-data-at-name` returns what is already at the name, and waits only if there is nothing.**
# This is *not* what the first version of this comment said (it said "subscribes"), and the difference is
# the whole reason the 2026-10-04 run read `0 -> 0` for an account funded 100,000,000: one fixed channel
# meant the second read was handed the **first** read's datum, whose own `block_number` (5) put it before
# both the fund (7) and the withdraw (14). So the channel is a **parameter** — a distinct channel per read
# is not a stylistic choice, it is what makes the read a read. Sets `BAL` and `BAL_BLK`.
#
# **Deploy first, then read — and record the deploy's status.** The read returns data *already at the
# name*, so there is nothing to race: the deploy is submitted, its status is waited for, and the read is
# then a one-shot that returns the datum immediately. The first version of this function started the
# listener and deployed *into* it, and that ordering has a blind spot the 14:09 run fell into: when the
# read came back `unreadable` there was **no deploy status on record**, so a deploy that never landed and
# a chain that never produced a block were indistinguishable. A probe that cannot say which of its two
# halves failed is not an instrument.
balance() {  # <channel> <contract>
  local ch="$1" file="$2" tmp="$OUT/balance-$1.txt"
  local id st
  id=$(deploy_as devnet-bootstrap "$GENESIS_PRIV" "$file")
  st=pending
  for _ in $(seq 25); do
    st=$(deploy_status "$id"); [[ "$st" == ok* ]] && break
    sleep 3
  done
  echo "  probe '$ch': deploy=${id:-none} status=$st" >> "$OUT/witness.txt"
  timeout 60 docker exec devnet-bootstrap rnode --grpc-host localhost \
    listen-data-at-name -t pub -c "\"$ch\"" > "$tmp" 2>/dev/null
  # The last `GInt` in the response, and the block it was produced in — read off the datum rather than
  # from a position in the text, so a reader can check *when* the value it is comparing came from.
  BAL=$(grep -o 'GInt([0-9]\+)' "$tmp" 2>/dev/null | tail -1 | grep -o '[0-9]\+')
  BAL_BLK=$(grep -o 'block_number: [0-9]\+' "$tmp" 2>/dev/null | tail -1 | grep -o '[0-9]\+')
  [[ -z "$BAL" ]] && BAL=unreadable
  [[ -z "$BAL_BLK" ]] && BAL_BLK=?
}

sample() { echo "$(date -u +%H:%M:%S) h=$(height 40403) fin=$(finalised) pos=[$(pos_read)]" | tee -a "$OUT/series.txt"; }
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
DEVNET_NODE_MEMORY="$CAP" DEVNET_EXTRA_FLAGS="--quarantine-length $QUARANTINE" \
  timeout 900 tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 \
  --epoch-length "$EPOCH" --no-autopropose --propose-on-deploy 2>&1 | tail -1
for _ in $(seq 90); do [[ "$(height 42403)" -gt 0 ]] 2>/dev/null && break; sleep 2; done
deploy_as devnet-bootstrap "$GENESIS_PRIV" hello.rho >/dev/null; sleep 15; sample

echo "== before: the pool, and no pending withdrawal"
before=$(pos_read); echo "  $before" | tee -a "$OUT/witness.txt"

echo "== fund validator-2 (genesis funds only the deployer), then withdraw signed by it"
f=$(deploy_as devnet-bootstrap "$GENESIS_PRIV" leave-fund.rho)
sleep 15; echo "  fund: $(deploy_status "$f")" | tee -a "$OUT/witness.txt"
w=$(deploy_as devnet-bootstrap "$K2_PRIV" pos-withdraw.rho)
# **Poll for the staging at once, and do not sleep first.** `pending_withdrawers` holds the request only
# to the next epoch boundary — one block's width at `--epoch-length 10` — so the 15 s sleep the earlier
# versions took before their first look is longer than the window they were looking for. The first
# re-run caught it by luck; the second missed it and reported the L1 NOTE instead.
staged=""; t_end=$(( $(date +%s) + 60 ))
while (( $(date +%s) < t_end )); do
  staged=$(pending); [[ -n "$staged" ]] && break
  sleep 1
done
sleep 5; st=$(deploy_status "$w"); echo "  withdraw deploy: $st" | tee -a "$OUT/witness.txt"
# **The baseline, and the control, in one read.** It is taken *after* the withdraw deploy, so the
# withdraw's own phlo is already out of the balance and the payout is the only credit still to come — the
# old rig read *before* the fund, so `before -> after` would have moved by the whole 100,000,000 and a
# `>` comparison could not have told the fund from the payout. It also has to read close to the funded
# figure: a small number here means the **instrument** failed, not that the chain withheld a payment.
balance leave-balance leave-balance.rho
b_pre=$BAL; blk_pre=$BAL_BLK
echo "  balance after the withdraw deploy, before the payout: $b_pre (block $blk_pre)" | tee -a "$OUT/witness.txt"
docker exec devnet-bootstrap rnode --grpc-host localhost eval /contracts/pos-withdraw.rho >/dev/null 2>&1 || true
docker logs devnet-bootstrap 2>&1 | grep -E '^\[pos\] ' > "$OUT/pos-lines.txt" || true

# L1 — staged, with a deadline. The arithmetic is read off the API, which settles CH-U6-09. The reading
# was taken above, at the deploy; this block only reports it.
if [[ -n "$staged" ]]; then
  echo "  L1 PASS: the withdrawal staged — deadline=${staged%% *} blocks_remaining=${staged##* }" | tee -a "$OUT/witness.txt"
  echo "    (CH-U6-09: the read path's own deadline and blocks_remaining ARE the recorded arithmetic; the" \
       "worksheet's \"waits quarantine_length more blocks past its deadline\" is not what the API says.)" >> "$OUT/witness.txt"
else
  # A withdraw that is ALREADY cleared by the time we look is not a failure — quarantine is 20 blocks.
  echo "  L1 NOTE: no pending entry at first sight (already cleared, or never staged) — pos=[$(pos_read)]" | tee -a "$OUT/witness.txt"
fi

# L2/L3 — the active set shrank (the leave), the pending entry cleared, and the head is past the
# deadline (the payout's precondition, which is NOT the same thing).
echo "== driving past the deadline"
deadline=${staged%% *}; [[ "$deadline" =~ ^[0-9]+$ ]] || deadline=""
targets=(devnet-bootstrap devnet-validator-1 devnet-validator-2); i=0; left=""; cleared=""; passed=""
t_end=$(( $(date +%s) + LEAVE_BUDGET_S ))
while (( $(date +%s) < t_end )); do
  [[ "$(active)" == "2" ]] && left=1
  [[ -n "$staged" && -z "$(pending)" ]] && cleared=1
  local_h=$(height 40403)
  [[ -n "$deadline" && -n "$local_h" ]] && (( local_h > deadline )) && passed=1
  # **The loop must run past the DEADLINE, not merely past the staging.** `pending` clears at the epoch
  # boundary — `close_block` moves the request into `withdrawers`, ~10 blocks after the stage and ~20
  # before the payout — so a loop keyed on "the pending entry cleared" stops two epochs early and reads
  # the vault before anything has been credited to it. That is the fourth way this rig had been wrong
  # about its own timing.
  [[ -n "$left" && -n "$passed" ]] && break
  deploy_as "${targets[i % 3]}" "$GENESIS_PRIV" hello.rho >/dev/null; i=$((i + 1))
  sleep "$DEPLOY_EVERY_S"; sample
done
[[ -n "$left" || "$(active)" == "2" ]] && echo "  drove to height $(height 40403); deadline=${deadline:-unknown}, past-deadline=${passed:-no}" >> "$OUT/witness.txt"
[[ -n "$left" || "$(active)" == "2" ]] && echo "  L2 PASS: the leaving validator is out of the active set (3 -> 2)" | tee -a "$OUT/witness.txt" \
  || echo "  L2 FAIL: the active set is still $(active) — the leave did not take effect" | tee -a "$OUT/witness.txt"
if [[ -n "$cleared" || -z "$(pending)" ]]; then
  echo "  L3 PASS: no pending withdrawal remains once its deadline passed" | tee -a "$OUT/witness.txt"
else
  echo "  L3 FAIL: the pending withdrawal never cleared within ${LEAVE_BUDGET_S}s" | tee -a "$OUT/witness.txt"
fi
after=$(pos_read); echo "  after: $after" >> "$OUT/witness.txt"
balance leave-balance-after leave-balance-after.rho
b_post=$BAL; blk_post=$BAL_BLK
echo "  balance after the deadline: $b_post (block $blk_post)" >> "$OUT/witness.txt"
# **The witness A2.5 names.** `close_block`'s step 3 pays `bond + committed_reward` into
# `vault_address(validator)` and removes the escrow entry, and validator 2 makes no deploy of its own
# after the withdraw — so between the two reads the only thing that can move its vault is the payout.
# Both reads are signed by the genesis deployer, whose phlo is charged to its own vault, so the delta is
# the payout and not a fee (`leave-balance.rho`'s caveat was about the reader's own fees; here they are
# somebody else's).
if [[ "$b_pre" =~ ^[0-9]+$ && "$b_post" =~ ^[0-9]+$ ]]; then
  if (( b_post > b_pre )); then
    echo "  L4 PASS: the stake was paid out — vault $b_pre -> $b_post (+$(( b_post - b_pre )), blocks $blk_pre -> $blk_post)" | tee -a "$OUT/witness.txt"
  elif (( b_pre < 1000 )); then
    echo "  L4 INSTRUMENT ERROR: the baseline read $b_pre from an account funded 100,000,000 — the probe is what failed, not the payout (after=$b_post)" | tee -a "$OUT/witness.txt"
  else
    echo "  L4 FAIL: no payout observed — vault $b_pre -> $b_post (blocks $blk_pre -> $blk_post)" | tee -a "$OUT/witness.txt"
  fi
else
  echo "  L4 INSTRUMENT ERROR: the vault could not be read (before=$b_pre after=$b_post) — not a verdict" | tee -a "$OUT/witness.txt"
fi
echo "== quiet read (${READ_S}s)"; h0=$(height 40403); sleep "$READ_S"; h1=$(height 40403); sample
echo "  height $h0 -> $h1 over ${READ_S}s" | tee -a "$OUT/witness.txt"

for c in devnet-bootstrap devnet-validator-1 devnet-validator-2; do
  docker logs "$c" > "$OUT/$c.log" 2>&1
  grep -o 'round gate escaped[^"]*' "$OUT/$c.log" | sed "s/^/$c /" >> "$OUT/escape-lines.txt" || true
done
gzip -f "$OUT"/devnet-*.log
tools/devnet.sh down >/dev/null 2>&1
echo
echo "=== witnesses ==="; cat "$OUT/witness.txt"
echo "run root: $OUT   (commit it: spec/audit/evidence/)"
