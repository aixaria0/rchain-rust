#!/usr/bin/env python3
"""Bounded silent-validator drill; isolated runner only, no helper deploys."""
import argparse
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("acceptance", ROOT / "tools/consensus-acceptance-drill.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def escape_timings(text):
    return [int(x) for x in re.findall(r"round gate escaped after \d+ attempt\(s\) and (-?\d+) ms blocked", text)]


def attestation_decisions(text):
    """Count observed proposer decisions; absence of a log is never proof of a decision."""
    rows = re.findall(r"attestation at tip \d+: (licensed|withheld) \u2014 ([^\n]+)", text)
    return {
        "licensed": sum(kind == "licensed" for kind, _ in rows),
        "withheld": sum(kind == "withheld" for kind, _ in rows),
        "quorum_reachable_false": sum("quorum_reachable=false" in detail for _, detail in rows),
        "quorum_reachable_true": sum("quorum_reachable=true" in detail for _, detail in rows),
    }


def tap_decisions(text):
    """Count only explicit logged tap decisions, preserving observed age ranges."""
    rows = re.findall(
        r"attest-tap decision=(skip_freshness|skip_sender_gate|queued) hash=[0-9a-f]+[^\n]*? age_ms=(-?\d+)",
        text,
    )
    counts = {key: 0 for key in ("skip_freshness", "skip_sender_gate", "queued")}
    ages = {key: [] for key in counts}
    for decision, age in rows:
        counts[decision] += 1
        ages[decision].append(int(age))
    return {"counts": counts, "age_ranges_ms": {
        key: [min(values), max(values)] if values else None
        for key, values in ages.items()
    }}


def operator_evidence(status):
    """Only report operator-observed values, without interpreting network topology as reachability."""
    keys = ("latestBlockNumber", "peers", "finalityStall", "finalityStallEpisodes",
            "undeliveredSends", "consecutiveSelfValidationFailures", "tasksPanicked",
            "autoproposeTapSuppressed")
    return {key: status.get(key) for key in keys}


def run(args):
    drill = base.Drill(args)
    silent = args.validators - 1
    active = list(range(silent))
    mode = getattr(args, "retention_mode", "legacy")
    if mode not in ("legacy", "quiet", "epoch-work"):
        raise ValueError(f"unsupported retention mode: {mode}")
    paused = False
    log_since = None
    failure = None
    final = None
    retained = None
    retention_target = None
    first_epoch_work = None
    epoch_work_deploys = 0
    first_epoch_work_finalized = False
    evidence = {}
    operator_latest = {}
    operator_height_ranges = {}
    drill.record(tree=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                 validators=args.validators, silent_validator=silent, active=active,
                 window=args.window, marker=drill.marker, expected=drill.value,
                 helper_deploys=0, autopropose=False, retention_mode=mode)
    try:
        # Refuse to measure a network with an unexpected proposer configuration.
        for i in range(args.validators):
            status = base.get(i, "/api/v1/status")
            if status.get("autopropose") is not False or status.get("proposeOnDeploy") is not True:
                raise RuntimeError(f"validator {i} has unexpected proposer flags")
            blocks = base.get(i, "/api/blocks/1")
            _, block_hash = base.block_identity(blocks[0])
            answer = base.probe(i, drill.marker, block_hash)
            drill.record(node=i, absent_marker_at=block_hash, response=answer)
            if answer.get("expr") != []:
                raise RuntimeError("absent-marker control failed")
        subprocess.run(["docker", "pause", base.node_name(silent)], check=True, timeout=10)
        paused = True
        log_since = datetime.now(timezone.utc).isoformat()
        drill.record(event="validator_paused", node=silent, log_since=log_since, active_stake=args.validators - 1,
                     total_stake=args.validators)
        drill.deploy(f'@{json.dumps(drill.marker)}!({drill.value})')
        deadline = time.monotonic() + args.window
        while time.monotonic() < deadline:
            identities = []
            for i in active:
                status = base.get(i, "/api/v1/status/operator", admin=True)
                operator_latest[str(i)] = operator_evidence(status)
                height = status.get("latestBlockNumber")
                if isinstance(height, int) and not isinstance(height, bool):
                    progress = operator_height_ranges.setdefault(str(i), [height, height])
                    progress[0] = min(progress[0], height)
                    progress[1] = max(progress[1], height)
                try:
                    response = base.get(i, "/api/last-finalized-block")
                    height, block_hash = base.finalized_identity(response)
                    answer = base.probe(i, drill.marker, block_hash)
                    drill.record(node=i, status=status, finalized=response,
                                 finalized_marker_at=block_hash, response=answer)
                    if height >= 1 and base.marker_present(answer, drill.value):
                        identities.append({"node": i, "height": height, "hash": block_hash,
                                           "post_state": response["blockInfo"]["postStateHash"]})
                except (OSError, ValueError) as error:
                    drill.record(node=i, read_error=repr(error),
                                 response_body=error.read().decode() if hasattr(error, "read") else None,
                                 status=status)
            if (len(identities) == len(active)
                    and len({(x["hash"], x["post_state"]) for x in identities}) == 1):
                final = identities
                break
            time.sleep(2)
        if final is None:
            raise RuntimeError("active validators did not finalize the marker within the window")
        # Keep the existing strict N=8 gate as the default and visibly red if
        # a genuinely idle chain cannot cross two more finalized epochs.
        # The independent quiet witness checks retention, without changing
        # quorum, proposing rules, or manufacturing blocks. Epoch-work is
        # explicitly separate and submits fresh signed state-bearing writes.
        retention_target = ((max(x["height"] for x in final) // 10 + 2) * 10
                            if (mode == "epoch-work" or
                                (mode == "legacy" and args.validators == 8)) else None)
        retention_started = time.monotonic()
        deadline = retention_started + args.window
        next_epoch_work = retention_started
        while time.monotonic() < deadline:
            if mode == "epoch-work" and time.monotonic() >= next_epoch_work:
                if epoch_work_deploys >= 48:
                    raise RuntimeError("bounded signed epoch-work budget exhausted")
                epoch_work_deploys += 1
                channel = f"{drill.marker}:epoch-work:{epoch_work_deploys}"
                value = 1000 + epoch_work_deploys
                if first_epoch_work is None:
                    # Negative control: no finalized marker snapshot can
                    # already contain the fresh, never-before-submitted write.
                    for identity in final:
                        before = base.probe(identity["node"], channel, identity["hash"])
                        drill.record(node=identity["node"], epoch_work_absent_at=identity["hash"],
                                     channel=channel, response=before)
                        if not isinstance(before, dict) or before.get("expr") != []:
                            raise RuntimeError("signed epoch-work negative control failed")
                    first_epoch_work = (channel, value)
                drill.deploy(f'@{json.dumps(channel)}!({value})')
                drill.record(event="signed_epoch_work_submitted", channel=channel,
                             value=value, index=epoch_work_deploys)
                next_epoch_work = time.monotonic() + 5
            identities = []
            for identity in final:
                i = identity["node"]
                response = base.get(i, "/api/last-finalized-block")
                height, current_hash = base.finalized_identity(response)
                answer = base.probe(i, drill.marker, current_hash)
                original = base.probe(i, drill.marker, identity["hash"])
                drill.record(node=i, retention_height=height, retention_target=retention_target,
                             current_finalized_hash=current_hash, response=answer,
                             original_finalized_snapshot=identity["hash"], original_response=original)
                if not base.marker_present(answer, drill.value) or not base.marker_present(original, drill.value):
                    raise RuntimeError(f"validator {i} lost the finalized marker")
                identities.append({"node": i, "height": height, "hash": current_hash,
                                   "post_state": response["blockInfo"]["postStateHash"]})
            agreed = len({(x["hash"], x["post_state"]) for x in identities}) == 1
            reached = retention_target is None or all(
                x["height"] >= retention_target for x in identities)
            if mode == "epoch-work" and agreed and reached and first_epoch_work is not None:
                channel, value = first_epoch_work
                witnessed = []
                for identity in identities:
                    answer = base.probe(identity["node"], channel, identity["hash"])
                    drill.record(node=identity["node"], epoch_work_finalized_at=identity["hash"],
                                 epoch_work_channel=channel, response=answer)
                    witnessed.append(base.marker_present(answer, value))
                first_epoch_work_finalized = all(witnessed)
            if (reached and agreed and time.monotonic() - retention_started >= 20
                    and (mode != "epoch-work" or first_epoch_work_finalized)):
                retained = identities
                break
            time.sleep(2)
        if retained is None:
            if mode == "epoch-work":
                raise RuntimeError("signed-work durability did not cross two further finalized epoch boundaries with identical marker and work state")
            if retention_target is not None:
                raise RuntimeError("marker retention did not cross two further epoch boundaries without helper traffic")
            raise RuntimeError("marker retention did not complete the quiet observation interval")
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    finally:
        try:
            base.causal_trace.capture(drill, base.get, base.node_name, nodes=active)
        except Exception as error:
            failure = failure or f"active DAG capture failed: {error}"
        # Collect active-node logs while the silent validator is still paused:
        # recovery traffic cannot be mistaken for the quiet arm's observations.
        for i in active:
            try:
                log_args = ["docker", "logs"] + (["--since", log_since] if log_since else [])
                r = subprocess.run([*log_args, base.node_name(i)],
                                   capture_output=True, text=True, timeout=15, check=True)
                text = r.stdout + r.stderr
                path = drill.out / f"quiet-node-{i}.log"
                path.write_text(text)
                if re.search(r"Self-created block .*failed validation|equivocation detected|task panicked", text, re.I):
                    failure = failure or f"consensus validation/equivocation/task failure on {i}"
                evidence[str(i)] = {"escape_ms": escape_timings(text),
                                    "not_due_observed": "NotEnoughNewBlocks" in text,
                                    "attestation_decisions": attestation_decisions(text),
                                    "tap_decisions": tap_decisions(text)}
                base.causal_trace.emit_capture(path)
            except Exception as error:
                failure = failure or f"log capture failed on {i}: {error}"
        if paused:
            try:
                subprocess.run(["docker", "unpause", base.node_name(silent)], check=True, timeout=10)
            except Exception as error:
                failure = failure or f"cleanup unpause failed: {error}"
    escapes = [ms for row in evidence.values() for ms in row["escape_ms"]]
    if any(ms < 15000 for ms in escapes):
        failure = failure or "escape occurred before the 15000ms bound"
    result = "fail" if failure else ("pass" if escapes else "inconclusive")
    summary = {"result": result, "validators": args.validators, "silent_validator": silent,
               "active_finalized": final, "retained_finalized": retained,
               "retention_target": retention_target, "retention_min_seconds": 20,
               "retention_mode": mode, "signed_epoch_work_deploys": epoch_work_deploys,
               "first_epoch_work_finalized": first_epoch_work_finalized,
               "retention_rule": ("two_finalized_epochs_with_signed_state_work" if mode == "epoch-work"
                                  else "two_finalized_epoch_boundaries" if retention_target is not None
                                  else "quiet_interval"),
               "escape_evidence": evidence, "helper_deploys": epoch_work_deploys,
               "operator_latest": operator_latest, "operator_height_ranges": operator_height_ranges,
               "reason": failure or (None if escapes else "stall/escape path was not exercised"),
               "boundary": "finite quiet/silent-validator witness; no rejoin or universal liveness claim"}
    (drill.out / "result.json").write_text(json.dumps(summary, indent=2) + "\n")
    print("QUIET_STALL_RESULT " + json.dumps(summary), flush=True)
    if result != "pass":
        raise SystemExit(2 if result == "inconclusive" else 1)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validators", type=int, choices=[4, 8], required=True)
    parser.add_argument("--window", type=int, default=240)
    parser.add_argument("--out", required=True)
    parser.add_argument("--retention-mode", choices=("legacy", "quiet", "epoch-work"),
                        default="legacy", help="legacy strict N8 gate, independent quiet retention, or signed-work epoch durability")
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("DEVNET_PREFIX") != "acceptance":
        parser.error("requires the isolated acceptance runner and disposable acceptance network")
    if not 30 <= args.window <= 600:
        parser.error("window must be between 30 and 600 seconds")
    run(args)
