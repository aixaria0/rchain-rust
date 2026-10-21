#!/usr/bin/env python3
"""Fail-closed unit tests for the read-only partition liveness-window diagnostic."""
import importlib.util
from pathlib import Path
import unittest


PATH = Path(__file__).resolve().parents[1] / "tools/consensus-window-diagnostic.py"
SPEC = importlib.util.spec_from_file_location("consensus_window_diagnostic", PATH)
diagnostic = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(diagnostic)


def identity(n):
    return f"{n:02x}" * 65


def digest(n):
    return f"{n:02x}" * 32


def block(i, author, height, parents, bonds=None):
    if bonds is None:
        bonds = [(1, 100), (2, 100), (3, 100)]
    return {
        "blockHash": digest(i),
        "sender": identity(author),
        "blockNumber": height,
        "seqNum": i,
        "preStateHash": digest(220),
        "postStateHash": digest(221),
        "justifications": [digest(p) for p in parents],
        "bonds": [{"validator": identity(s), "stake": n} for s, n in bonds]
    }


class WindowDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.one = block(1, 1, 10, [])
        self.two = block(2, 2, 5, [])
        self.three = block(3, 3, 4, [])
        self.candidate = block(4, 1, 11, [1, 2, 3])
        self.index = diagnostic.load_trace().index_blocks([
            self.one, self.two, self.three, self.candidate])

    def test_full_bonded_denominator_is_not_reduced_to_live_stake(self):
        result = diagnostic.parent_window(
            self.candidate, self.index, diagnostic.load_trace())
        self.assertEqual((result["live_sender_count"], result["live_stake"],
                          result["bonded_stake"]), (2, 200, 300))
        self.assertFalse(result["can_form_full_stake_quorum"])
        self.assertEqual(result["stale_parent_senders"], [identity(3)])

    def test_horizon_edge_is_inclusive_and_next_height_retired(self):
        b = block(5, 1, 12, [1, 2, 3])
        self.index[b["blockHash"]] = b
        r = diagnostic.parent_window(b, self.index, diagnostic.load_trace())
        self.assertEqual(r["parent_tip"], 10)
        self.assertEqual(r["live_sender_count"], 2)

    def test_strict_supermajority_can_be_reached_by_three(self):
        fresh = block(6, 3, 10, [])
        b = block(7, 1, 11, [1, 2, 6])
        self.index[fresh["blockHash"]] = fresh
        self.index[b["blockHash"]] = b
        r = diagnostic.parent_window(b, self.index, diagnostic.load_trace())
        self.assertEqual(r["live_stake"], 300)
        self.assertTrue(r["can_form_full_stake_quorum"])

    def test_missing_justification_refuses_incomplete_capture(self):
        broken = block(8, 1, 11, [1, 2, 99])
        with self.assertRaisesRegex(ValueError, "missing justification"):
            diagnostic.summarize_capture(
                {"node": 0, "blocks": [self.one, self.two, self.three, broken]},
                min_height=11)

    def test_stake_context_is_per_candidate_not_global(self):
        self.candidate["bonds"] = [
            {"validator": identity(i), "stake": v}
            for i, v in [(1, 150), (2, 150), (3, 100)]]
        r = diagnostic.parent_window(self.candidate, self.index,
                                     diagnostic.load_trace())
        self.assertEqual(r["bonded_stake"], 400)
        self.assertEqual(r["live_stake"], 300)
        self.assertTrue(r["can_form_full_stake_quorum"])

    def test_summary_reports_no_finality_or_safety_claim(self):
        r = diagnostic.summarize_capture(
            {"node": 4, "blocks": [self.one, self.two, self.three,
                                  self.candidate]}, min_height=11)
        self.assertEqual(r["sampled_accepted_blocks"], 1)
        self.assertEqual(r["below_full_stake_quorum_parent_sets"], 1)
        self.assertEqual(r["status"], "DIAGNOSTIC_ONLY_NOT_RELEASE_ACCEPTANCE")
        self.assertIn("not a finalizer-support calculation", r["boundary"])


if __name__ == "__main__":
    unittest.main()
