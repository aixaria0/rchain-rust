#!/usr/bin/env python3
"""Read-only N=8 partition forensics: reconstruct the DAG's liveness window.

This is NOT a finalizer replay, stake-support proof or release-acceptance test.
It reads source-pinned accepted-block captures produced by consensus-causal-trace.py.
"""
import argparse
import importlib.util
import json
from collections import Counter
from pathlib import Path


WINDOW = 5  # block-storage/src/dag/liveness.rs::LIVENESS_WINDOW


def load_trace():
    path = Path(__file__).with_name("consensus-causal-trace.py")
    spec = importlib.util.spec_from_file_location("consensus_causal_trace", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def parent_window(block, index, trace):
    """Mirror the liveness *input* derived from this block's justifications.

    Does not calculate next-fringe support. No stake is declared finalized here.
    """
    parents = [index[h] for h in block["justifications"]]
    if not parents:
        raise ValueError("no parent set for inspected block")
    bonds = trace.unique_bonds(block["bonds"])
    tip = max(parent["blockNumber"] for parent in parents)
    latest = {}
    for parent in parents:
        author = parent["sender"]
        height = parent["blockNumber"]
        latest[author] = max(latest.get(author, -1), height)
    live = {s: stake for s, stake in bonds.items()
            if s in latest and tip - latest[s] <= WINDOW}
    total = sum(bonds.values())
    live_stake = sum(live.values())
    return {"block_height": block["blockNumber"],
            "block_hash": block["blockHash"],
            "proposer": block["sender"],
            "parent_tip": tip,
            "live_sender_count": len(live),
            "live_stake": live_stake,
            "bonded_stake": total,
            "can_form_full_stake_quorum": 3 * live_stake > 2 * total,
            "stale_parent_senders": sorted(s for s in latest if s in bonds and s not in live)}


def summarize_capture(capture, min_height=0):
    """Fail closed on missing accepted blocks, bonds or parent dependencies."""
    trace = load_trace()
    blocks = capture.get("blocks")
    index = trace.index_blocks(blocks)
    if not index:
        raise ValueError("empty capture")
    missing = sorted({p for b in index.values()
                      for p in b["justifications"] if p not in index})
    if missing:
        raise ValueError(f"capture has {len(missing)} missing justification blocks")
    if type(min_height) is not int or min_height < 0:
        raise ValueError("minimum height must be a nonnegative integer")
    rows = [parent_window(b, index, trace) for b in index.values()
            if b["blockNumber"] >= min_height and b["justifications"]]
    if not rows:
        raise ValueError("no inspected blocks in requested height range")
    rows.sort(key=lambda r: (r["block_height"], r["block_hash"]))
    tips = {}
    for block in index.values():
        sender = block["sender"]
        if sender not in tips or (block["seqNum"], block["blockNumber"]) > (
                tips[sender]["seq"], tips[sender]["height"]):
            tips[sender] = {"height": block["blockNumber"], "seq": block["seqNum"]}
    distribution = Counter(b["sender"] for b in index.values()
                           if b["blockNumber"] >= min_height)
    total = sum(distribution.values())
    return {
        "status": "DIAGNOSTIC_ONLY_NOT_RELEASE_ACCEPTANCE",
        "capture_node": capture.get("node"),
        "min_height": min_height,
        "liveness_window_heights": WINDOW,
        "sampled_accepted_blocks": len(rows),
        "sender_tip_heights": dict(sorted((s[:12], r["height"]) for s, r in tips.items())),
        "blocks_by_sender_since_min_height": dict(sorted(
            (s[:12], n) for s, n in distribution.items())),
        "single_live_sender_parent_sets": sum(r["live_sender_count"] == 1 for r in rows),
        "below_full_stake_quorum_parent_sets": sum(
            not r["can_form_full_stake_quorum"] for r in rows),
        "live_sender_count_histogram": dict(sorted(
            Counter(r["live_sender_count"] for r in rows).items())),
        "highest_sender_share_of_captured_blocks": max(distribution.values()) / total,
        "last_twelve_parent_sets": [
            {**{k: r[k] for k in ("block_height", "proposer", "parent_tip",
                                  "live_sender_count", "live_stake", "bonded_stake",
                                  "can_form_full_stake_quorum")},
             "proposer": r["proposer"][:12]}
            for r in rows[-12:]],
        "boundary": (
            "Source-window reconstruction from accepted block headers; not "
            "a finalizer-support calculation, signature verification, consensus "
            "safety proof, or evidence that every bonded peer was online.")
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", type=Path, required=True,
                        help="Existing causal/node-N.json; never modifies source")
    parser.add_argument("--min-height", type=int, default=0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = summarize_capture(json.loads(args.capture.read_text()), args.min_height)
    data = json.dumps(output, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(data)
    print(data, end="")


if __name__ == "__main__":
    main()
