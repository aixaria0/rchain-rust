#!/usr/bin/env bash
# reconcile-network.sh — bring a busted RChain testnet back to ONE chain, without a genesis.
#
# Derived from the reconciliation utility on PR #294 (jimscarver), with the review's five invariants and
# #287's own corrections implemented. Design and rationale: issue #287. The four-divergent-heads incident
# it exists for: spec/audit/evidence/te-1-2026-10-09-four-divergent-heads.md.
#
# What it does
#   1. reads every node's finalised block and reaches a **meet** — the deepest height a *strict
#      supermajority of stake* agrees on — or refuses, naming what is missing;
#   2. **drops a provably equivocating validator from the decision before weighing stake** (two distinct
#      signed blocks by one sender at one height, which the block API can show). Never by silence: a
#      validator that is merely quiet, slow or absent keeps its whole weight;
#   3. states the point, the vouching set and the stake arithmetic out loud rather than implying them;
#   4. enumerates what is above the point, per block, and never drops it silently;
#   5. (`--apply`) stops every non-master node, moves each data directory aside — **never deleting** —
#      and restarts them to resync from the master's DAG;
#   5b. (`--restore-from-master`) copies the master's *chain state* onto each joiner instead, for the case
#      step 5 cannot reach — a net with no finalised fringe has nothing to resync to (C259). This is
#      **not a sync**: the joiners adopt the master's view wholesale, which is the operator asserting a
#      winner. It is a labelled stopgap, it never copies node identity, and the tool prints what it is
#      adopting and what it is dropping before it runs;
#   6. verifies the outcome on **block hashes per height**, not on heights.
#
# What it never does
#   * treat an unfinalised block as the truth;
#   * shrink the denominator by anything but proof (see 2);
#   * touch the master's data directory;
#   * run without `--apply` — the default prints the plan and exits 0;
#   * report a height as converged (four nodes at height 0 are equal).
#
# Transport and control
#   `RECONCILE_NODES` is a file of `host api-port unit name master?` rows; the built-in table is the
#   live testnet. `host` being `local`/`127.0.0.1`/`localhost` reads the API with curl on this host
#   (which is how the devnet drill runs it); anything else is ssh'd to.
#   `RECONCILE_CONTROL` is `systemd` (default: stop/start the `unit`, whose data dir is moved with mv)
#   or `docker` (the devnet: stop/start the container, and its data hangs off a volume, so "aside" is a
#   tar on the host kept beside the volume rather than a rename).
#
# Usage:  RECONCILE_NODES=<file> RECONCILE_CONTROL=docker reconcile-network.sh \
#           [--apply | --restore-from-master] [--master NAME]

set -uo pipefail

# host  api-port  unit  name  master?
DEFAULT_NODES='164.90.140.144 40403 rnode    A master
164.90.140.144 41403 rnode-d  D
104.131.176.164 40403 rnode   B
104.131.176.164 41403 rnode-c C'

if [ -n "${RECONCILE_NODES:-}" ]; then NODES="$(cat "$RECONCILE_NODES")"; else NODES="$DEFAULT_NODES"; fi
SSH_KEY=${SSH_KEY:-$HOME/.ssh/id_droplet}
SSH="ssh -i $SSH_KEY -o BatchMode=yes -o ConnectTimeout=10"
CONTROL=${RECONCILE_CONTROL:-systemd}
APPLY=0; MASTER=A; RESTORE=0
while [ $# -gt 0 ]; do case "$1" in
  --apply) APPLY=1;;
  --restore-from-master) RESTORE=1; APPLY=1;;
  --master) MASTER=$2; shift;;
esac; shift; done

is_local() { case "$1" in local|127.0.0.1|localhost) return 0;; esac; return 1; }

api() { # host port path
  if is_local "$1"; then
    curl -s --max-time 15 "http://127.0.0.1:$2$3" 2>/dev/null
  else
    $SSH "root@$1" "curl -s --max-time 15 http://127.0.0.1:$2$3" 2>/dev/null
  fi
}

