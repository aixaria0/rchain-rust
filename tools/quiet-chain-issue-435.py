#!/usr/bin/env python3
"""Bounded no-helper, one-validator-paused finality witness on an isolated devnet.

No traffic to the public testnet. Never manufacture progress with Nil/helper deploys.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("devnet_fuzz", ROOT / "tools/devnet-fuzz.py")
fuzz = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fuzz)

def call(port, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        "http://127.0.0.1:%d%s" % (port, path), data=data,
        headers={"Content-Type": "application/json"},
        method="POST" if data is not None else "GET",
    )
    with urllib.request.urlopen(req, timeout=8) as response:
        return json.load(response)

def node(i):
    return "acceptance-bootstrap" if i == 0 else "acceptance-validator-%d" % i

def record(path, **data):
    with path.open("a") as out:
        out.write(json.dumps({"utc": time.time(), **data}, sort_keys=True) + "\n")

def run(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    events = out / "observations.jsonl"
    marker = "quiet-435-" + uuid.uuid4().hex
    paused = args.validators - 1
    survivors = list(range(paused))
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    base = { "source": source, "validators": args.validators,
             "paused_validator": paused, "live_validators": survivors,
             "quiet_window_seconds": args.window,
             "marker": marker, "marker_value": 435, "signed_deploy_count": 0,
             "helper_deploy_count": 0}
    record(events, phase="setup", **base)
    versions = []
    for i in range(args.validators):
        status = call(40403 + 1000*i, "/api/status")
        versions.append(status.get("version", {}).get("node"))
        if status.get("latestBlockNumber", 0) < 1:
            raise AssertionError("validator %d lacks genesis" % i)
        if status.get("shardId") != versions_shard(args):
            raise AssertionError("unexpected shard ID on validator %d: %s" % (i, status.get("shardId")))
        record(events, phase="initial_status", node=i, status=status)
    if len(set(versions)) != 1:
        raise AssertionError("mixed node binary versions")
    base["version"] = versions[0]
    subprocess.run(["docker", "pause", node(paused)], check=True, timeout=12)
    check = subprocess.check_output(
        ["docker", "inspect", "-f", "{{.State.Paused}}", node(paused)], text=True, timeout=10).strip()
    if check != "true":
        raise AssertionError("the selected validator did not pause")
    record(events, phase="paused", node=paused, paused=check)
    status = call(40403, "/api/status")
    term = '@' + json.dumps(marker) + '!(435)'
    write = subprocess.run(
        ["docker", "exec", "-i", node(0), "sh", "-c", "cat > /tmp/quiet435.rho"],
        input=term, capture_output=True, text=True, timeout=10)
    if write.returncode:
        raise RuntimeError("marker file write failed: " + write.stderr)
    cmd = ["docker", "exec", node(0), "rnode", "--grpc-host", "localhost",
           "deploy", "--phlo-limit", "1000000", "--phlo-price", "1",
           "--private-key", fuzz.DEPLOYER_PRIV, "--shard-id", status["shardId"],
           "--valid-after-block-number", str(status["latestBlockNumber"]),
           "/tmp/quiet435.rho"]
    submit = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    base["signed_deploy_count"] = 1
    record(events, phase="signed_marker_deploy", returncode=submit.returncode,
           stdout=submit.stdout[:2000], stderr=submit.stderr[:2000])
    if submit.returncode:
        raise RuntimeError("marker deploy submission failed")
    deadline = time.monotonic() + args.window
    last = {}
    while time.monotonic() < deadline:
        ready = 0
        for i in survivors:
            try:
                block = call(40403+1000*i, "/api/last-finalized-block")["blockInfo"]
                block_hash = block["blockHash"]
                term_read = 'new return in { for (@v <- @' + json.dumps(marker) + ') { return!(v) } }'
                response = call(40403+1000*i, "/api/v1/explore-deploy-by-block-hash",
                                {"term": term_read, "blockHash": block_hash, "usePreStateHash": False})
                match = response.get("expr") == [{"ExprInt": {"data": 435}}]
                observed = {"finalized_height": block["blockNumber"],
                            "finalized_hash": block_hash, "marker_present": match,
                            "reply_source": response.get("replySource")}
                last[str(i)] = observed
                if match: ready += 1
                record(events, phase="poll", node=i, **observed)
            except (OSError, ValueError, KeyError, urllib.error.HTTPError) as error:
                last[str(i)] = {"error": str(error)}
                record(events, phase="poll_error", node=i, error=str(error))
        if ready == len(survivors):
            base.update({"result":"PASS", "observations":last})
            (out / "result.json").write_text(json.dumps(base, indent=2, sort_keys=True)+"\n")
            print("QUIET_CHAIN_PASS " + json.dumps(base, sort_keys=True), flush=True)
            return
        time.sleep(4)
    base.update({"result":"FAIL", "reason":"no finalized marker at all surviving validators within bounded window",
                 "observations":last})
    (out / "result.json").write_text(json.dumps(base, indent=2, sort_keys=True)+"\n")
    print("QUIET_CHAIN_FAIL "+json.dumps(base, sort_keys=True), flush=True)
    raise RuntimeError(base["reason"])

def versions_shard(args):
    return os.environ.get("DEVNET_SHARD_ID", "/root")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--validators", required=True, type=int, choices=[4,8])
    ap.add_argument("--window", type=int, default=240)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        ap.error("disposable GitHub Actions runner required")
    if not 30 <= args.window <= 600:
        ap.error("invalid window")
    run(args)

if __name__ == "__main__":
    main()
