"""Unit controls for the #447 same-channel application conflict verdict."""

import importlib.util
from pathlib import Path
import sys
import unittest


TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
SPEC = importlib.util.spec_from_file_location("n447_conflict", TOOLS / "n447-same-channel.py")
conflict = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(conflict)


class VerdictControls(unittest.TestCase):
    def test_a_survives_b_rejected(self):
        self.assertEqual(conflict.classify_effects(True, False, {"b"}, "a", "b"), "A")

    def test_b_survives_a_rejected(self):
        self.assertEqual(conflict.classify_effects(False, True, {"a"}, "a", "b"), "B")

    def test_both_effects_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(True, True, {"a"}, "a", "b")

    def test_no_effect_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(False, False, {"a"}, "a", "b")

    def test_missing_rejection_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(True, False, set(), "a", "b")

    def test_both_rejected_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(False, False, {"a", "b"}, "a", "b")

    def test_opaque_unrelated_rejection_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(True, False, {"unknown"}, "a", "b")

    def test_rejection_does_not_match_effect_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            conflict.classify_effects(True, False, {"a"}, "a", "b")

    def test_reads_use_api_captured_output_channel(self):
        from unittest.mock import patch
        with patch.object(conflict.fork, "explore",
                          return_value={"expr": [{"ExprString": {"data": "A"}}]}) as call:
            self.assertEqual(conflict.expr("node", "n447_effect_a", "block"),
                             [{"ExprString": {"data": "A"}}])
        self.assertIn('@"out"!(value)', call.call_args.args[1])

    def test_transient_non_block_inventory_is_not_inclusion(self):
        from unittest.mock import patch
        with patch.object(conflict.fork, "request", return_value=["syncing"]):
            self.assertEqual(
                conflict.find_signed_carrier("node", "signed-term", 0),
                (None, None))

    def test_inclusion_from_verified_block_inventory(self):
        from unittest.mock import patch
        sample = [{"blockHash": "a" * 64, "blockNumber": 1,
                   "deployCount": 1}]
        with patch.object(
            conflict.fork, "request", side_effect=[
                sample, {"deploys": [{"term": "signed-term", "sig": "signature"}]}
            ]
        ):
            block, sig = conflict.find_signed_carrier("node", "signed-term", 0)
        self.assertEqual(block["blockHash"], "a" * 64)
        self.assertEqual(sig, "signature")

    def test_carrier_queries_supported_depth_and_finds_signed_term(self):
        from unittest.mock import patch
        items = [{"blockHash": "h0", "blockNumber": 0, "deployCount": 16},
                 {"blockHash": "h1", "blockNumber": 1, "deployCount": 1}]
        with patch.object(conflict.fork, "request", side_effect=[
            items, {"deploys": [{"term": "signed-cell", "sig": "s1"}]}
        ]) as req:
            block, signature = conflict.find_signed_carrier("n", "signed-cell", 0)
        self.assertEqual(block["blockHash"], "h1")
        self.assertEqual(signature, "s1")
        self.assertEqual(req.call_args_list[0].args[1], "/api/blocks/25")

    def test_block_list_error_does_not_become_no_inclusion(self):
        from unittest.mock import patch
        with patch.object(conflict.fork, "request",
                          return_value={"error": "depth limit is 50"}):
            with self.assertRaisesRegex(AssertionError, "block-list API"):
                conflict.find_signed_carrier("n", "signed-cell", 0)

    def test_winning_update_has_the_expected_linear_cell(self):
        self.assertEqual(conflict.require_single_cell_value(
            [{"ExprInt": {"data": 1}}], "A"), 1)
        self.assertEqual(conflict.require_single_cell_value(
            [{"ExprInt": {"data": 2}}], "B"), 2)

    def test_wrong_or_partial_cell_cannot_pass(self):
        with self.assertRaises(AssertionError):
            conflict.require_single_cell_value([{"ExprInt": {"data": 3}}], "A")
        with self.assertRaises(AssertionError):
            conflict.require_single_cell_value([{"ExprInt": {"data": 1}},
                                                {"ExprInt": {"data": 2}}], "A")
        with self.assertRaises(AssertionError):
            conflict.require_single_cell_value([], "B")

    def test_bool_cannot_masquerade_as_numeric_value(self):
        with self.assertRaises(AssertionError):
            conflict.require_single_cell_value([{"ExprInt": {"data": True}}], "A")

    def test_marker_json_shape(self):
        self.assertTrue(conflict.contains_marker([{"ExprString": {"data": "A"}}], "A"))
        self.assertFalse(conflict.contains_marker([{"ExprString": {"data": "B"}}], "A"))


if __name__ == "__main__":
    unittest.main()