# Every block a node holds at one height: "<blockHash> <sender> <postStateHash> <deployCount> <bonds>".
# A height legitimately holds one block per bonded validator, so this returns *all* of them — which is
# what makes the equivocation check possible from the API alone, with no node-internal surface.
blocks_at() { # host port height
  api "$1" "$2" "/api/blocks/$3/$3" | python3 -c "
import json,sys
h=int('$3')
try:
    b=json.load(sys.stdin)
    b=b if isinstance(b,list) else []
except Exception:
    sys.exit(0)
for x in b:
    if not isinstance(x,dict): continue
    if x.get('blockNumber') != h: continue
    bonds=';'.join('%s=%s' % (y.get('validator',''), y.get('stake','')) for y in (x.get('bonds') or []))
    print('%s %s %s %s %s' % (x.get('blockHash',''), x.get('sender',''),
                              x.get('postStateHash',''), x.get('deployCount',0), bonds))"
}
# The last-finalised endpoint answers with an *error string* rather than an object when there is no
# finalised fringe yet (`"Finalized fringe is not available."`) — which is the state a diverged net is in,
# so this refuses to assume an object.
json_field() { # expr over `b`, evaluated with `b` the block object (or {})
  python3 -c "
import json,sys
try:
    d=json.load(sys.stdin)
    b=(d.get('blockInfo') or d.get('block') or d) if isinstance(d,dict) else {}
    print($1)
except Exception: print('')"
}
lfb_num()  { api "$1" "$2" /api/last-finalized-block | json_field "b.get('blockNumber','')"; }
lfb_hash() { api "$1" "$2" /api/last-finalized-block | json_field "b.get('blockHash','')"; }
height_of() { api "$1" "$2" /api/status | json_field "d.get('latestBlockNumber','')"; }

