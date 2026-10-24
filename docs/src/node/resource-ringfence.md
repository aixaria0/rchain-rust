# Ringfencing a node

A node cannot set its own memory, CPU or disk limits — that is the operator's boundary — and a node
that is not bounded competes with everything else on the host for memory. The #420 incident is the
worked example: two validators on each 1 GB host, a deploy burst, host memory exhausted, and the kernel
chose which processes died. This page is the operator half of the
[resource safety case](../spec/resource-safety-case.md); the node-side preflight, admission and fallback
are tracked there (HAZOP study node U17) and are **not yet shipped**. Until they are, the limits on this
page are the only barrier between a busy node and the host.

One validator per host is the default. Two validators may share a host only when the host is sized for
both and the failure-domain risk is accepted: a host loss then removes **both** validators' stake at
once, which is up to half of a four-validator pool.

## Give the node its own cgroup

On a systemd host, a drop-in or a unit with a dedicated slice is the simplest ringfence. Size the limits
from [the hardware requirements](validator-requirements.md) — including the **restart/replay** headroom,
not just steady-state RSS.

```ini
# /etc/systemd/system/rnode.service.d/resource.conf
[Service]
MemoryHigh=6G
MemoryMax=8G
MemorySwapMax=0
CPUQuota=200%
IOWeight=100
TasksMax=4096
Restart=on-failure
RestartSec=10s
```

- `MemoryHigh` is the throttle point; `MemoryMax` is the hard ceiling. Keeping
  `MemorySwapMax=0` (or equal to `MemoryMax` with Docker) prevents a slow swap death.
- The cgroup limit is also the node's **effective budget**: a limit below the chain's replay floor will
  OOM the node during start-up, before its API opens. The safety case's preflight (unit A2) is designed
  to refuse that host rather than replay into it.
- `CPUQuota` leaves headroom for the kernel and the page cache; do not give the node every core.
- A hard cgroup limit contains the damage — the cgroup OOM killer takes the node, not the host — but it
  is still a failure. The goal is to shed work before the limit, not to find it.

For a one-off run:

```sh
systemd-run --scope --slice=rnode.slice \
  -p MemoryHigh=6G -p MemoryMax=8G -p MemorySwapMax=0 -p CPUQuota=200% \
  /usr/local/bin/rnode run --profile default ...
```

## Docker

```sh
docker run --name rnode \
  --memory 8g --memory-swap 8g --cpus 2 --pids-limit 4096 \
  -v /srv/rnode:/var/lib/rnode \
  rnode:local run --profile docker ...
```

`--memory-swap` equal to `--memory` disables swap; leaving it unset lets the container swap, which turns
an OOM into a long stall. The devnet tooling already uses this shape (`tools/devnet.sh`).

## Give the data directory its own disk

LMDB is synchronous-write-heavy and its map sizes are sparse reservations, not immediate usage
(`casper/src/storage.rs`). Put `storage.data-dir` on a **dedicated NVMe mount**, not the root filesystem,
and leave free space for the append-only history and the RSpace trie. A spinning disk or a nested
container's I/O layer dominates the cost even when the CPU and RAM are adequate.

```sh
mount | grep "$(rnode ... --data-dir 2>/dev/null || echo /var/lib/rnode)"   # confirm the filesystem
df -h /var/lib/rnode                                                      # confirm free space
```

The start-up preflight (unit A2) is designed to refuse a data dir with less free space than the chain's
growth plus a margin; until it lands, check `df` before a restart on a long chain.

## Monitor the boundary, not just the node

Alert on the cgroup and the filesystem, not only on `/api/status`:

```sh
cat /sys/fs/cgroup/rnode.slice/memory.current
cat /sys/fs/cgroup/rnode.slice/memory.peak
cat /sys/fs/cgroup/rnode.slice/memory.events     # low high max oom oom_kill
cat /sys/fs/cgroup/rnode.slice/cpu.stat
df -h /var/lib/rnode
```

The external `testnet.rhobot.net/health` snapshot carries `mem_available_mb`/`disk_free_mb`; the node's
own `degraded`/`acceptsDeploys` fields and operator headroom land with units A1/C of the safety case.
Neither surface is a certificate: a `degraded = normal` sample means the node is not reporting pressure
at that instant, not that it is healthy.

## What the node cannot do

The node cannot raise a cgroup limit, prevent co-location, or choose the kernel's OOM victim. It can
detect a limit, refuse to start below a policy floor, shed work before the limit, degrade to read-only,
and report the state and the reason. Those are the safety-case barriers B31–B41; this page is the
operator barrier they depend on.
