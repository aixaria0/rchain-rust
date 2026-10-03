#!/usr/bin/env python3
"""Compute #214's two re-probe readings, and only those — from the artifacts `n149-sample.py` wrote.

The sampler is shared, unchanged: it records `height`, `finalized` and `alive` per node each second in
`series.tsv`, and maintains the block-hash union in `blocks.tsv`. This program is the *reduction* for
#214's questions (a two-mark criterion-1 arm, and a kill/restart criterion-2 arm). It computes nothing
the sampler did not record, and where a quantity would need an assumption it returns `None` and says so
rather than guessing — a made-up number here would be a C176-class defect.

Usage:  python3 spec/audit/evidence/n214-summarise.py <run-root>
"""

import os
import re
import sys


def read_marks(path):
    marks = {}
    with open(path) as fh:
        for line in fh:
            if "\t" in line:
                k, v = line.rstrip("\n").split("\t", 1)
                marks[k] = v
    return marks


def read_blocks(path):
    """The union: one row per distinct block hash, with the second it was first seen."""
    rows = []
    if not os.path.exists(path):
        return rows
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            t, num, sender, dc, parents, h = line.rstrip("\n").split("\t")
            rows.append({"epoch": int(t), "number": int(num), "sender": sender,
                         "deploy_count": int(dc), "parents": int(parents), "hash": h})
    return rows


def read_series(path):
    """node -> [(epoch, finalized, alive)] in sample order; `finalized` is None when not reported."""
    out = {}
    if not os.path.exists(path):
        return out
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip() or line.startswith("utc\t"):
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 6:
                continue
            _utc, epoch, node, _height, fin, alive = parts
            out.setdefault(node, []).append(
                (int(epoch), int(fin) if fin.isdigit() else None, alive == "1"))
    return out


def blocks_between(blocks, lo, hi):
    """Distinct block hashes first seen in [lo, hi) — the union, never a height delta."""
    return [b for b in blocks if lo <= b["epoch"] < hi]


def finality_at(series, node, epoch):
    """The latest `finalized` this node reported at or before `epoch` (None if it never did)."""
    vals = [f for e, f, _a in series.get(node, []) if e <= epoch and f is not None]
    return max(vals) if vals else None


def time_to_finality(series, deploy_epoch, target_number, alive_nodes):
    """The first second at which EVERY node in `alive_nodes` reports finality at or past the target.

    A node with no sample for a second is not a node that agreed, and a gap is not a sample: the walk
    asks each node only for seconds it actually has, so a hole makes the answer later than the truth
    rather than earlier. Returns seconds after `deploy_epoch`, or None.
    """
    epochs = sorted({e for node in alive_nodes for e, _f, _a in series.get(node, [])
                     if e >= deploy_epoch})
    for e in epochs:
        ok = True
        for node in alive_nodes:
            v = finality_at(series, node, e)
            if v is None or v < target_number:
                ok = False
                break
        if ok:
            return e - deploy_epoch
    return None


def deploy_block_after(blocks, epoch):
    """The first block carrying a deploy, first seen at or after `epoch` — the deploy's own block.

    With `--propose-on-deploy` and no `--autopropose` the only blocks built are those a deploy triggers,
    so a block with `deploy_count >= 1` after the deploy instant is that deploy's block. Recorded as a
    limitation, not a proof: a system deploy riding the same window would satisfy the same test.
    """
    for b in sorted(blocks, key=lambda x: x["epoch"]):
        if b["epoch"] >= epoch and b["deploy_count"] >= 1:
            return b
    return None


def arm_a(dirpath):
    marks = read_marks(os.path.join(dirpath, "marks.tsv"))
    series = read_series(os.path.join(dirpath, "series.tsv"))
    blocks = read_blocks(os.path.join(dirpath, "blocks.tsv"))
    d1, d2 = int(marks["deploy1"]), int(marks["deploy2"])
    end = int(marks["end"])
    alive = sorted(series)
    out = {"arm": "A", "dir": os.path.basename(dirpath),
           "blocks_deploy1": len(blocks_between(blocks, d1, d2)),
           "blocks_deploy2": len(blocks_between(blocks, d2, end)),
           "nodes_sampled": alive}
    for label, dep, hi in (("deploy1", d1, d2), ("deploy2", d2, end)):
        blk = deploy_block_after(blocks, dep)
        out[f"{label}_block"] = blk["number"] if blk else None
        out[f"{label}_ttf_s"] = (time_to_finality(series, dep, blk["number"], alive)
                                 if blk else None)
    # The witness: a pass needs BOTH deploys to finalise, with every sampled node alive at the end.
    alive_at_end = all(any(a for e, _f, a in series[n] if e >= end - 5) for n in alive)
    out["every_node_alive_at_end"] = alive_at_end
    out["verdict"] = ("pass" if (out["deploy1_ttf_s"] is not None
                                 and out["deploy2_ttf_s"] is not None and alive_at_end)
                      else "fail")
    return out


def arm_b(dirpath):
    marks = read_marks(os.path.join(dirpath, "marks.tsv"))
    series = read_series(os.path.join(dirpath, "series.tsv"))
    kill, kill_end = int(marks["kill"]), int(marks["kill_end"])
    restart, rec_end = int(marks["restart"]), int(marks["recover_end"])
    stopped = marks.get("stopped_node", "?")
    survivors = sorted(n for n in series if n != stopped)
    out = {"arm": "B", "dir": os.path.basename(dirpath), "stopped_node": stopped,
           "survivors": survivors}

    def max_fin(nodes, lo, hi):
        vals = [f for n in nodes for e, f, _a in series.get(n, []) if lo <= e <= hi and f is not None]
        return max(vals) if vals else None

    out["fin_at_kill"] = finality_at(series, survivors[0], kill) if survivors else None
    out["fin_in_kill_window"] = max_fin(survivors, kill, kill_end)
    out["fin_in_recovery"] = max_fin(survivors, restart, rec_end)
    survivors_alive = all(any(a for e, _f, a in series.get(n, []) if kill <= e <= kill_end)
                          for n in survivors)
    out["survivors_alive_through_kill"] = survivors_alive
    advanced = (out["fin_in_kill_window"] is not None and out["fin_at_kill"] is not None
                and out["fin_in_kill_window"] > out["fin_at_kill"])
    out["advanced_past_kill"] = advanced
    out["recovered"] = (out["fin_in_recovery"] is not None
                        and out["fin_in_kill_window"] is not None
                        and out["fin_in_recovery"] > out["fin_in_kill_window"])
    if not survivors_alive:
        out["verdict"] = "void — a survivor stopped answering (distinguish a deliberate stop from an OOM)"
    else:
        out["verdict"] = "pass" if (advanced and out["recovered"]) else "fail"
    return out


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "target/n214-blocks"
    print(f"# n214 readings from {root}")
    arms = []
    for d in sorted(os.listdir(root)):
        p = os.path.join(root, d)
        if not os.path.isdir(p) or not os.path.exists(os.path.join(p, "marks.tsv")):
            continue
        with open(os.path.join(p, "series.tsv")) as fh:
            head = "".join(l for l in fh if l.startswith("# "))
        print(f"\n## {d}")
        for line in head.splitlines():
            print(f"  {line}")
        arms.append(arm_a(p) if d.endswith("-a") else arm_b(p))
    for a in arms:
        print(f"\n### {a['arm']} — {a['dir']}")
        for k, v in a.items():
            if k in ("arm", "dir"):
                continue
            print(f"  {k}: {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