# A validator's stake, read from a block's own bond map. Absent means zero weight, which is what a node
# that has never seen the validator should conclude — not an error.
stake_of() { # sender bonds
  local sender="$1" bonds="$2" kv
  for kv in ${bonds//;/ }; do
    [ "${kv%%=*}" = "$sender" ] && { echo "${kv##*=}"; return; }
  done
  echo 0
}

# --- A: the chain-state environments, by `casper/src/storage.rs::rnode_db_mapping` ---------------
#
# **Copied**: `blockstorage` (block bodies), `dagstorage` (block metadata, the fringe records, the
# approved store, the deploy and deployer indices, and the two merge caches), `rspace/history` and
# `rspace/cold` (the on-chain tuple space), and `transaction`.
#
# **Not copied, each for its own reason**: `deploypoolstorage` (this node's pending deploy pool — not
# the network's; owners re-submit), `reporting` (a local trace cache), `eval/history` and `eval/cold`
# (the *off-chain* evaluator's space, not consensus), `gateway` (its own comment says "node-local, never
# consensus state").
#
# **And never the identity.** `node.key.pem`, `node.certificate.pem` and the validator key are the
# node's own and are not in this list. A joiner holding the survivor's key *is* an equivocator, and an
# agreement check cannot see it — the two look identical until they later sign conflicting blocks.
CHAIN_ENVS="blockstorage dagstorage rspace/history rspace/cold transaction"

# The named volume a container keeps its shard data on.
data_volume() {
  docker inspect -f '{{range .Mounts}}{{if eq .Destination "/var/lib/rnode"}}{{.Name}}{{end}}{{end}}' "$1" 2>/dev/null
}

# **The stopgap, and it says so.** This is not a sync: it copies the survivor's chain state onto each
# joiner, so what the joiners agree about afterwards is the *survivor's view*, including the blocks above
# the meet that only it accepted. The operator is asserting a winner, which is why `--apply` prints the
# blocks being adopted and the blocks being dropped before it runs. It exists because on a net with no
# finalised fringe there is nothing to sync *to* (see section 3's meet and `--sync-anchor`), and it is
# marked a stopgap because it depends on the data-dir layout being movable — the thing #287's design
# deliberately avoided depending on.
restore_chain_state_from_master() {
  local n="$1" mvol vol mdir dir
  if [ "$CONTROL" = "docker" ]; then
    # `$MASTER` is the node's *name*; the container that holds its volume is `UNIT[$MASTER]`. Passing the
    # name is the bug that made this report "no data volume on A" while the volume sat there — found by
    # running it, and worth the sentence because the two are easy to confuse in this script.
    mvol="$(data_volume "${UNIT[$MASTER]}")"; vol="$(data_volume "${UNIT[$n]}")"
    [ -z "$mvol" ] && { echo "    WARNING: no data volume on ${UNIT[$MASTER]} — nothing copied" >&2; return 1; }
    [ -z "$vol" ] && { echo "    WARNING: no data volume on ${UNIT[$n]} — nothing copied" >&2; return 1; }
    docker run --rm -v "$mvol":/src:ro -v "$vol":/dst alpine sh -c "
      set -e
      copied=''; skipped=''
      for d in $CHAIN_ENVS; do
        # **A source that is not there is skipped, not fatal.** LMDB creates an environment only when
        # something writes to it, so a directory the mapping names (`transaction` on this build) can
        # simply not exist — and under \`set -e\` a failed \`cp\` aborted the whole copy, which is how the
        # first run reported a failure while the store was fine.
        if [ ! -e \"/src/\$d\" ]; then skipped=\"\$skipped \$d\"; continue; fi
        # **Copy beside, then swap.** Removing the destination first and *then* copying means a failed
        # copy leaves the joiner with nothing — the opposite of the 'aside, never gone' rule this tool
        # holds itself to everywhere else. So the new copy lands at a staging name and the old one is
        # only removed once the copy is complete.
        mkdir -p \"/dst/\$(dirname \"\$d\")\"
        rm -rf \"/dst/\$d.reconcile-new\"
        cp -a \"/src/\$d\" \"/dst/\$d.reconcile-new\"
        rm -rf \"/dst/\$d\"
        mv \"/dst/\$d.reconcile-new\" \"/dst/\$d\"
        copied=\"\$copied \$d\"
      done
      # A copied LMDB lock file is a stale lock: the joiner would wait on a lock nobody holds.
      find /dst -name 'lock.mdb' -delete
      echo \"    copied:\$copied\${skipped:+; not present on the survivor:\$skipped}\" >&2
      true" \
      && echo "    chain state copied from ${UNIT[$MASTER]}'s volume; identity and genesis untouched" \
      || { echo "    WARNING: the copy failed — $n is unchanged" >&2; return 1; }
  else
    mdir="$($SSH "root@${HOST[$MASTER]}" "systemctl cat ${UNIT[$MASTER]} | grep -oP '(?<=--data-dir ).*?(?= |\$)' | head -1" 2>/dev/null)"
    dir="$($SSH "root@${HOST[$n]}" "systemctl cat ${UNIT[$n]} | grep -oP '(?<=--data-dir ).*?(?= |\$)' | head -1" 2>/dev/null)"
    [ -z "$mdir" ] || [ -z "$dir" ] && { echo "    WARNING: a --data-dir could not be read" >&2; return 1; }
    # tar on the survivor, stream through this host, untar on the joiner: no key between the two.
    $SSH "root@${HOST[$MASTER]}" "tar -C '$mdir' -cf - $CHAIN_ENVS" 2>/dev/null \
      | $SSH "root@${HOST[$n]}" "mkdir -p '$dir' && tar -C '$dir' -xf - && find '$dir' -name lock.mdb -delete" 2>/dev/null \
      && echo "    chain state streamed from ${MASTER} to $n; identity and genesis untouched" \
      || { echo "    WARNING: the copy failed — $n is unchanged" >&2; return 1; }
  fi
}

stop_node() {
  # **Resolve the node first, in its own statement.** `local n="$1" unit="${UNIT[$n]}"` looks right and is
  # not: the words are expanded *before* `local` runs, so the subscript uses the caller's `n` — which is
  # whatever the last loop left there. It worked by accident in one loop (whose variable and argument
  # coincided) and started the wrong container in another, found by running it.
  local n="$1" host unit
  host="${HOST[$n]}"; unit="${UNIT[$n]}"
  if [ "$CONTROL" = "docker" ]; then
    docker stop "$unit" >/dev/null 2>&1 && echo "    $unit stopped (container)" && return
    echo "    WARNING: $unit did not stop" >&2
  else
    $SSH "root@$host" "systemctl stop $unit" >/dev/null 2>&1 && echo "    $unit stopped" && return
    echo "    WARNING: $unit did not stop" >&2
  fi
}
start_node() {
  local n="$1" host unit
  host="${HOST[$n]}"; unit="${UNIT[$n]}"
  if [ "$CONTROL" = "docker" ]; then
    docker start "$unit" >/dev/null 2>&1 && echo "    $unit started (container)" && return
  else
    $SSH "root@$host" "systemctl start $unit" >/dev/null 2>&1 && echo "    $unit started" && return
  fi
  echo "    WARNING: $unit did not start" >&2
}
# **Aside, never gone.** systemd: the data directory is renamed in place. docker: the data lives on a
# volume, which cannot be renamed, so its whole contents are tarred to a host file named after the
# volume and the timestamp *before* anything is removed — the backup is the "aside", and the run prints
# where it is. The genesis files are a special case and are handled first, because they may live *inside*
# the data directory: a standalone node's default genesis is `<data_dir>/genesis`, and wiping the data
# dir is what destroyed the genesis inputs on the live net. On the devnet they are mounted from outside
# at `/genesis`, so the copy is a no-op there and must not read as failure.
move_data_dir_aside() {
  local n="$1" ts="$2" host unit vol backup
  host="${HOST[$n]}"; unit="${UNIT[$n]}"
  if [ "$CONTROL" = "docker" ]; then
    vol="$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/var/lib/rnode"}}{{.Name}}{{end}}{{end}}' "$unit" 2>/dev/null)"
    if [ -z "$vol" ]; then echo "    WARNING: no data volume found on $unit — nothing moved" >&2; return; fi
    backup="${RECONCILE_BACKUP_DIR:-$PWD/target}/reconcile-backup-${vol}-${ts}.tar"
    docker run --rm -v "$vol":/data -v "$(dirname "$backup")":/backup alpine \
      tar cf "/backup/$(basename "$backup")" -C /data . >/dev/null 2>&1 \
      && echo "    $vol backed up to $backup (never deleted — this is the 'aside')" \
      || { echo "    WARNING: could not back up $vol — refusing to touch it" >&2; return 1; }
    docker rm -f "$unit" >/dev/null 2>&1
    docker volume rm "$vol" >/dev/null 2>&1 && echo "    $vol emptied; the backup holds its contents"
  else
    $SSH "root@$host" "d=\$(systemctl cat $unit | grep -oP '(?<=--data-dir ).*?(?= |\$)' | head -1)
      cp -a \$d/genesis \${d}.genesis-keep-$ts 2>/dev/null && echo '    genesis inputs preserved'
      mv \$d \${d}.bak-reconcile-$ts && echo \"    \$d -> \${d}.bak-reconcile-$ts (never deleted)\"
      mkdir -p \$d && cp -a \${d}.genesis-keep-$ts/. \$d/genesis/ 2>/dev/null
      chown -R rnode:rnode \$d" 2>/dev/null
  fi
}

