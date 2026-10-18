#!/usr/bin/env python3
"""Fail-closed verdict checks for the quiet/silent-validator runner."""
import contextlib
import importlib.util
import io
import itertools
import json
import os
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
    def test_disposable_http_ports_track_the_selected_host_offset(self):
        with patch.dict(os.environ, {"DEVNET_PORT_OFFSET": "12000"}):
            with patch.object(quiet.base.fuzz, "http_json", return_value=(200, {"ok": True})) as req:
                self.assertEqual(quiet.base.get(3, "/api/v1/status"), {"ok": True})
                req.assert_called_with("http://127.0.0.1:55403/api/v1/status",
                                       "GET", None, timeout=10)
                quiet.base.get(7, "/api/v1/status/operator", admin=True)
                req.assert_called_with("http://127.0.0.1:59405/api/v1/status/operator",
                                       "GET", None, timeout=10)

    def test_operator_evidence_never_infers_support_from_peers(self):
        state = quiet.operator_evidence({"peers": 7, "finalityStall": "0 of 800",
                                         "undeliveredSends": 9, "tasksPanicked": 0})
        self.assertEqual(state["finalityStall"], "0 of 800")
        self.assertEqual(state["undeliveredSends"], 9)
        self.assertIsNone(state["latestBlockNumber"])

    def test_attestation_summary_retains_both_quorum_outcomes(self):
        lines = ("attestation at tip 3: licensed — quorum_reachable=true paced=true\n"
                 "attestation at tip 4: withheld — quorum_reachable=false paced=false\n")
        self.assertEqual(quiet.attestation_decisions(lines),
                         {"licensed": 1, "withheld": 1,
                          "quorum_reachable_false": 1, "quorum_reachable_true": 1})

    def test_tap_evidence_counts_only_logged_decisions(self):
        lines = ("attest-tap decision=skip_freshness hash=abc123 height=3 block_ms=1 now_ms=2 age_ms=16001\n"
                 "attest-tap decision=skip_sender_gate hash=aabb height=3 last=Some(3) own_sender=false block_ms=3 now_ms=4 age_ms=100\n"
                 "attest-tap decision=queued hash=bbcc height=4 block_ms=5 now_ms=6 age_ms=-10\n")
        result = quiet.tap_decisions(lines)
        self.assertEqual(result["counts"], {"skip_freshness": 1, "skip_sender_gate": 1, "queued": 1})
        self.assertEqual(result["age_ranges_ms"]["skip_freshness"], [16001, 16001])
        self.assertEqual(result["age_ranges_ms"]["queued"], [-10, -10])

    def campaign(self, elapsed, lose_marker=False, retention_height=24):
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
                if route == "/api/v1/status/operator":
                    return {"latestBlockNumber": 7, "peers": 2,
                            "finalityStall": "0 of 400", "undeliveredSends": 3}
                return {"blockInfo": {"postStateHash": "b" * 64}}

            log = "" if elapsed is None else f"round gate escaped after 2 attempt(s) and {elapsed} ms blocked"
            operations = []

            def command(argv, **kwargs):
                operations.append(argv)
                return subprocess.CompletedProcess(argv, 0, stdout=log, stderr="")

            final_reads = 0

            def finalized(response):
                nonlocal final_reads
                final_reads += 1
                return (2 if final_reads <= 3 else retention_height, "a" * 64)

            code = 0
            with patch.object(quiet.base, "Drill", return_value=drill), \
                 patch.object(quiet.base, "get", side_effect=get), \
                 patch.object(quiet.base, "probe", side_effect=probe), \
                 patch.object(quiet.base, "finalized_identity", side_effect=finalized), \
                 patch.object(quiet.base.causal_trace, "emit_capture"), \
                 patch.object(quiet.base.causal_trace, "capture"), \
                 patch.object(quiet.subprocess, "run", side_effect=command), \
                 patch.object(quiet.subprocess, "check_output", return_value="test-sha"), \
                 patch.object(quiet.time, "sleep"), \
                 patch.object(quiet.time, "monotonic", side_effect=itertools.count()), contextlib.redirect_stdout(io.StringIO()):
                try:
                    quiet.run(args)
                except SystemExit as error:
                    code = error.code
            log_reads = [op for op in operations if op[:2] == ["docker", "logs"]]
            self.assertEqual(len(log_reads), 3)
            self.assertTrue(all("--since" in op for op in log_reads))
            summary = json.loads((out / "result.json").read_text())
            self.assertEqual(summary["operator_height_ranges"], {"0": [7, 7], "1": [7, 7], "2": [7, 7]})
            self.assertEqual(summary["operator_latest"]["0"]["undeliveredSends"], 3)
            self.assertIn("attestation_decisions", summary["escape_evidence"]["0"])
            self.assertIn("tap_decisions", summary["escape_evidence"]["0"])
            self.assertEqual(len(submitted), 1)
            self.assertNotIn("Nil", submitted)
            self.assertIn(["docker", "unpause", quiet.base.node_name(3)], operations)
            if retention_height < 20 and not lose_marker:
                self.assertIn("two further epoch boundaries", summary["reason"])
            return code, summary["result"]

    def test_unexercised_escape_is_not_a_pass(self):
        self.assertEqual(self.campaign(None), (2, "inconclusive"))

    def test_premature_escape_fails_even_with_finalized_marker(self):
        self.assertEqual(self.campaign(14999), (1, "fail"))

    def test_exact_elapsed_boundary_can_pass(self):
        self.assertEqual(self.campaign(15000), (0, "pass"))

    def test_time_only_retention_is_not_a_pass(self):
        self.assertEqual(self.campaign(16000, retention_height=2), (1, "fail"))

    def test_lost_marker_fails_even_with_valid_escape(self):
        self.assertEqual(self.campaign(16000, lose_marker=True), (1, "fail"))


if __name__ == "__main__":
    unittest.main()
