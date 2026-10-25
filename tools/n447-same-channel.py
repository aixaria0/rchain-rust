#!/usr/bin/env python3
"""Issue #447: two real validators race to update one linear Rholang cell.

The node, network, stakes and keys must belong to a disposable Docker devnet.
A split is not a safety proof; report observed counter state and rejected ids.
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys
import time
import traceback

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
SPEC = importlib.util.spec_from_file_location("fork_point_447", TOOLS / "n420-fork-point.py")
fork = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fork)
from n447_partition import DockerBackend, SplitNetwork

CONTAINERS = fork.CONTAINERS
CELL = '@"n447_linear_cell"'
A = 'for (@old <- ' + CELL + ') { ' + CELL + '!(old + 1) | @"n447_effect_a"!("A") }'
B = 'for (@old <- ' + CELL + ') { ' + CELL + '!(old + 2) | @"n447_effect_b"!("B") }'


def find_signed_carrier(node, term, minimum_height):
    """Find the containing block by signed term, not by the current tip."""
    # Block inventory can temporarily be a node-status string while sync is
    # rebuilding. Such a response is NOT a carrying block, but should not
    # abort the bounded inclusion polling loop before it sees the real block.
    inventory = fork.blocks(node, 75)
    if not isinstance(inventory, list):
        return None, None
    candidates = []
    for entry in inventory:
        if isinstance(entry, dict) and "blockHash" in entry:
            candidates.append(entry)
        elif isinstance(entry, str) and len(entry) == 64 and all(
                c in "0123456789abcdefABCDEF" for c in entry):
            candidates.append(fork.block_info(node, entry))
        # Any other item is non-block diagnostic material and cannot count
        # toward inclusion. The surrounding bounded deadline still fails.
    for block in sorted(candidates, key=lambda b: int(b["blockNumber"])):
        if (int(block["blockNumber"]) <= minimum_height
                or int(block.get("deployCount", 0)) < 1):
            continue
        body = fork.request(node, "/api/block/" + block["blockHash"])
        if not isinstance(body, dict) or not isinstance(body.get("deploys"), list):
            raise AssertionError(f"malformed block deploy inventory: {body!r}")
        matches = [d["sig"] for d in body["deploys"]
                   if isinstance(d, dict) and d.get("term") == term]
        if len(matches) == 1:
            return block, matches[0]
        if len(matches) > 1:
            raise AssertionError("duplicate matching signed terms in one block")
    return None, None


def expr(container, channel, block_hash):
    term = 'for (@value <- @"' + channel + '") { @"out"!(value) }'
    response = fork.explore(container, term, block_hash, False)
    if not isinstance(response, dict) or not isinstance(response.get("expr"), list):
        raise AssertionError(f"unexpected exploratory response: {response!r}")
    return response["expr"]


def contains_marker(expressions, marker):
    return any(item.get("ExprString", {}).get("data") == marker
               for item in expressions if isinstance(item, dict))


def classify_effects(a, b, rejected, id_a, id_b):
    """Return the sole surviving writer, refusing impossible partial-effect mixes."""
    if len(rejected) != 1 or not rejected.issubset({id_a, id_b}):
        raise AssertionError(f"expected exactly one of two conflicting signatures: {rejected!r}")
    if a == b or not (a or b):
        raise AssertionError(f"exactly one application effect must remain (a={a}, b={b})")
    kept = "A" if a else "B"
    expected_rejected = id_b if a else id_a
    if rejected != {expected_rejected}:
        raise AssertionError("rejectedDeploys does not match the missing application effect")
    return kept


def wait_for_block_on_all(block_hash, seconds=120):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            views = {name: fork.block_info(name, block_hash) for name in CONTAINERS}
            fork.merge_state_agreement(views, CONTAINERS, block_hash)
            return views
        except Exception:
            time.sleep(2)
    raise AssertionError("four-validator merged-block/state agreement not observed")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True)
    parser.add_argument("--timeout", type=int, default=480)
    args = parser.parse_args(argv)
    p = Path(args.out)
    p.parent.mkdir(parents=True, exist_ok=True)
    record = {"result": "INCOMPLETE",
              "boundary": "finite 2+2 conflict on a disposable Docker devnet",
              "steps": []}

    def step(name, data):
        record["steps"].append({"stage": name, "value": data})
        p.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
        print("STAGE:", name, flush=True)

    try:
        fork.wait_all(CONTAINERS, args.timeout)
        baseline = fork.tip(CONTAINERS[0])
        step("genesis", baseline)

        # Seed one ordinary linear datum, which BOTH sides will later consume.
        seed_term = CELL + "!(0)"
        fork.deploy(CONTAINERS[0], seed_term, baseline["blockNumber"])
        deadline = time.monotonic() + min(args.timeout, 90)
        seed = None
        while time.monotonic() < deadline:
            fork.propose(CONTAINERS[0])
            seed, seed_sig = find_signed_carrier(
                CONTAINERS[0], seed_term, baseline["blockNumber"])
            if seed:
                break
            time.sleep(2)
        if seed is None:
            raise AssertionError("unable to seed a linear cell in a carrying block")
        step("seed_signature", seed_sig)
        step("seed", seed)
        for node in CONTAINERS:
            end = time.monotonic() + 60
            while time.monotonic() < end:
                try:
                    assert fork.block_info(node, seed["blockHash"])["blockHash"] == seed["blockHash"]
                    break
                except Exception:
                    time.sleep(2)
            else:
                raise AssertionError(f"seed block not replicated before split: {node}")
        step("seed_replicated", True)

        with SplitNetwork(DockerBackend(), prefix=fork.PREFIX, main=fork.NETWORK) as topology:
            pair_reachability = [
                fork.probe_transport(CONTAINERS[0], CONTAINERS[1]),
                fork.probe_transport(CONTAINERS[1], CONTAINERS[0]),
                fork.probe_transport(CONTAINERS[2], CONTAINERS[3]),
                fork.probe_transport(CONTAINERS[3], CONTAINERS[2]),
            ]
            across = [
                fork.probe_transport(CONTAINERS[0], CONTAINERS[2]),
                fork.probe_transport(CONTAINERS[2], CONTAINERS[0]),
            ]
            step("partition", {"topology": topology.require_partition(),
                               "intra_pair_tcp": pair_reachability,
                               "cross_pair_tcp": across})
            if not all(pair_reachability) or any(across):
                raise AssertionError("network topology is not a true 2+2 split")
            fork.deploy(CONTAINERS[0], A, seed["blockNumber"])
            fork.deploy(CONTAINERS[2], B, seed["blockNumber"])
            fork.propose(CONTAINERS[0])
            fork.propose(CONTAINERS[2])
            end = time.monotonic() + args.timeout
            while time.monotonic() < end:
                tip_a, sig_a = find_signed_carrier(
                    CONTAINERS[0], A, seed["blockNumber"])
                tip_b, sig_b = find_signed_carrier(
                    CONTAINERS[2], B, seed["blockNumber"])
                if (tip_a and tip_b
                    and tip_a["blockNumber"] == tip_b["blockNumber"]
                    and tip_a["blockHash"] != tip_b["blockHash"]):
                    break
                fork.propose(CONTAINERS[0])
                fork.propose(CONTAINERS[2])
                time.sleep(2)
            else:
                raise AssertionError("both conflicting signed deploys did not produce same-height fork tips")
            if sig_a == sig_b:
                raise AssertionError("branch deploy IDs are not distinct")
            step("tips", {"a": tip_a, "b": tip_b,
                          "id_a": sig_a, "id_b": sig_b})

        step("healed", {node: sorted(DockerBackend().members(node)) for node in CONTAINERS})
        target = [tip_a["blockHash"], tip_b["blockHash"]]
        end = time.monotonic() + min(args.timeout, 120)
        while time.monotonic() < end:
            views = fork.observe_cross_tip_gossip(CONTAINERS, target)
            if fork.all_tips_visible(views):
                break
            time.sleep(2)
        else:
            step("gossip_missing", views)
            raise AssertionError("disconnected groups did not exchange their tips after healing")
        step("gossip", views)

        end = time.monotonic() + args.timeout
        merge = None
        while time.monotonic() < end and merge is None:
            fork.propose(CONTAINERS[0])
            merge = fork.find_merge(CONTAINERS[0], tip_a["blockNumber"], target,
                                    time.time() + 15)
        if merge is None:
            raise AssertionError("no multi-parent block justified both conflicting fork tips")
        merge_hash = merge["blockHash"]
        views = wait_for_block_on_all(merge_hash)
        step("merge", {"hash": merge_hash, "views": views})
        rejected = set(merge["rejectedDeploys"]) & {sig_a, sig_b}
        step("rejections", {"all": merge["rejectedDeploys"], "matching": sorted(rejected)})

        states = {}
        for node in CONTAINERS:
            states[node] = {
                "a": expr(node, "n447_effect_a", merge_hash),
                "b": expr(node, "n447_effect_b", merge_hash),
                "cell": expr(node, "n447_linear_cell", merge_hash),
            }
        step("readback", states)
        if len({json.dumps(state, sort_keys=True) for state in states.values()}) != 1:
            raise AssertionError("validator readbacks differ")
        first = states[CONTAINERS[0]]
        survived = classify_effects(
            contains_marker(first["a"], "A"),
            contains_marker(first["b"], "B"),
            rejected, sig_a, sig_b,
        )
        step("winner", survived)
        record["result"] = "PASS"
    except Exception as error:
        record["error"] = f"{type(error).__name__}: {error}"
        record["traceback"] = traceback.format_exc()
        try:
            record["last_block_inventory"] = fork.blocks(CONTAINERS[0], 15)
        except Exception as diagnostic_error:
            record["inventory_error"] = f"{type(diagnostic_error).__name__}: {diagnostic_error}"
        raise
    finally:
        p.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
        print("same-channel verdict", record["result"], "evidence", p, flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        traceback.print_exc()
        sys.exit(1)
