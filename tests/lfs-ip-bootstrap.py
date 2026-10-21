#!/usr/bin/env python3
"""Isolated two-peer identity-address LFS rejoin witness on a disposable devnet."""
import json
import os
import re
import subprocess
import sys
import time
import urllib.request

BOOTSTRAP = "devnet-bootstrap"
JOINER = "devnet-validator-1"
NETWORK = "devnet"


def cmd(*args, timeout=25):
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout, check=True).stdout.strip()


def status(port):
    with urllib.request.urlopen("http://127.0.0.1:%d/api/v1/status" % port, timeout=3) as response:
        return json.load(response)


def genesis_hash(port):
    with urllib.request.urlopen("http://127.0.0.1:%d/api/blocks/0/0" % port, timeout=4) as response:
        blocks = json.load(response)
    if not isinstance(blocks, list) or not blocks:
        return None
    return blocks[0].get("blockHash")


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("refusing LFS reset outside a disposable hosted Actions runner")
    if os.environ.get("DEVNET_PREFIX", "devnet") != "devnet":
        raise SystemExit("unexpected devnet prefix")
    running = cmd("docker", "inspect", "-f", "{{.State.Running}}", BOOTSTRAP)
    if running != "true":
        raise SystemExit("bootstrap not running")
    bootstrap_ip = cmd(
        "docker", "inspect", "-f",
        '{{with index .NetworkSettings.Networks "devnet"}}{{.IPAddress}}{{end}}',
        BOOTSTRAP,
    )
    if not re.fullmatch(r"\d{1,3}(?:\.\d{1,3}){3}", bootstrap_ip):
        raise SystemExit("missing disposable bootstrap IPv4 address")
    before = status(40403)
    earliest = time.monotonic() + 90
    while before.get("latestBlockNumber", 0) < 3 and time.monotonic() < earliest:
        time.sleep(2)
        before = status(40403)
    if before.get("latestBlockNumber", 0) < 3:
        raise SystemExit("bootstrap did not produce enough blocks for a meaningful LFS rejoin")
    original_genesis = genesis_hash(40403)
    if not original_genesis:
        raise SystemExit("bootstrap did not expose genesis block hash")
    old_joiner = status(41403)
    if old_joiner.get("latestBlockNumber", 0) < 1:
        raise SystemExit("joiner did not have prior genesis before reset")
    env = {**os.environ, "DEVNET_RESET_BOOTSTRAP_HOST": bootstrap_ip}
    print("LFS_INPUT " + json.dumps({
        "network": NETWORK, "bootstrap_host": BOOTSTRAP, "configured_locator": bootstrap_ip,
        "baseline_bootstrap_height": before["latestBlockNumber"],
        "baseline_joiner_height": old_joiner["latestBlockNumber"],
        "genesis_hash": original_genesis
    }), flush=True)
    reset = subprocess.run(["tools/devnet.sh", "reset", "1"], env=env, timeout=45,
                           text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print("LFS_RESET " + reset.stdout[-4500:], flush=True)
    if reset.returncode != 0:
        raise SystemExit("reset failed: %d" % reset.returncode)
    config = json.loads(cmd("docker", "inspect", "-f", "{{json .Config.Cmd}}", JOINER))
    cmd_args = " ".join(config)
    if ("@" + bootstrap_ip + "?protocol=") not in cmd_args:
        raise SystemExit("joiner did not actually receive an IP bootstrap locator")
    if "--host devnet-validator-1" not in cmd_args:
        raise SystemExit("unexpected joiner advertised host")
    deadline = time.monotonic() + 180
    last_status = None
    last_error = None
    last_logs = ""
    while time.monotonic() < deadline:
        # Docker sends node logs to stdout and stderr.
        result = subprocess.run(["docker", "logs", JOINER], capture_output=True, text=True, timeout=15)
        last_logs = result.stdout + result.stderr
        restored = "LFS state is successfully restored." in last_logs
        if restored:
            try:
                last_status = status(41403)
                got_genesis = genesis_hash(41403)
                if got_genesis != original_genesis:
                    raise AssertionError("rejoined genesis mismatch")
                if last_status.get("latestBlockNumber", 0) < 1:
                    raise AssertionError("rejoined node has no genesis")
                print("LFS_REJOIN_PASS " + json.dumps({
                    "bootstrap_locator": bootstrap_ip,
                    "transport_authenticated_bootstrap_id": True,
                    "restored_log": restored,
                    "checkpoint_confirmation": True,
                    "joiner_height": last_status["latestBlockNumber"],
                    "same_genesis_hash": got_genesis == original_genesis,
                }), flush=True)
                return
            except (OSError, ValueError, AssertionError) as error:
                last_error = str(error)
        time.sleep(2)
    print("LFS_REJOIN_FAIL " + json.dumps({
        "last_error": last_error,
        "joiner_status": last_status,
        "tail": last_logs[-14000:],
    }), flush=True)
    raise SystemExit(1)


if __name__ == "__main__":
    main()
