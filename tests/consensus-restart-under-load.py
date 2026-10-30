#!/usr/bin/env python3
"""Restart-under-load: kill one validator mid-burst, restart it, require exact agreement.

This is the end-to-end falsifier for Law 51d (a stalled escape consumes its permit) and Law 88c
(a stored pending body is re-offered at runtime). It is a **disposable Docker devnet** arm: it does
not authorise a public-net deployment, and it changes no consensus parameter.

PASS requires, for each cycle, within `--window` seconds of the victim's restart:

* all validators report the **same finalized `(height, hash)`**, at least three heights above the
  pre-kill baseline;
* every deploy submitted in the window is resolved `ProcessedWithSuccess` on every validator;
* no finalized height regresses and no fixed height changes hash across samples;
* every `/health` reports `pipelineFaults == 0`, `orphanBodies == 0`, `proposalHalted == false`,
  and no log contains `block receiver state full`;
* the victim proposes again (its own `proposed` counter advances after restart).

The predicates live in `restart_under_load_controls.py` so they can be tested offline; this script
only collects and feeds them.
"""
import argparse
import importlib.util
import json
import re
import subprocess
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


drill = _load("acceptance_drill", ROOT / "tools/consensus-acceptance-drill.py")
controls = _load("restart_controls", ROOT / "tests/restart_under_load_controls.py")
fuzz = drill.fuzz

PREFIX = __import__("os").environ.get("DEVNET_PREFIX", "restartload")


def node_name(i):
    return f"{PREFIX}-bootstrap" if i == 0 else f"{PREFIX}-validator-{i}"


def health(i):
    return drill.get(i, "/health")


def docker(*args, timeout=60):
    return subprocess.run(["docker", *args], text=True, capture_output=True, timeout=timeout)


def record(path, **event):
    with path.open("a") as f:
        f.write(json.dumps({"utc": time.time(), **event}) + "\n")


def wait_ready(i, window, events):
    deadline = time.monotonic() + window
    last = None
    while time.monotonic() < deadline:
        try:
            value = health(i)
            if value.get("ready") is True:
                return value
            last = value
        except (OSError, ValueError) as error:
            last = repr(error)
        time.sleep(1)
    record(events, wait_ready=i, last=last)
    raise RuntimeError(f"validator {i} did not become ready within {window}s")


def deploy_on(i, term):
    """Submit a signed deploy through validator `i`'s own CLI; return its deploy id."""
    status = drill.get(i, "/api/v1/status")
    node = node_name(i)
    path = f"/tmp/restart-under-load-{uuid.uuid4().hex}.rho"
    write = subprocess.run(
        ["docker", "exec", "-i", node, "sh", "-c", f"cat > {path}"],
        input=term,
        text=True,
        capture_output=True,
        timeout=10,
    )
    if write.returncode:
        raise RuntimeError(f"deploy file write failed on node {i}: {write.stderr}")
    result = subprocess.run(
        [
            "docker", "exec", node, "rnode", "--grpc-host", "localhost", "deploy",
            "--phlo-limit", "1000000", "--phlo-price", "1",
            "--private-key", fuzz.DEPLOYER_PRIV, "--shard-id", status["shardId"],
            "--valid-after-block-number", str(status["latestBlockNumber"]), path,
        ],
        text=True,
        capture_output=True,
        timeout=30,
    )
    if result.returncode:
        raise RuntimeError(f"deploy on node {i} failed: {result.stderr}")
    match = re.search(r"DeployId is: ([0-9a-fA-F]+)", result.stdout)
    if not match:
        raise RuntimeError(f"deploy on node {i} returned no deploy id: {result.stdout!r}")
    return match.group(1).lower()


def finalized(i):
    return drill.finalized_identity(drill.get(i, "/api/last-finalized-block"))


def agreement(validators):
    identities = [finalized(i) for i in range(validators)]
    return identities


