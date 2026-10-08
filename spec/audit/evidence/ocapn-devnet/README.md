# The OCapN transports on a live devnet

**What this is.** Two `rnode` processes on a docker network, over a chain with a genesis and a block
producer, where one node reaches an object published by the other over the **`noise`** transport. The
transcript is `run-1.txt`; `bash run.sh` reproduces it.

**Why it exists.** Every other piece of evidence for these transports puts either a *test client* or
a *harness* on one side of the wire:

| evidence | what is on the far side | what that leaves open |
|---|---|---|
| `node/tests/ocapn_listener.rs` | this repository's own `Session::dial`, in the test process | the node's **own** dial path is not what was exercised |
| `node/tests/ocapn_two_nodes.rs` | a second node — in the *same process*, on loopback | two processes, a real network, a real chain |
| `ocapn-noise/` | Agoric's Noise *core*, driven from a harness | not a node, and not a session |
| this run | a second **node process**, on a docker network | — |

**What `run-1.txt` shows, line by line.**

- Both nodes serve `noise` on `0.0.0.0:22045` — `OCapN listener serving noise on … (2 chain-backed
  capability/ies)`. That is a node started from the image built off this branch, with a genesis and
  blocks (`latestBlockNumber=158`), not a test fixture.
- The identity file is mode `600` and 64 bytes, created by the node (`load_or_create_noise_identity`)
  rather than supplied — C234's rule, read-heart.
- `devnet-validator-1` dials the bootstrap through **its own admin route**
  (`POST /api/v1/ocapn/dial`, `enable-ocapn-dial`), and answers `HTTP 200` with
  `{"fetched":"Export(1)","peer":"devnet-bootstrap.noise"}` — a descriptor for an object the bootstrap
  exported, held by a session that did not exist a moment before.
- The dial names the bootstrap by its **Ed25519 key**, derived from the seed the node wrote and put in
  the locator's `verify` hint. The handshake is therefore a proof: a dial naming a different key is
  refused at the cleartext prefix, before any cryptography. (`ocapn_two_nodes.rs` is the same
  differential in-process.)
- The bootstrap's own record of the event is the operator's half of HAZOP rows C236/D9 —
  `session admitted: 6b3a02… (proved 6b3a02…) over noise` — naming the peer by the key it **proved**
  (C242/C243) and the transport it arrived on, rather than the socket.

**How a node is given an OCapN listener without touching `tools/devnet.sh`.** There is no CLI flag for
`api-server.ocapn-listen-*` — it is hocon-only — but a node reads `<data-dir>/rnode.conf` when
`--config-file` is not given, and the devnet mounts each node's data dir at the root of its own volume.
So `run.sh` writes the config into the node's data directory and restarts the node. The network's own
flags are untouched.

**Two traps this run paid for, recorded so the next one does not.**

1. **`docker restart` appends to the container's log**, so waiting for a line that says the listener
   came up finds the line from *before* the restart and returns at once. The first version of this
   script dialled a node that had bound nothing and reported a connection reset against a listener
   that was not up. Each restart is now stamped and the wait reads the log `--since` then.
2. **A TCP connect to a published docker port proves nothing.** The port's userland proxy accepts
   before the container's listener exists, and then resets. The client therefore retries on a reset
   and treats an HTTP *status* — 200, or a refusal — as the readiness signal.

**What this does not show, said rather than implied.**

- **Interop with a foreign implementation.** The peer here is another `rnode`. This measures the
  transports *between nodes of this implementation*, and nothing about a peer that speaks a different
  one. That is **C227**, and it needs a peer speaking another implementation, not another host.
- **The websocket hang.** `websocket` is not exercised here; `endo-spike/run-3.txt` is where that
  transport's standing is recorded.
- **Public-testnet readiness.** This is a local devnet. The programme's acceptance bar is #214's
  criteria (`docs/src/spec/testnet-acceptance.md`), and criterion 1 is still ❌ on A1.5 with two arms
  unrun. This run does not move that, and must not be read as if it did.

**Reproducing.** `bash run.sh` brings the net up, writes the config, dials, and prints the recording.
It needs a built `rnode:local` (`tools/devnet.sh build`) from the revision under test; the image in use
when this transcript was written was built from `ocapn/noise` at `8ef355499`. The net is left running —
`tools/devnet.sh down` stops it.

`dial.py` is the client, and it is a client only: it derives the peer's public key and makes the HTTP
request. It exists because the workstation's `curl` is a conda build against a libcurl it cannot load
and resets against a listener that is demonstrably serving.
