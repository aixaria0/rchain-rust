#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import sys
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "tools/post-rebuild-node-acceptance.py"
spec = importlib.util.spec_from_file_location("post_rebuild", SCRIPT)
mod = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = mod
spec.loader.exec_module(mod)

COMMIT = "a" * 40
VERSION = f"RChain Node 0.1.0 ({COMMIT})"


def row(url="http://127.0.0.1:40403", height=100, read=12, result="values"):
    return mod.ProbeRow(url=url, height=height, finalized=max(0, height - 1),
                        read_ms=read, noop_ms=10, delta_ms=read - 10, result=result)


class ParseTests(unittest.TestCase):
    def test_extract_commit_is_exact(self):
        self.assertEqual(mod.extract_commit(VERSION), COMMIT)
        self.assertIsNone(mod.extract_commit("RChain Node 0.1.0 (commit # unknown)"))

    def test_probe_row_parses_values_and_empty_expr(self):
        text = (
            "node height final read med no-op med delta result\n"
            "http://127.0.0.1:40403 1839 1835 44 40 4 values\n"
            "http://127.0.0.1:43403 1 ? 34 33 1 EMPTY expr\n"
        )
        got = mod.parse_probe(text, "http://127.0.0.1:40403")
        self.assertEqual((got.height, got.finalized, got.read_ms, got.result), (1839, 1835, 44, "values"))
        got = mod.parse_probe(text, "http://127.0.0.1:43403")
        self.assertEqual((got.height, got.finalized, got.result), (1, None, "EMPTY expr"))


class DecisionTests(unittest.TestCase):
    def decide(self, subject=None, control=None, versions=None, factor=3, max_control_height=1):
        return mod.decide(
            expected_commit=COMMIT,
            versions=versions or {"subject": VERSION, "control": VERSION},
            rows={"subject": subject or row(read=20),
                  "control": control or row(url="http://127.0.0.1:43403", height=1, read=10)},
            control_name="control", subject_name="subject",
            max_control_height=max_control_height, max_factor=factor,
        )

    def test_pass_is_same_build_fresh_values_and_within_factor(self):
        got = self.decide()
        self.assertEqual(got.verdict, "PASS")
        self.assertEqual(got.ratio, 2.0)

    def test_high_ratio_is_fail_not_blocked(self):
        got = self.decide(subject=row(read=31), factor=3)
        self.assertEqual(got.verdict, "FAIL")
        self.assertGreater(got.ratio, 3)

    def test_build_mismatch_is_blocked(self):
        other = "b" * 40
        got = self.decide(versions={"subject": VERSION, "control": f"RChain Node 0.1.0 ({other})"})
        self.assertEqual(got.verdict, "BLOCKED")
        self.assertTrue(any("reviewed" in r or "not on one exact build" in r for r in got.reasons))

    def test_unknown_build_is_blocked(self):
        got = self.decide(versions={"subject": "RChain Node 0.1.0 (commit # unknown)", "control": VERSION})
        self.assertEqual(got.verdict, "BLOCKED")
        self.assertTrue(any("unknown" in r for r in got.reasons))

    def test_empty_read_is_blocked(self):
        got = self.decide(subject=row(result="EMPTY expr"))
        self.assertEqual(got.verdict, "BLOCKED")
        self.assertTrue(any("not values" in r for r in got.reasons))

    def test_nonfresh_control_is_blocked(self):
        got = self.decide(control=row(url="http://127.0.0.1:43403", height=2, read=10), max_control_height=1)
        self.assertEqual(got.verdict, "BLOCKED")
        self.assertTrue(any("fresh-control" in r for r in got.reasons))


if __name__ == "__main__":
    unittest.main()
