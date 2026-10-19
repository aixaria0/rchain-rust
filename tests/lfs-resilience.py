#!/usr/bin/env python3
"""Bounded hosted-runner rejoin, partition, restart and network-delay witness."""
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import urllib.error

spec = importlib.util.spec_from_file_location("adversarial", Path(__file__).with_name("lfs-adversarial-bootstrap.py"))
adversarial = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adversarial)
p = adversarial.positive
JOINER = p.JOINER
BOOTSTRAP = p.BOOTSTRAP


def wait_http(seconds=65):
    until = time.monotonic() + seconds
    last = None
    while time.monotonic() < until:
        try:
            status = p.status(41403)
            if status.get("latestBlockNumber", 0) > 0:
                return status
        except (urllib.error.URLError, OSError, ValueError) as error:
            last = str(error)
        time.sleep(2)
    raise AssertionError("joiner HTTP unavailable after restart: " + str(last))


def confirm_genesis(expected):
    status = wait_http()
    got = p.genesis_hash(41403)
    if got != expected:
        raise AssertionError("joiner genesis mismatch")
    return status


def qdisc(*arguments):
    pid = p.cmd("docker", "inspect", "-f", "{{.State.Pid}}", BOOTSTRAP)
    if not pid.isdigit() or int(pid) <= 1:
        raise AssertionError("bootstrap network namespace unavailable")
    return adversarial.run("sudo", "nsenter", "-t", pid, "-n", "tc", *arguments, timeout=15)


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("requires a disposable GitHub-hosted runner")
    if os.environ.get("DEVNET_PREFIX", "devnet") != "devnet":
        raise SystemExit("refusing unexpected network namespace")
    ip = p.cmd("docker", "inspect", "-f",
               '{{with index .NetworkSettings.Networks "devnet"}}{{.IPAddress}}{{end}}', BOOTSTRAP)
    if not re.fullmatch(r"\d{1,3}(?:\.\d{1,3}){3}", ip):
        raise AssertionError("invalid Docker bootstrap IP")
    trusted_id = adversarial.cert_id(BOOTSTRAP)
    expected = p.genesis_hash(40403)
    if not expected or p.status(40403).get("latestBlockNumber", 0) < 3:
        raise AssertionError("bootstrap not mature enough for a rejoin witness")

    # A longer, witnessed physical disconnect. A false success during isolation fails.
    adversarial.reset(ip, key=trusted_id, pause=True)
    if "devnet" in adversarial.network_membership():
        raise AssertionError("joiner is still connected")
    for i in range(25):
        time.sleep(1)
        if adversarial.joined(expected):
            raise AssertionError("unexpected successful restore while disconnected")
    if not adversarial.network_membership().get("devnet") is None:
        raise AssertionError("joiner unexpectedly reconnected")
    adversarial.run("docker", "network", "connect", "devnet", JOINER)
    adversarial.wait_joined(expected, seconds=150)
    print("LFS_LONG_PARTITION_PASS " + json.dumps({
        "disconnected_seconds": 25, "no_restore_during_partition": True,
        "restored_after_reconnect": True, "genesis_equal": True,
        "height": confirm_genesis(expected)["latestBlockNumber"],
    }), flush=True)

    # Repeated clean-volume LFS joins on the exact same certificate identity.
    cycles = []
    for index in range(2):
        adversarial.reset(ip, key=trusted_id)
        adversarial.wait_joined(expected, seconds=110)
        info = confirm_genesis(expected)
        cycles.append({"cycle": index + 1, "height": info["latestBlockNumber"]})
    print("LFS_REJOIN_CYCLES_PASS " + json.dumps({"cycles": cycles, "genesis_equal": True}), flush=True)

    # Stop/start preserves the node data volume, unlike reset; prove restart recovery.
    adversarial.run("docker", "stop", JOINER, timeout=45)
    adversarial.run("docker", "start", JOINER, timeout=45)
    restarted = confirm_genesis(expected)
    print("LFS_DURABLE_RESTART_PASS " + json.dumps({
        "same_volume": True, "genesis_equal": True,
        "height": restarted["latestBlockNumber"],
    }), flush=True)

    # Real traffic conditioning on bootstrap egress using a Linux netem queue.
    # This is strict: if unsupported, refuse to claim the latency arm passed.
    if not shutil.which("tc") or not shutil.which("nsenter"):
        raise AssertionError("runner missing tc/nsenter: latency arm not tested")
    configured = False
    try:
        qdisc("qdisc", "replace", "dev", "eth0", "root", "netem",
              "delay", "180ms", "40ms", "loss", "1%")
        configured = True
        observed = qdisc("qdisc", "show", "dev", "eth0")
        if "netem" not in observed:
            raise AssertionError("netem queue configuration not observable")
        adversarial.reset(ip, key=trusted_id)
        adversarial.wait_joined(expected, seconds=140)
        info = confirm_genesis(expected)
        print("LFS_DELAY_LOSS_PASS " + json.dumps({
            "bootstrap_egress_delay": "180ms +/-40ms", "loss": "1%",
            "qdisc": observed.strip(), "genesis_equal": True,
            "height": info["latestBlockNumber"],
        }), flush=True)
    finally:
        if configured:
            qdisc("qdisc", "del", "dev", "eth0", "root")

if __name__ == "__main__":
    main()
