#!/usr/bin/env python3
"""Disposable-hosted-runner LFS controls: wrong trust key and interrupted rejoin."""
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import urllib.error

spec = importlib.util.spec_from_file_location(
    "positive", Path(__file__).with_name("lfs-ip-bootstrap.py"))
positive = importlib.util.module_from_spec(spec)
spec.loader.exec_module(positive)


def run(*args, timeout=35, env=None):
    return subprocess.run(args, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          timeout=timeout, env=env).stdout.strip()


def cert_id(node):
    pem = subprocess.run(
        ["docker", "exec", node, "cat", "/var/lib/rnode/node.certificate.pem"],
        check=True, stdout=subprocess.PIPE, timeout=15,
    ).stdout
    subject = subprocess.run(
        ["openssl", "x509", "-noout", "-subject", "-nameopt", "RFC2253"],
        input=pem, check=True, stdout=subprocess.PIPE, timeout=10,
    ).stdout.decode().strip().replace(" ", "")
    if not subject.startswith("subject=CN="):
        raise AssertionError("unexpected node certificate subject")
    return subject.split("subject=CN=", 1)[1]


def logs():
    return run("docker", "logs", "--tail", "250",
               positive.JOINER, timeout=15)


def joined(expected_hash):
    data = logs()
    if "LFS state is successfully restored." not in data:
        return False
    try:
        current = positive.genesis_hash(41403)
        return current == expected_hash and positive.status(41403).get(
            "latestBlockNumber", 0) >= 1
    except (OSError, ValueError, urllib.error.URLError):
        return False


def reset(ip, key=None, pause=False):
    env = dict(os.environ)
    env["DEVNET_RESET_BOOTSTRAP_HOST"] = ip
    if key is not None:
        env["DEVNET_RESET_BOOTSTRAP_ID"] = key
    if pause:
        env["DEVNET_RESET_NETWORK_PAUSE"] = "1"
    output = run("tools/devnet.sh", "reset", "1",
                 env=env, timeout=60)
    config = json.loads(positive.cmd(
        "docker", "inspect", "-f", "{{json .Config.Cmd}}", positive.JOINER))
    arguments = " ".join(config)
    wanted = key if key is not None else cert_id(positive.BOOTSTRAP)
    if ("rnode://" + wanted + "@" + ip + "?protocol=") not in arguments:
        raise AssertionError("reset did not install exact requested bootstrap peer")
    print("LFS_RESET_CONFIG " + json.dumps({
        "configured_node_id": wanted, "bootstrap_locator": ip,
        "network_paused": pause, "reset_log_tail": output[-300:],
    }), flush=True)


def wait_joined(expected_hash, seconds=100):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if joined(expected_hash):
            return
        time.sleep(2)
    raise AssertionError(
        "LFS rejoin timed out: " + logs()[-3000:])


def network_membership():
    raw = positive.cmd(
        "docker", "inspect", "-f", "{{json .NetworkSettings.Networks}}",
        positive.JOINER)
    return json.loads(raw)


def main():
    if (os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"):
        raise SystemExit("isolated hosted Actions runner required")
    if os.environ.get("DEVNET_PREFIX", "devnet") != "devnet":
        raise SystemExit("refusing unexpected devnet namespace")
    old = positive.status(40403)
    if old.get("latestBlockNumber", 0) < 3:
        raise AssertionError("bootstrap needs mature chain before reset")
    expected_hash = positive.genesis_hash(40403)
    if not expected_hash:
        raise AssertionError("missing bootstrap genesis")
    ip = positive.cmd("docker", "inspect", "-f",
        '{{with index .NetworkSettings.Networks "devnet"}}{{.IPAddress}}{{end}}',
        positive.BOOTSTRAP)
    if not re.fullmatch(r"\d{1,3}(?:\.\d{1,3}){3}", ip):
        raise AssertionError("invalid Docker bootstrap IP")
    trusted_id = cert_id(positive.BOOTSTRAP)
    other_id = cert_id("devnet-validator-2")
    if other_id == trusted_id:
        raise AssertionError("positive and negative trust identities coincide")
    print("LFS_ADVERSARIAL_BASELINE " + json.dumps({
        "bootstrap_height": old["latestBlockNumber"],
        "genesis_hash": expected_hash,
        "ip": ip, "different_authenticated_ids": other_id != trusted_id,
    }), flush=True)

    # Different, real certificate identity at the correct bootstrap IP:
    # no approved state may be accepted as coming from the configured key.
    reset(ip, key=other_id)
    for _ in range(6):
        time.sleep(2)
        if joined(expected_hash):
            raise AssertionError("wrong bootstrap identity restored genesis")
    print("LFS_WRONG_ID_REFUSED " + json.dumps({
        "no_restore_in_12_seconds": True,
        "configured_id_is_not_bootstrap": True,
        "joiner_running": positive.cmd(
            "docker", "inspect", "-f", "{{.State.Running}}",
            positive.JOINER) == "true",
    }), flush=True)

    # Reconfigure with the actual certificate identity, same IP.
    reset(ip, key=trusted_id)
    wait_joined(expected_hash)
    print("LFS_CORRECT_ID_RECOVERED " + json.dumps({
        "restored": True, "same_genesis_hash": True,
        "height": positive.status(41403)["latestBlockNumber"],
    }), flush=True)

    # Force a bounded partition immediately after a *fresh* reset.
    reset(ip, key=trusted_id, pause=True)
    if "devnet" in network_membership():
        raise AssertionError("network partition control did not disconnect joiner")
    for _ in range(4):
        time.sleep(1)
        if joined(expected_hash):
            raise AssertionError("state restored while joiner disconnected")
    run("docker", "network", "connect", "devnet", positive.JOINER)
    if "devnet" not in network_membership():
        raise AssertionError("network reconnect failed")
    wait_joined(expected_hash, seconds=150)
    print("LFS_PARTITION_REJOIN_PASS " + json.dumps({
        "network_isolation_observed": True,
        "joined_after_reconnect": True,
        "same_genesis_hash": True,
        "height": positive.status(41403)["latestBlockNumber"],
    }), flush=True)


if __name__ == "__main__":
    main()
