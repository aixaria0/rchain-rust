#!/usr/bin/env python3
"""Explicit operator-anchor LFS: verify block ancestry, state roots, durable restart.

Only for a disposable hosted Actions devnet. Does not assert an archive of all
finalized sibling branches: the sync contract follows the named anchor's ancestry.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error

def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

history = load("history", "lfs-finalized-history.py")
adversarial = history.adversarial
positive = adversarial.positive
OUTPUT = Path("target/lfs-anchor-evidence")
OUTPUT.mkdir(parents=True, exist_ok=True)


def save(name, doc):
    (OUTPUT / name).write_text(json.dumps(doc, sort_keys=True, indent=2) + "\n")


def read_block(port, hash_):
    doc = history.fetch(port, "/api/block/" + hash_)
    if not isinstance(doc, dict) or not isinstance(doc.get("blockInfo"), dict):
        raise AssertionError("missing block-detail envelope " + hash_)
    info = doc["blockInfo"]
    if info.get("blockHash") != hash_:
        raise AssertionError("wrong block returned")
    return info


def closure(port, anchor):
    found = {}
    stack = [anchor]
    while stack:
        current = stack.pop()
        if current in found:
            continue
        if len(found) > 128:
            raise AssertionError("anchor ancestry exceeds explicit test bound")
        info = read_block(port, current)
        fp = history.fingerprint(info)
        parents = info.get("justifications") or []
        if not isinstance(parents, list) or any(not isinstance(h, str) for h in parents):
            raise AssertionError("unexpected justification format")
        found[current] = fp
        for parent in parents:
            if parent not in found:
                stack.append(parent)
    return found


def inspect_anchor(expected, anchor, port=41403, deadline=140):
    end = time.monotonic() + deadline
    last = {}
    while time.monotonic() < end:
        matched = {}
        for key, fp in expected.items():
            try:
                observed = history.fingerprint(read_block(port, key))
                if observed == fp:
                    matched[key] = observed
            except (OSError, ValueError, KeyError, AssertionError, urllib.error.URLError):
                pass
        missing = sorted(set(expected) - set(matched))
        finalized = history.last_finalized(port)
        last = {"matched": len(matched), "total": len(expected), "missing": missing,
                "lfb": finalized["blockNumber"] if finalized else None}
        if not missing and finalized and finalized["blockNumber"] >= anchor["blockNumber"]:
            if history.is_finalized(port, anchor["blockHash"]):
                return last
        time.sleep(3)
    save("anchor_failure.json", last)
    raise AssertionError("explicit anchor/state restore incomplete: " + repr(last))


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("disposable hosted CI only")
    if os.environ.get("DEVNET_PREFIX", "devnet") != "devnet":
        raise SystemExit("unexpected devnet prefix")
    genesis = positive.genesis_hash(40403)
    if not genesis:
        raise AssertionError("missing genesis before controlled proposal")
    deployment = subprocess.run(["tools/devnet.sh", "deploy", "hello.rho"], text=True,
                                capture_output=True, timeout=60)
    if deployment.returncode:
        raise AssertionError("seed deploy rejected: " + deployment.stdout[-500:])
    print("LFS_ANCHOR_DEPLOY " + deployment.stdout[-200:], flush=True)

    baseline = None
    for attempt in range(30):
        proposed = subprocess.run(["tools/devnet.sh", "propose"], text=True,
                                  capture_output=True, timeout=30)
        anchor = history.last_finalized(40403)
        if anchor and anchor["blockNumber"] >= 2:
            ancestor = closure(40403, anchor["blockHash"])
            changed = [fp for fp in ancestor.values()
                       if fp["blockNumber"] > 0 and fp["preStateHash"] and fp["preStateHash"] != fp["postStateHash"]]
            if len(ancestor) >= 3 and changed:
                baseline = {"anchor": anchor, "ancestry": ancestor,
                            "changed_ancestors": len(changed), "attempts": attempt+1}
                break
        time.sleep(2)
    if not baseline:
        raise AssertionError("could not drive a nontrivial finalized anchor and state change")
    save("anchor_baseline.json", baseline)
    print("LFS_ANCHOR_BASELINE " + json.dumps({
        "hash": baseline["anchor"]["blockHash"], "height": baseline["anchor"]["blockNumber"],
        "ancestor_count": len(baseline["ancestry"]), "changed": baseline["changed_ancestors"],
    }), flush=True)

    bootstrap_ip = positive.cmd("docker", "inspect", "-f",
        '{{with index .NetworkSettings.Networks "devnet"}}{{.IPAddress}}{{end}}', positive.BOOTSTRAP)
    node_id = adversarial.cert_id(positive.BOOTSTRAP)
    os.environ["DEVNET_RESET_SYNC_ANCHOR"] = baseline["anchor"]["blockHash"]
    adversarial.reset(bootstrap_ip, key=node_id)
    args = json.loads(positive.cmd(
        "docker", "inspect", "-f", "{{json .Config.Cmd}}", positive.JOINER))
    if "--sync-anchor" not in args or args[args.index("--sync-anchor") + 1] != baseline["anchor"]["blockHash"]:
        raise AssertionError("explicit anchor flag was not installed on joiner")
    if not adversarial.joined(genesis):
        adversarial.wait_joined(genesis, seconds=140)
    result = inspect_anchor(baseline["ancestry"], baseline["anchor"])
    logs_before = adversarial.logs()
    count_before = logs_before.count("LFS state is successfully restored.")
    if count_before < 1:
        raise AssertionError("missing successful LFS checkpoint log")
    save("first_restore.json", {"result": result, "restore_log_count": count_before})
    print("LFS_EXPLICIT_ANCHOR_RESTORED " + json.dumps(result), flush=True)

    adversarial.run("docker", "stop", positive.JOINER, timeout=50)
    adversarial.run("docker", "start", positive.JOINER, timeout=50)
    result2 = inspect_anchor(baseline["ancestry"], baseline["anchor"])
    logs_after = adversarial.logs()
    count_after = logs_after.count("LFS state is successfully restored.")
    checkpoint = {"result": result2, "before_restart": count_before,
                  "after_restart": count_after, "new_lfs": count_after > count_before}
    save("restart_checkpoint.json", checkpoint)
    if count_after > count_before:
        raise AssertionError("restarted node reentered LFS despite retained store")
    print("LFS_EXPLICIT_ANCHOR_PERSISTENT " + json.dumps(checkpoint), flush=True)


if __name__ == "__main__":
    main()
