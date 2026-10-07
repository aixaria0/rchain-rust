#!/usr/bin/env python3
"""A1.5: is "does the sixth deploy finalise" a *timing* condition rather than a mystery?

The rule is the rig's own (n214-rotation-results.md): a deploy's block is the first block first seen
at or after the deploy instant with `deploy_count >= 1`, and it finalises iff its block_number is
<= the greatest `finalized` the run reached.

The hypothesis under test: the fringe cannot advance without messages from a *later* round, so the
last `LIVENESS_WINDOW` (5) heights of a quiet chain are unfinalisable. If that is the whole story,
then per arm, `finalized` should equal `tip - 5` (or the last deploy's block, whichever is lower), and
whether the sixth deploy lands inside that tail is decided purely by how far production ran past it.
"""
import sys
from pathlib import Path

RUNS = [
    ("07af032ad", "spec/audit/evidence/n213-blocks/07af032ad-20261004T074618Z"),
    ("513e2192b", "spec/audit/evidence/n214-rotation-blocks/513e2192b-20261004T154216Z"),
    ("f1ca009", "spec/audit/evidence/n223-rejoin-blocks/regressions/rotation"),
]

# Any extra run directories named on the command line (the fresh ones, under target/).
for extra in sys.argv[1:]:
    RUNS.append((Path(extra).resolve().name, extra))
SPACING = 12
DEPLOYS = 6


def rows(path):
    out = []
    for line in Path(path).read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        fields = line.split("\t")
        # `series.tsv` carries an unprefixed header after its `#` block; skip it.
        if fields[0] in ("utc", "first_seen_epoch"):
            continue
        out.append(fields)
    return out


def arm(base, tree, arm):
    d = Path(base) / arm
    if not (d / "marks.tsv").is_file() or not (d / "series.tsv").is_file():
        return None  # not finished yet
    marks = {r[0]: r[1] for r in rows(d / "marks.tsv")}
    first = int(marks["first_deploy"])
    deploys = [first + i * SPACING for i in range(DEPLOYS)]

    series = rows(d / "series.tsv")
    # Columns: utc, epoch, node, height, finalized, alive
    finalized = max(
        (int(r[4]) for r in series if r[4] not in ("none", "")), default=0
    )
    tip = max(int(r[3]) for r in series)
    # The tip at rest: the greatest height seen in the last quarter of the window.
    rest_tip = max(int(r[3]) for r in series[len(series) * 3 // 4 :])

    blocks = rows(d / "blocks.tsv")
    # Columns: first_seen_epoch, block_number, sender, deploy_count, parents, block_hash
    signed = sorted(
        (int(r[0]), int(r[1])) for r in blocks if int(r[3]) >= 1
    )
    deploys_blocks = []
    for instant in deploys:
        hit = next((bn for (seen, bn) in signed if seen >= instant), None)
        deploys_blocks.append(hit)

    return {
        "tree": tree,
        "arm": arm,
        "tip": tip,
        "rest_tip": rest_tip,
        "finalized": finalized,
        "blocks": deploys_blocks,
        "finalised": [b is not None and b <= finalized for b in deploys_blocks],
    }


def main():
    print(f"{'run':11} {'arm':4} {'tip':>4} {'rest':>4} {'fin':>4} {'tip-fin':>7} "
          f"{'sixth blk':>9} {'tail?':>6}  deploy blocks")
    for tree, base in RUNS:
        for a in ("R1", "R2", "R3"):
            r = arm(base, tree, a)
            if r is None:
                print(f"{tree:11} {a:4}  (absent)")
                continue
            sixth = r["blocks"][-1]
            gap = r["tip"] - r["finalized"]
            # Is the sixth deploy's block inside the final LIVENESS_WINDOW heights?
            in_tail = sixth is not None and sixth > r["tip"] - 5
            ok = r["finalised"][-1]
            print(f"{tree:11} {a:4} {r['tip']:>4} {r['rest_tip']:>4} {r['finalized']:>4} "
                  f"{gap:>7} {str(sixth):>9} {str(in_tail):>6}  {r['blocks']}  "
                  f"{'OK' if ok else 'SHORT'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
