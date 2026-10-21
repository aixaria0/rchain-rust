#!/usr/bin/env python3
"""Fail-closed finalized-history/state and durable checkpoint LFS witness.

Runs ONLY on isolated GitHub-hosted devnet. Compares finalized blocks including
their post-state hashes, not merely genesis or API readiness.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request

spec = importlib.util.spec_from_file_location("adversarial", Path(__file__).with_name("lfs-adversarial-bootstrap.py"))
adversarial = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adversarial)
p = adversarial.positive
ART = Path("target/lfs-history-evidence")
ART.mkdir(parents=True, exist_ok=True)


def fetch(port, path, timeout=8):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=timeout) as response:
        return json.load(response)


def last_finalized(port):
    try:
        obj = fetch(port, "/api/last-finalized-block")
        if isinstance(obj, dict) and isinstance(obj.get("blockInfo"), dict):
            info = obj["blockInfo"]
            if isinstance(info.get("blockNumber"), int) and info.get("blockHash"):
                return info
    except (OSError, ValueError, urllib.error.URLError):
        pass
    return None


def blocks_at(port, height):
    rows = fetch(port, f"/api/blocks/{height}/{height}")
    if not isinstance(rows, list):
        raise AssertionError("blocks response is not an array")
    return [row for row in rows if isinstance(row, dict) and row.get("blockNumber") == height]


def is_finalized(port, hash_):
    value = fetch(port, "/api/is-finalized/" + hash_)
    if not isinstance(value, bool):
        raise AssertionError("is-finalized route must return JSON boolean")
    return value


def fingerprint(row):
    fields = ("blockHash", "blockNumber", "sender", "preStateHash", "postStateHash")
    values = {field: row.get(field) for field in fields}
    if not values["blockHash"] or not values["postStateHash"] or not isinstance(values["blockNumber"], int):
        raise AssertionError("incomplete block metadata: " + repr(values))
    return values


def gather(port, cut):
    result = {}
    for height in range(cut + 1):
        for row in blocks_at(port, height):
            f = fingerprint(row)
            if is_finalized(port, f["blockHash"]):
                result[f["blockHash"]] = f
    return result


def write(name, obj):
    path = ART / name
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n")
    return hashlib.sha256(path.read_bytes()).hexdigest()


def assert_history(port, expected, anchor, deadline=130):
    end = time.monotonic() + deadline
    recent = None
    while time.monotonic() < end:
        try:
            lfb = last_finalized(port)
            rows = gather(port, anchor["blockNumber"])
            matched = {key: row for key, row in rows.items() if key in expected}
            missing = sorted(set(expected) - set(matched))
            mismatch = [key for key in matched if matched[key] != expected[key]]
            recent = {"lfb": lfb, "found": len(matched), "missing": missing, "mismatch": mismatch}
            if not missing and not mismatch and lfb and lfb["blockNumber"] >= anchor["blockNumber"]:
                if not is_finalized(port, anchor["blockHash"]):
                    raise AssertionError("anchor is no longer finalized")
                return recent
        except (OSError, ValueError, urllib.error.URLError) as ex:
            recent = {"read_error": str(ex)}
        time.sleep(3)
    write("history_failure.json", recent)
    raise AssertionError("finalized history/state did not converge: " + repr(recent))


def inspect_logs():
    result = subprocess.run(["docker", "logs", "devnet-validator-1"], capture_output=True, text=True, timeout=15)
    return result.stdout + result.stderr


def checkpoint_witness(expected, anchor, label):
    joined = assert_history(41403, expected, anchor, deadline=140)
    if not joined["lfb"]:
        raise AssertionError("no finalized LFB reported after restore")
    return {"phase": label, "verified_finalized_blocks": len(expected), "lfb": joined["lfb"],
            "anchor": anchor["blockHash"]}


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("refusing non-hosted/non-isolated runner")
    if os.environ.get("DEVNET_PREFIX", "devnet") != "devnet":
        raise SystemExit("refusing foreign devnet")
    ip = p.cmd("docker", "inspect", "-f",
               '{{with index .NetworkSettings.Networks "devnet"}}{{.IPAddress}}{{end}}', p.BOOTSTRAP)
    identity = adversarial.cert_id(p.BOOTSTRAP)
    initial = p.status(40403)
    if initial.get("latestBlockNumber", 0) < 1:
        raise AssertionError("missing bootstrap genesis")

    # A real state transition must exist before any checkpoint claim.
    deploy = subprocess.run(["tools/devnet.sh", "deploy", "hello.rho"], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
    if deploy.returncode != 0:
        raise AssertionError("state-changing deploy rejected: " + deploy.stdout[-1600:])
    print("LFS_STATE_DEPLOY " + deploy.stdout[-500:], flush=True)

    anchor = None
    expected = None
    deadline = time.monotonic() + 160
    proposed = 0
    while time.monotonic() < deadline:
        if os.environ.get("LFS_HISTORY_MANUAL_PROPOSE") == "1" and proposed < 24:
            trial = subprocess.run(["tools/devnet.sh", "propose"], text=True,
                                   capture_output=True, timeout=35)
            proposed += 1
            print("LFS_CONTROL_PROPOSE " + json.dumps({
                "attempt": proposed, "code": trial.returncode,
                "tail": (trial.stdout + trial.stderr)[-260:]}), flush=True)
        lfb = last_finalized(40403)
        if lfb and lfb["blockNumber"] >= 2:
            cut = min(lfb["blockNumber"], 48)
            snap = gather(40403, cut)
            changed = [x for x in snap.values() if x["blockNumber"] > 0
                       and x["preStateHash"] and x["preStateHash"] != x["postStateHash"]]
            if changed and len(snap) >= 2:
                anchor = lfb
                # Only the bounded finalized prefix (<=48) is claimed.
                expected = snap
                break
        time.sleep(3)
    if not expected:
        raise AssertionError("no finalized, state-changing prefix within 160 seconds")
    baseline = {"lfb": anchor, "finalized_count": len(expected),
                "changed_finalized_count": len(changed), "cut_height": cut,
                "blocks": expected}
    sha = write("baseline.json", baseline)
    print("LFS_HISTORY_BASELINE " + json.dumps({
        "anchor_height": anchor["blockNumber"], "anchor_hash": anchor["blockHash"],
        "cut_height": cut, "finalized_count": len(expected),
        "state_changes": len(changed), "baseline_sha256": sha,
    }), flush=True)

    adversarial.reset(ip, key=identity)
    if "LFS state is successfully restored." not in inspect_logs():
        adversarial.wait_joined(p.genesis_hash(40403), seconds=140)
    first = checkpoint_witness(expected, anchor, "fresh_volume_lfs")
    logs_before = inspect_logs()
    restores_before = logs_before.count("LFS state is successfully restored.")
    if restores_before < 1:
        raise AssertionError("LFS restore evidence absent")
    write("first_restore.json", first)
    print("LFS_FINALIZED_STATE_RESTORED " + json.dumps(first), flush=True)

    # Restart the SAME container on its existing volume, then require the
    # persisted finalized prefix still exists. A new LFS is not disguised.
    adversarial.run("docker", "stop", p.JOINER, timeout=50)
    adversarial.run("docker", "start", p.JOINER, timeout=50)
    second = checkpoint_witness(expected, anchor, "persistent_volume_restart")
    logs_after = inspect_logs()
    restores_after = logs_after.count("LFS state is successfully restored.")
    second["restores_before_restart"] = restores_before
    second["restores_after_restart"] = restores_after
    second["new_lfs_after_restart"] = restores_after > restores_before
    write("restart_checkpoint.json", second)
    if restores_after > restores_before:
        raise AssertionError("restart triggered LFS instead of durable restored checkpoint")
    print("LFS_CHECKPOINT_PERSISTENCE_PASS " + json.dumps(second), flush=True)
    print("LFS_STATE_HISTORY_PASS " + json.dumps({
        "finalized_blocks_checked": len(expected), "changed_blocks": len(changed),
        "anchor_height": anchor["blockNumber"],
        "post_state_hashes_identical": True, "restored_without_second_lfs": True,
    }), flush=True)


if __name__ == "__main__":
    main()
