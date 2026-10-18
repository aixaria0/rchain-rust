#!/usr/bin/env python3
"""Fail-closed verdict checks for the quiet/silent-validator runner."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("quiet", ROOT / "tools/consensus-quiet-drill.py")
quiet = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quiet)


class QuietVerdicts(unittest.TestCase):
    def campaign(self, elapsed, lose_marker=False):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            args = SimpleNamespace(validators=4, window=30, out=directory)
            drill = SimpleNamespace(out=out, marker="unique-marker", value=42,
                                    record=lambda **event: None)
            submitted = []
            drill.deploy = submitted.append
            probes = 0

            def probe(*args):
                nonlocal probes
                probes += 1
                if probes <= 4 or (lose_marker and probes > 7):
                    return {"expr": []}
                return {"expr": [{"ExprInt": {"data": 42}}]}

            def get(node, route, **kwargs):
                if route == "/api/v1/status":
                    return {"autopropose": False, "proposeOnDeploy": True}
                if route == "/api/blocks/1":
                    return [{"blockNumber": 0, "blockHash": "0" * 64}]
                return {}

            log = "" if elapsed is None else f"round gate escaped after 2 attempt(s) and {elapsed} ms blocked"
            operations = []

            def command(argv, **kwargs):
                operations.append(argv)
                return subprocess.CompletedProcess(argv, 0, stdout=log, stderr="")

            code = 0
            with patch.object(quiet.base, "Drill", return_value=drill), \
                 patch.object(quiet.base, "get", side_effect=get), \
                 patch.object(quiet.base, "probe", side_effect=probe), \
                 patch.object(quiet.base, "finalized_identity", return_value=(2, "a" * 64)), \
                 patch.object(quiet.base.causal_trace, "emit_capture"), \
                 patch.object(quiet.subprocess, "run", side_effect=command), \
                 patch.object(quiet.subprocess, "check_output", return_value="test-sha"), \
                 patch.object(quiet.time, "sleep"), contextlib.redirect_stdout(io.StringIO()):
                try:
                    quiet.run(args)
                except SystemExit as error:
                    code = error.code
            summary = json.loads((out / "result.json").read_text())
            self.assertEqual(len(submitted), 1)
            self.assertNotIn("Nil", submitted)
            self.assertIn(["docker", "unpause", quiet.base.node_name(3)], operations)
            return code, summary["result"]

    def test_unexercised_escape_is_not_a_pass(self):
        self.assertEqual(self.campaign(None), (2, "inconclusive"))

    def test_premature_escape_fails_even_with_finalized_marker(self):
        self.assertEqual(self.campaign(14999), (1, "fail"))

    def test_exact_elapsed_boundary_can_pass(self):
        self.assertEqual(self.campaign(15000), (0, "pass"))

    def test_lost_marker_fails_even_with_valid_escape(self):
        self.assertEqual(self.campaign(16000, lose_marker=True), (1, "fail"))


if __name__ == "__main__":
    unittest.main()
