#!/usr/bin/env python3
"""Summarise `n214-tail-lag-run.sh`: is the unfinalised tail a **lag** or a **loss**?

Reads one run directory (the one the rig writes under `target/n214-tail-lag/`) and prints, per phase:

    WALL  the greatest `finalized` while the chain was quiet   (end of phase 2)
    LAG   the greatest `finalized` after one more deploy       (end of phase 3)

and, for each deploy, its block and whether that block was covered at each reading.

The verdict is on the *sixth* deploy and is one of:

    OK-at-wall   its block was already finalised before the extra deploy — A1.5 passes for this arm
    LAG          not finalised at the wall, finalised after one more deploy — a delay, not a loss
    LOSS         not finalised even then — the defect A1.5 exists to catch

Usage: python3 spec/audit/evidence/n214-tail-lag-summarise.py <run-dir>
"""
import sys
from pathlib import Path

SPACING = 12


def rows(path):
    out = []
    for line in Path(path).read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        fields = line.split("\t")
        if fields[0] in ("utc", "first_seen_epoch"):
            continue
        out.append(fields)
    return out


def read(dir_, n_deploys, arm_name):
    d = Path(dir_) / arm_name
    marks = {r[0]: r[1] for r in rows(d / "marks.tsv")}
    first, quiet_end, seventh = (
        int(marks["first_deploy"]),
        int(marks["quiet_end"]),
        int(marks["seventh_deploy"]),
    )
    instants = [first + i * SPACING for i in range(n_deploys)] + [seventh]

    series = rows(d / "series.tsv")  # utc, epoch, node, height, finalized, alive
    fin = lambda upto: max(
        (int(r[4]) for r in series if r[4] not in ("none", "") and int(r[1]) <= upto),
        default=0,
    )
    # **The tip has to be read at each wall too**: it moves between the two phases, so a single
    # final tip would report a gap that never existed.
    tip = lambda upto: max((int(r[3]) for r in series if int(r[1]) <= upto), default=0)
    wall, lag = fin(quiet_end), fin(10**18)
    tip_wall, tip_end = tip(quiet_end), tip(10**18)

    # A deploy's block is the first block first seen at or after its instant with deploy_count >= 1 —
    # the rig's own rule (`n214-rotation-results.md`).
    blocks = rows(d / "blocks.tsv")
    signed = sorted((int(r[0]), int(r[1])) for r in blocks if int(r[3]) >= 1)
    hit = [next((bn for (seen, bn) in signed if seen >= t), None) for t in instants]
    return wall, lag, tip_wall, tip_end, hit


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    n = 6
    root = Path(sys.argv[1])
    arms = [a.name for a in sorted(root.iterdir()) if (a / "marks.tsv").is_file()]
    if not arms:
        print(f"no finished arms under {root}")
        return 1
    rc = 0
    for name in arms:
        if summarise_arm(root, name, n) != 0:
            rc = 1
    return rc


def summarise_arm(root, arm_name, n):
    print(f"=== {arm_name} ===")
    wall, lag, tip_wall, tip_end, hit = read(root, n, arm_name)
    print(f"WALL: tip={tip_wall} finalized={wall} gap={tip_wall - wall}   "
          f"LAG: tip={tip_end} finalized={lag} gap={tip_end - lag}")
    for i, b in enumerate(hit):
        at = "covered" if b is not None and b <= wall else "IN THE TAIL"
        after = "covered" if b is not None and b <= lag else "NOT COVERED"
        print(f"  deploy {i + 1}: block {b}  at the wall: {at}   after the extra deploy: {after}")

    # **The verdict is about the deploys that were actually in the tail**, not about a fixed one: which
    # deploy lands there is the slack's business, and an arm where none did cannot speak to the question.
    tail = [b for b in hit[:n] if b is not None and b > wall]
    if not tail:
        print("VERDICT: NOT EXERCISED — no deploy was in the tail at the wall, so the probe "
              "cannot say whether one would have been rescued")
    elif all(b <= lag for b in tail):
        print(f"VERDICT: LAG — {len(tail)} deploy(s) unfinalised at the wall were finalised once "
              f"the chain produced again")
    else:
        stuck = [b for b in tail if b > lag]
        print(f"VERDICT: LOSS — {len(stuck)} deploy(s) still not finalised after a further deploy: {stuck}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