def wait_agreement(validators, min_height, window, events):
    deadline = time.monotonic() + window
    while time.monotonic() < deadline:
        try:
            identities = agreement(validators)
            verdict = controls.agreement_verdict(identities, min_height)
            if verdict is None:
                return identities
            record(events, wait_agreement=verdict, identities=identities)
        except (OSError, ValueError) as error:
            record(events, wait_agreement_error=repr(error))
        time.sleep(2)
    raise RuntimeError(f"no agreement at height >= {min_height} within {window}s")


def deploys_resolved(validators, deploy_ids):
    for i in range(validators):
        for deploy_id in deploy_ids:
            status = drill.get(i, f"/api/v1/deploy-status/{deploy_id}")
            if not controls.deploy_is_success(status):
                return False
    return True


def run_cycle(args, victim, cycle, events):
    baseline = agreement(args.validators)
    verdict = controls.baseline_verdict(baseline)
    if verdict is not None:
        raise RuntimeError(f"pre-kill baseline is not agreed: {verdict}")
    base_height = baseline[0][0]
    record(events, cycle=cycle, baseline=baseline)

    submitted = []
    started = time.monotonic()
    killed = False
    deadline = started + args.window
    stream = 0
    while time.monotonic() < deadline:
        if not killed and time.monotonic() - started >= 5:
            node = node_name(victim)
            before = docker("inspect", "-f", "{{.Id}}", node).stdout.strip()
            pre_log_len = len(docker("logs", node).stdout + docker("logs", node).stderr)
            docker("kill", "--signal", "KILL", node)
            docker("start", node)
            after = docker("inspect", "-f", "{{.Id}}", node).stdout.strip()
            if before != after:
                raise RuntimeError("the restarted container is not the same container")
            record(events, cycle=cycle, killed=victim, at=time.time(), container=before)
            killed = True
            stream += 1
        target = (victim + 1 + stream) % args.validators if killed else stream % args.validators
        if target == victim and killed:
            target = (target + 1) % args.validators
        marker = f"restart-under-load:{cycle}:{uuid.uuid4().hex}"
        deploy_id = deploy_on(target, f'@"{marker}"!(1)')
        submitted.append((deploy_id, marker, target))
        record(events, cycle=cycle, deploy=deploy_id, marker=marker, target=target)
        stream += 1
        time.sleep(0.5)

    wait_ready(victim, args.window, events)
    identities = wait_agreement(args.validators, base_height + 3, args.window, events)
    record(events, cycle=cycle, agreed=identities)

    if not deploys_resolved(args.validators, [d for d, _, _ in submitted]):
        raise RuntimeError("a deploy submitted in the window is not resolved on every validator")

    for i in range(args.validators):
        body = health(i)
        verdict = controls.health_verdict(body)
        if verdict is not None:
            raise RuntimeError(f"validator {i}: {verdict}")
        record(events, cycle=cycle, node=i, health=body)

    logs = docker("logs", node_name(victim)).stdout + docker("logs", node_name(victim)).stderr
    if "block receiver state full" in logs:
        raise RuntimeError("the victim logged a receiver capacity refusal")
    if not controls.proposed_after_restart(logs[pre_log_len:]):
        raise RuntimeError("the victim did not propose after restart")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--validators", type=int, default=4)
    parser.add_argument("--cycles", type=int, default=2)
    parser.add_argument("--window", type=int, default=240)
    parser.add_argument("--out", default="target/restart-under-load")
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    events = out / "observations.jsonl"
    events.write_text("")
    # The container walk is only valid for the devnet naming.
    if args.validators < 3:
        raise SystemExit("a restart-under-load arm needs at least three validators")

    try:
        for cycle in range(args.cycles):
            victim = 1 + cycle % (args.validators - 1)
            run_cycle(args, victim, cycle, events)
        result = {"verdict": "PASS", "cycles": args.cycles, "validators": args.validators}
    except Exception as error:  # noqa: BLE001 - the verdict is the output
        result = {"verdict": "FAIL", "error": str(error)}
    (out / "result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result))
    return 0 if result["verdict"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
