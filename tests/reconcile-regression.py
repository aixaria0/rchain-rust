#!/usr/bin/env python3
"""Offline full-script reconciliation regressions with deterministic API fixtures."""
import os
from pathlib import Path
import subprocess
import tempfile

SCRIPT = Path(__file__).resolve().parents[1] / "tools/reconcile-network.sh"
def run_case(name, finalized, expected, required):
    # Exercise the production anchor selection in isolation with injected API snapshots.
    source = SCRIPT.read_text()
    a = source.index('echo "== 3. finalized anchor')
    b = source.index("# --- 4. what is above", a)
    fragment = source[a:b]
    shell = """set -uo pipefail
NAMES=(A B)
declare -A LFB LFH
LFB[A]="$A_HEIGHT"; LFH[A]="$A_HASH"
LFB[B]="$B_HEIGHT"; LFH[B]="$B_HASH"
""" + fragment
    def fields(v):
        if isinstance(v, dict):
            return str(v.get("blockNumber", "")), v.get("blockHash", "")
        return "", ""
    ah, ab = fields(finalized[0]); bh, bb = fields(finalized[1])
    env = {**os.environ, "A_HEIGHT": ah, "A_HASH": ab, "B_HEIGHT": bh, "B_HASH": bb}
    p = subprocess.run(["bash", "-c", shell], env=env, capture_output=True, text=True, timeout=10)
    combined = p.stdout + p.stderr
    assert p.returncode == expected, f"{name}: exit={p.returncode}, expected={expected}\n{combined}"
    assert required in combined, f"{name}: missing {required!r}\n{combined}"
    print(f"PASS {name} (exit {p.returncode})")

if __name__ == "__main__":
    anchor = {"blockNumber": 0, "blockHash": "anchor"}
    run_case("unanimous anchor", [anchor, anchor], 0, "unanimously reported finalized anchor")
    run_case("divergent finalized hashes",
             [anchor, {"blockNumber": 0, "blockHash": "other"}],
             4, "finalized heads differ")
    run_case("unavailable finality",
             [anchor, "Finalized fringe is not available."],
             4, "no usable finalized block")
    run_case("same hash at different finalized heights",
             [anchor, {"blockNumber": 1, "blockHash": "anchor"}],
             4, "finalized heads differ")
    # Same first block but a different second block must not be reported as convergence.
    # Verification is behind --apply, so test the actual set-comparison fragment below.
    fragment = SCRIPT.read_text()
    start = fragment.index('  for (( h=MEET; h<=MAXH; h++ )); do', fragment.index('== 9. verification'))
    end = fragment.index('  echo "  [$i]', start)
    verify = fragment[start:end]
    with tempfile.TemporaryDirectory(prefix="reconcile-dag-") as d:
        for case, left, right, expected in [
            ("equal sets different order", "a\\nb", "b\\na", 1),
            ("extra sibling block", "a\\nb", "a", 0),
            ("different second block", "a\\nb", "a\\nc", 0),
            ("missing height", "a", "", 0),
        ]:
            script = """set -uo pipefail
MEET=0; MAXH=0; ok=1
NAMES=(A B)
declare -A HOST PORT
HOST[A]=A; HOST[B]=B; PORT[A]=1; PORT[B]=2
blocks_at() { if [ "$1" = A ]; then printf '%b\\n' "$LEFT"; else printf '%b\\n' "$RIGHT"; fi; }
""" + verify + "\nprintf '%s' \"$ok\"\n"
            p = subprocess.run(["bash", "-c", script], env={**os.environ, "LEFT": left, "RIGHT": right},
                               text=True, capture_output=True, timeout=10)
            assert p.returncode == 0 and p.stdout == str(expected), (case, p.stdout, p.stderr)
            print(f"PASS {case}")
