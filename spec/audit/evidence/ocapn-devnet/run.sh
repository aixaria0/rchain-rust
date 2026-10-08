#!/usr/bin/env bash
#
# run.sh — the OCapN transports on a **live devnet**.
#
# The two-node test (`node/tests/ocapn_two_nodes.rs`) runs both nodes in *one process*, on loopback,
# under a test fixture. This runs the same shape where it matters: **two node processes**, on a docker
# network, over a chain with a genesis and a block producer.
#
# `devnet-validator-1` dials the bootstrap's `noise` listener through its own admin route
# (`POST /api/v1/ocapn/dial`) and comes away holding an object the bootstrap published. The dial names
# the bootstrap by its **Ed25519 key**, read off the key file the bootstrap wrote and checked by the
# handshake — so the session is a proof, not a hope.
#
# **What it is not.** The peer is still another `rnode`: this measures the transports between nodes of
# *this* implementation and says nothing about interop with a foreign one. That is C227's open row,
# and it needs a peer speaking a different implementation — not a different host.
#
# **How a node is given an OCapN listener without touching `tools/devnet.sh`.** There is no CLI flag
# for `api-server.ocapn-listen-*` (it is hocon-only), but the node reads `<data-dir>/rnode.conf` when
# `--config-file` is not given, and the devnet mounts each node's data dir at `/var/lib/rnode`. So the
# config is written into the node's data directory and the node is restarted. Nothing about the
# network's own flags changes.
#
# **The client is `dial.py`, not curl.** This workstation's `curl` is a conda build against a libcurl
# it cannot load (`libcurl.so.4: no version information available`), and it resets the connection to a
# listener that is demonstrably serving. The dial is made with python3's stdlib instead — same
# request, same JSON, no borrowed shared object.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
cd "$ROOT"

VALIDATORS="${VALIDATORS:-2}"
PREFIX="${DEVNET_PREFIX:-devnet}"
BOOTSTRAP="${PREFIX}-bootstrap"
VALIDATOR="${PREFIX}-validator-1"
OCAPN_PORT="${OCAPN_PORT:-22045}"
# `ADMIN_BASE + 1 * 1000` — see the port constants in `tools/devnet.sh`: the admin API is published
# only under `--admin`, and each validator is offset by a thousand.
ADMIN_V1=$((40405 + 1000))
OUT="$HERE/run-1.txt"

log() { echo "[ocapn-devnet] $*"; }

{
  log "bringing up a fresh ${VALIDATORS}-validator devnet"
  bash tools/devnet.sh down >/dev/null 2>&1 || true
  bash tools/devnet.sh up --validators "$VALIDATORS"

  # The conf a node reads from its own data directory. The identity key is *not* written here: the
  # node creates it on first bind (`load_or_create_noise_identity`), mode 0600, and `dial.py` reads it
  # back — which is also the check that it was created rather than assumed.
  #
  # `docker restart` **appends** to the container's log, so a wait that greps the log for the
  # listener's line finds the line from the *previous* run and returns before the node has bound
  # anything — measured, and the way this script first reported a connection reset against a listener
  # that was simply not up yet. Each restart is therefore stamped, and the wait reads only the log
  # since then.
  declare -A SINCE
  write_conf() {
    local name="$1" dial="$2"
    {
      echo 'api-server {'
      echo "  ocapn-listen-noise = \"0.0.0.0:${OCAPN_PORT}\""
      echo '  ocapn-identity-key = "/var/lib/rnode/ocapn-identity.key"'
      echo '  ocapn-advertised-host = "127.0.0.1"'
      [[ "$dial" == yes ]] && echo '  enable-ocapn-dial = true'
      echo '}'
    } | docker exec -i "$name" sh -c 'cat > /var/lib/rnode/rnode.conf'
    SINCE["$name"]="$(date +%s)"
    docker restart "$name" >/dev/null
    log "$name: OCapN listener written to rnode.conf and the node restarted"
  }

  write_conf "$BOOTSTRAP" no
  write_conf "$VALIDATOR" yes

  # Wait for each listener to say it is serving. The node logs the line as it binds, which is after
  # the store replay — the same gap the tests' `wait_for_ocapn` covers by dialling the port.
  wait_listening() {
    local name="$1" i
    for i in $(seq 1 180); do
      if docker logs --since "${SINCE[$name]}" "$name" 2>&1 | grep -q "OCapN listener serving noise on"; then
        docker logs --since "${SINCE[$name]}" "$name" 2>&1 | grep "OCapN listener serving noise on" | tail -1
        return 0
      fi
      sleep 1
    done
    echo "!! $name never logged its noise listener" >&2
    docker logs --tail 20 "$name" >&2
    return 1
  }
  wait_listening "$BOOTSTRAP"
  wait_listening "$VALIDATOR"

  # **The identity file the node wrote**, and its mode — one file is one peer, and the mode is the
  # read-side half of C234's rule.
  docker exec "$BOOTSTRAP" stat -c '%a %n' /var/lib/rnode/ocapn-identity.key

  # **Validator-1 dials the bootstrap over `noise`**, through its own admin route. `dial.py` derives
  # the bootstrap's Ed25519 public key from the seed the node wrote (a PKCS#8 wrapper and openssl) and
  # puts it in the locator's `verify` hint — which is the frame the SYN carries.
  log "validator-1 dialling the bootstrap → http://127.0.0.1:${ADMIN_V1}/api/v1/ocapn/dial"
  # `pipefail` is set, so a dial that does not answer 200 stops the run here rather than leaving the
  # transcript to imply a success it did not have.
  python3 "$HERE/dial.py"

  # **The operator's side of the same event** (HAZOP rows C236 and D9): the bootstrap records who it
  # admitted, by the key it *proved* (C242/C243) and over which transport. This is the line that tells
  # an operator "serving" from "silently dead", and it names the peer rather than the socket.
  log "the bootstrap's record of the session:"
  docker logs "$BOOTSTRAP" 2>&1 | grep -i "session admitted" | tail -3 || echo "(no admission line)"

  log "OK — validator-1 holds an object published by the bootstrap, over noise, on a live net"
  log "the net is left running: 'tools/devnet.sh down' to stop it"
} 2>&1 | tee "$OUT"