declare -A HOST PORT UNIT LFB LFH HGT
NAMES=()
echo "== 1. what each node is showing =="
while read -r host port unit name master; do
  [ -z "${name:-}" ] && continue
  NAMES+=("$name"); HOST[$name]=$host; PORT[$name]=$port; UNIT[$name]=$unit
  LFB[$name]=$(lfb_num "$host" "$port"); LFH[$name]=$(lfb_hash "$host" "$port")
  HGT[$name]=$(height_of "$host" "$port")
  [ "${master:-}" = "master" ] && MASTER=$name
  printf "  %-2s h=%-6s finalised=%-6s %s\n" "$name" "${HGT[$name]:-?}" "${LFB[$name]:-?}" "${LFH[$name]:0:44}"
done <<< "$NODES"

if [ "${#NAMES[@]}" -eq 0 ]; then echo "no nodes in the table — nothing to reconcile" >&2; exit 2; fi
MAXH=0; for n in "${NAMES[@]}"; do [ "${HGT[$n]:-0}" -gt "$MAXH" ] 2>/dev/null && MAXH=${HGT[$n]}; done

# --- 2. provable equivocation, before any stake is weighed ---------------------
# **Proof, never silence.** A sender is excluded only when two *distinct* blocks by it appear at one
# height, which anyone can check from the block API and which no honest node produces. Silence, slowness
# and absence are not evidence: shrinking the denominator by silence is how an attacker who can mute
# honest validators finalises with a minority, which is the trade #290 names.
declare -A EXCLUDED=() EXCLUDED_WHY=()
EQUIV_LINES=()
echo "== 2. provable equivocation, checked before stake is weighed (heights 0..$MAXH) =="
for (( h=0; h<=MAXH; h++ )); do
  declare -A seen_sender_block=()
  for n in "${NAMES[@]}"; do
    while read -r bh sender _post _deploys _bonds; do
      [ -z "${bh:-}" ] && continue
      prev="${seen_sender_block[$sender]:-}"
      if [ -n "$prev" ] && [ "$prev" != "$bh" ]; then
        if [ -z "${EXCLUDED[$sender]:-}" ]; then
          EXCLUDED[$sender]=1
          EXCLUDED_WHY[$sender]="two distinct blocks at height $h"
          EQUIV_LINES+=("height $h: ${prev:0:12}… and ${bh:0:12}… by ${sender:0:16}…")
        fi
      fi
      seen_sender_block[$sender]="$bh"
    done < <(blocks_at "${HOST[$n]}" "${PORT[$n]}" "$h")
  done
done
if [ "${#EXCLUDED[@]}" -eq 0 ]; then
  echo "  none — every sender has at most one block per height, so no stake is dropped"
else
  for s in "${!EXCLUDED[@]}"; do echo "  EXCLUDED ${s:0:16}… — ${EXCLUDED_WHY[$s]}"; done
  for e in "${EQUIV_LINES[@]}"; do echo "    $e"; done
fi

# --- 3. the meet --------------------------------------------------------------
# **Weighed by stake, not by node count** (#287's correction): a lagging node must not drag the network
# back, and node count treats a silent validator as an equal voter. The threshold is `stake * 3 >
# total * 2` *strictly* — the protocol's own rule — not "two thirds", which reads as ≥. The pool is the
# bond map the block at that height carries, so every node's view of it is checkable; and when the nodes'
# bond maps disagree at a height, the denominators differ and no supermajority over a shared pool exists:
# that is state divergence wearing a quorum's clothes, and the corrected refusal case.
echo "== 3. the meet (stake-weighted, strict supermajority) =="
MEET=""; MEET_HASH=""; MEET_STAKE=0; MEET_POOL=0; MEET_VOUCHERS=""
TOPH=0; for n in "${NAMES[@]}"; do [ "${LFB[$n]:-0}" -gt "$TOPH" ] 2>/dev/null && TOPH=${LFB[$n]}; done
for (( h=TOPH; h>=0; h-- )); do
  declare -A hash_stake=() hash_vouchers=()
  pool=""; pool_disagreement=0
  for n in "${NAMES[@]}"; do
    while read -r bh sender _post _deploys bonds; do
      [ -z "${bh:-}" ] && continue
      if [ -z "$pool" ]; then pool="$bonds"
      elif [ "$pool" != "$bonds" ]; then pool_disagreement=1; fi
      if [ -z "${EXCLUDED[$sender]:-}" ]; then
        hash_stake[$bh]=$(( ${hash_stake[$bh]:-0} + $(stake_of "$sender" "$bonds") ))
        hash_vouchers[$bh]="${hash_vouchers[$bh]:-}${n} "
      fi
    done < <(blocks_at "${HOST[$n]}" "${PORT[$n]}" "$h")
  done
  [ -z "$pool" ] && continue
  if [ "$pool_disagreement" = "1" ]; then
    echo "  REFUSING: the nodes report different bond maps at height $h."
    echo "  The denominators differ, so no supermajority can be computed over a shared pool — that is"
    echo "  state divergence wearing a quorum's clothes. Stop and investigate; do not choose a pool."
    exit 3
  fi
  total=0; for kv in ${pool//;/ }; do total=$(( total + ${kv##*=} )); done
  excl_stake=0
  for s in "${!EXCLUDED[@]}"; do excl_stake=$(( excl_stake + $(stake_of "$s" "$pool") )); done
  denominator=$(( total - excl_stake ))
  best=""; best_stake=0
  for bh in "${!hash_stake[@]}"; do
    [ "${hash_stake[$bh]}" -gt "$best_stake" ] && { best="$bh"; best_stake=${hash_stake[$bh]}; }
  done
  if [ -n "$best" ] && [ $(( best_stake * 3 )) -gt $(( denominator * 2 )) ]; then
    MEET=$h; MEET_HASH=$best; MEET_STAKE=$best_stake; MEET_POOL=$denominator
    MEET_VOUCHERS="${hash_vouchers[$best]}"
    break
  fi
done
if [ -z "$MEET" ]; then
  echo "  REFUSING: no height has a strict supermajority of stake agreeing on one block."
  for n in "${NAMES[@]}"; do printf "    %-2s finalised %-6s %s\n" "$n" "${LFB[$n]:-?}" "${LFH[$n]:0:44}"; done
  echo "  Nothing here is safe to rewind to, and choosing one by hand would make a stall into a lie."
  exit 4
fi
echo "  the point is height $MEET, block ${MEET_HASH:0:44}"
echo "  vouched for by: $MEET_VOUCHERS"
echo "  stake: $MEET_STAKE of $MEET_POOL = $(( MEET_STAKE * 100 / MEET_POOL ))% — a strict supermajority (>66.6%)"
[ "${#EXCLUDED[@]}" -gt 0 ] && echo "  (the denominator excludes ${#EXCLUDED[@]} equivocator(s), by proof)"

# --- 4. what is above the point ----------------------------------------------
echo "== 4. what is above the point =="
echo "  The survivor is ${MASTER}'s DAG, not a chain: every validator proposes its own block each round, so"
echo "  a height legitimately holds one sibling per validator and there is no single line to extract. The"
echo "  joiners resync onto this DAG; the writers re-submit what they can see is missing."
REPORT="${RECONCILE_REPORT:-/tmp/reconcile-deploys.jsonl}"
: > "$REPORT"
total=0
for (( h=MEET+1; h<=MAXH; h++ )); do
  nblocks=0
  for n in "${NAMES[@]}"; do
    while read -r bh sender _post deploys _bonds; do
      [ -z "${bh:-}" ] && continue
      nblocks=$((nblocks+1)); total=$((total + ${deploys:-0}))
      printf '{"height":%s,"blockHash":"%s","sender":"%s","deployCount":%s,"sourceNode":"%s"}\n' \
        "$h" "$bh" "$sender" "${deploys:-0}" "$n" >> "$REPORT"
    done < <(blocks_at "${HOST[$n]}" "${PORT[$n]}" "$h")
  done
  [ "$nblocks" -gt 0 ] && printf "  #%s: %s block(s)\n" "$h" "$nblocks"
done
echo "  $total deploy(s) above the point; the per-block record is $REPORT"
echo "  (a block's *bodies* are not in the heights route — /api/block/{hash} carries the terms, the heights"
echo "   route carries counts. The owners re-submit; this tool does not claim to replay them.)"

if [ "$APPLY" != "1" ]; then
  echo "== plan only =="
  echo "  would: stop every non-master node, move each data directory aside (never delete), restart them to"
  echo "        resync onto ${MASTER}'s DAG, then verify block hashes per height."
  echo "  re-run with --apply to execute. ${MASTER}'s data directory is never touched."
  exit 0
fi

if [ "$RESTORE" = "1" ]; then
  echo "== 5. apply (--restore-from-master): the whole network is stopped, the survivor included =="
  # The survivor is stopped too, and that is not an oversight: a filesystem-level copy of a live LMDB is a
  # torn snapshot, and in this mode the copy *is* the truth the network will run on.
  for n in "${NAMES[@]}"; do echo "  $n:"; stop_node "$n"; done

  echo "== 6. the fiat, printed before it is applied =="
  echo "  This is NOT a sync. The joiners adopt ${MASTER}'s chain state wholesale, so what they agree about"
  echo "  afterwards is ${MASTER}'s view of the chain — including any block above the point that ${MASTER}"
  echo "  accepted and its peers did not. The blocks this drops are enumerated in section 4; that count is"
  echo "  the write set the owners re-submit. Running this is the operator asserting that ${MASTER} is the"
  echo "  chain. It is a stopgap because it depends on the data-dir layout being movable — which #287's"
  echo "  design deliberately avoided depending on — and not a substitute for an agreed anchor."

  TS=$(date -u +%Y%m%d-%H%M)
  echo "== 7. chain state copied onto each joiner (identity and genesis untouched) =="
  for n in "${NAMES[@]}"; do
    [ "$n" = "$MASTER" ] && continue
    echo "  $n:"
    restore_chain_state_from_master "$n"
  done

  echo "== 8. restarted: the survivor first, then the joiners =="
  start_node "$MASTER"; sleep 20
  for n in "${NAMES[@]}"; do
    [ "$n" = "$MASTER" ] && continue
    echo "  $n:"; start_node "$n"
  done
else
echo "== 5. apply: every non-master node is stopped first, before anything is moved =="
# All of them, then the moves. The version this derives from stopped and restarted each node inside one
# iteration, putting production back up before agreement had been proven.
for n in "${NAMES[@]}"; do
  [ "$n" = "$MASTER" ] && continue
  echo "  $n:"; stop_node "$n"
done

TS=$(date -u +%Y%m%d-%H%M)
echo "== 6. data directories moved aside (never deleted) =="
for n in "${NAMES[@]}"; do
  [ "$n" = "$MASTER" ] && continue
  echo "  $n:"; move_data_dir_aside "$n" "$TS"
done

echo "== 7. one joiner is restarted and must reach the point before the rest follow =="
first_joiner=""; for n in "${NAMES[@]}"; do [ "$n" = "$MASTER" ] && continue; first_joiner=$n; break; done
if [ -n "$first_joiner" ]; then
  start_node "$first_joiner"
  f=""
  for i in $(seq 1 40); do
    sleep 15
    f=$(lfb_num "${HOST[$first_joiner]}" "${PORT[$first_joiner]}")
    h="$(lfb_hash "${HOST[$first_joiner]}" "${PORT[$first_joiner]}")"
    echo "  [$i] $first_joiner finalised=${f:-?} ${h:0:32}"
    [ -n "$f" ] && [ "$f" -ge "$MEET" ] 2>/dev/null && break
  done
  if [ -z "$f" ] || [ "$f" -lt "$MEET" ] 2>/dev/null; then
    echo "  the first joiner did not reach the point — refusing to restart the others" >&2
    exit 5
  fi
fi

echo "== 8. the rest are restarted =="
for n in "${NAMES[@]}"; do
  [ "$n" = "$MASTER" ] && continue
  [ "$n" = "$first_joiner" ] && continue
  echo "  $n:"; start_node "$n"
done

fi

echo "== 9. verification: block hashes per height, not heights =="
# A converged *height* is not a converged chain — four nodes at height 0 are equal. Every assertion is on
# the hash at a height, and the frontier is compared within the protocol's own lag (one block).
ok=0
for i in $(seq 1 40); do
  sleep 15
  ok=1; line=""
  for n in "${NAMES[@]}"; do line="$line $n=$(height_of "${HOST[$n]}" "${PORT[$n]}")"; done
  for (( h=MEET; h<=MAXH; h++ )); do
    first=""
    for n in "${NAMES[@]}"; do
      bh="$(blocks_at "${HOST[$n]}" "${PORT[$n]}" "$h" | head -1 | cut -d' ' -f1)"
      [ -z "$bh" ] && continue
      if [ -z "$first" ]; then first="$bh"; elif [ "$bh" != "$first" ]; then ok=0; fi
    done
    [ "$ok" = "0" ] && break
  done
  echo "  [$i]$line  hashes-agree=$ok"
  [ "$ok" = "1" ] && break
done
if [ "$ok" != "1" ]; then
  echo "  NOT converged after the wait: the nodes still disagree on a block hash at or above $MEET" >&2
  exit 6
fi

echo "== 10. finality past the point =="
for n in "${NAMES[@]}"; do
  f=$(lfb_num "${HOST[$n]}" "${PORT[$n]}"); hh=$(lfb_hash "${HOST[$n]}" "${PORT[$n]}")
  printf "  %-2s finalised %-6s %s\n" "$n" "${f:-?}" "${hh:0:44}"
done
echo "  (this section plus section 9 is #287's falsifier: one head, agreeing block hashes, no genesis.)"
