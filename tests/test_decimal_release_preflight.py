"""Negative controls for the #373 release preflight; fixture evidence is synthetic."""
import hashlib
import importlib.util
import json
import struct
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "decimal_release_preflight.py"
SPEC = importlib.util.spec_from_file_location("release_gate", SCRIPT)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

NEW = "a" * 40
OLD = "b" * 40
GENESIS = "c" * 64
STATE = "d" * 64
BINARY = "e" * 64


def validator(identity):
    return {
        "identity": identity, "build_commit": NEW,
        "binary_sha256": "sha256:" + BINARY,
        "genesis_block_hash": GENESIS, "genesis_state_hash": STATE,
        "supports_decimal_tag35": True,
    }


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.payload = {
            "schema": 1, "activation": "fresh-genesis-uniform-validator-upgrade",
            "release_commit": NEW,
            "expected_validator_count": 2,
            "validators": [validator("v1"), validator("v2")],
            "new_genesis": {
                "block_hash": GENESIS, "state_hash": STATE,
                "block_version": 1,
                "pos_params_hex": (bytes([1]) + struct.pack("<10q", 1, 100, 10, 5, 2, 100, 7, 13, 19, 255)).hex(),
                "expected_min_phlo_price": 19, "expected_max_number_of_parents": 255,
            },
            "pre_restart": {"old_genesis_present": False},
        }
        self.evidence("no_existing", {"verified_no_prior_genesis": True}, "no_existing_chain_evidence")

    def evidence(self, name, data, key):
        raw = (json.dumps(data, sort_keys=True) + "\n").encode()
        path = self.root / (name + ".json")
        path.write_bytes(raw)
        self.payload["pre_restart"][key] = {
            "file": path.name, "sha256": hashlib.sha256(raw).hexdigest(),
        }

    @staticmethod
    def ancestry(repo, ancestor, build):
        return build == NEW

    def verify(self):
        return gate.check(self.payload, self.root, self.root, ancestor=self.ancestry)

    def old_evidence(self):
        self.payload["pre_restart"] = {"old_genesis_present": True}
        self.evidence("state_scan", {
            "schema_aware_expr_scan": True, "complete_state_root_export": True,
            "scanned_state_roots": 3, "expected_state_roots": 3,
            "expr_field35_count": 0,
        }, "state_scan")
        self.evidence("deploy_scan", {
            "complete_block_export": True, "scanned_blocks": 10,
            "expected_blocks": 10, "decimal_source_literal_count": 0,
        }, "deploy_scan")
        self.evidence("builds", {
            "complete_legacy_validator_inventory": True,
            "old_genesis_build_commits": [OLD],
        }, "validator_build_audit")

    def test_new_genesis_input_consistency_passes_but_still_requires_review(self):
        result = self.verify()
        self.assertEqual(result["result"], "INPUT_CONSISTENCY_ONLY_RELEASE_NOT_AUTHORIZED")
        self.assertEqual(result["min_phlo_price"], 19)

    def test_old_genesis_covered_evidence_passes_input_check_only(self):
        self.old_evidence()
        self.assertTrue(self.verify()["old_genesis_evidence_checked"])

    def test_mixed_binary_rejected(self):
        self.payload["validators"][1]["binary_sha256"] = "sha256:" + "f" * 64
        with self.assertRaisesRegex(gate.PreflightError, "mixed"):
            self.verify()

    def test_mismatched_genesis_rejected(self):
        self.payload["validators"][1]["genesis_state_hash"] = "f" * 64
        with self.assertRaisesRegex(gate.PreflightError, "genesis mismatch"):
            self.verify()

    def test_missing_validator_rejected(self):
        self.payload["validators"].pop()
        with self.assertRaisesRegex(gate.PreflightError, "incomplete"):
            self.verify()

    def test_decimal_absent_binary_rejected(self):
        self.payload["validators"][0]["supports_decimal_tag35"] = False
        with self.assertRaisesRegex(gate.PreflightError, "decimal"):
            self.verify()

    def test_unverified_decimal_ancestry_rejected(self):
        self.payload["validators"][0]["build_commit"] = OLD
        with self.assertRaisesRegex(gate.PreflightError, "differs from pinned"):
            self.verify()

    def test_wrong_param_format_rejected(self):
        raw = bytearray.fromhex(self.payload["new_genesis"]["pos_params_hex"])
        raw[0] = 2
        self.payload["new_genesis"]["pos_params_hex"] = raw.hex()
        with self.assertRaisesRegex(gate.PreflightError, "format 1"):
            self.verify()

    def test_wrong_param_length_rejected(self):
        self.payload["new_genesis"]["pos_params_hex"] += "00"
        with self.assertRaisesRegex(gate.PreflightError, "format 1"):
            self.verify()

    def test_transposed_committed_bounds_rejected(self):
        raw = bytearray.fromhex(self.payload["new_genesis"]["pos_params_hex"])
        raw[65:73], raw[73:81] = raw[73:81], raw[65:73]
        self.payload["new_genesis"]["pos_params_hex"] = raw.hex()
        with self.assertRaisesRegex(gate.PreflightError, "bounds differ"):
            self.verify()

    def test_no_old_genesis_assertion_without_proof_rejected(self):
        del self.payload["pre_restart"]["no_existing_chain_evidence"]
        with self.assertRaisesRegex(gate.PreflightError, "evidence"):
            self.verify()

    def test_old_genesis_requires_raw_state_scan_evidence(self):
        self.old_evidence()
        del self.payload["pre_restart"]["state_scan"]
        with self.assertRaisesRegex(gate.PreflightError, "state_scan"):
            self.verify()

    def test_incomplete_historical_block_export_rejected(self):
        self.old_evidence()
        self.evidence("incomplete", {
            "complete_block_export": True, "scanned_blocks": 9,
            "expected_blocks": 10, "decimal_source_literal_count": 0,
        }, "deploy_scan")
        with self.assertRaisesRegex(gate.PreflightError, "block coverage"):
            self.verify()

    def test_unknown_schema_aware_scanner_rejected(self):
        self.old_evidence()
        self.evidence("unknown", {
            "schema_aware_expr_scan": False, "complete_state_root_export": True,
            "scanned_state_roots": 3, "expected_state_roots": 3,
            "expr_field35_count": 0,
        }, "state_scan")
        with self.assertRaisesRegex(gate.PreflightError, "completeness"):
            self.verify()

    def test_field35_found_is_rejected(self):
        self.old_evidence()
        self.evidence("found", {
            "schema_aware_expr_scan": True, "complete_state_root_export": True,
            "scanned_state_roots": 3, "expected_state_roots": 3,
            "expr_field35_count": 1,
        }, "state_scan")
        with self.assertRaisesRegex(gate.PreflightError, "expr_field35_count"):
            self.verify()

    def test_decimal_literal_found_is_rejected(self):
        self.old_evidence()
        self.evidence("found", {
            "complete_block_export": True, "scanned_blocks": 10,
            "expected_blocks": 10, "decimal_source_literal_count": 1,
        }, "deploy_scan")
        with self.assertRaisesRegex(gate.PreflightError, "decimal_source_literal_count"):
            self.verify()

    def test_decimal_capable_old_genesis_binary_rejected(self):
        self.old_evidence()
        self.evidence("bad_legacy", {
            "complete_legacy_validator_inventory": True,
            "old_genesis_build_commits": [NEW],
        }, "validator_build_audit")
        with self.assertRaisesRegex(gate.PreflightError, "decimal-capable"):
            self.verify()

    def test_tampered_evidence_rejected(self):
        self.old_evidence()
        (self.root / "state_scan.json").write_text("{}")
        with self.assertRaisesRegex(gate.PreflightError, "checksum mismatch"):
            self.verify()


    def test_release_commit_missing_decimal_or_fork_dependency_is_blocked(self):
        original = self.ancestry
        for blocked in (gate.DECIMAL_MERGE, gate.LEAN_MERGE, gate.FORK_HEAD):
            def not_in_release(repo, earlier, later, *, missing=blocked):
                return original(repo, earlier, later) and earlier != missing
            with self.subTest(missing=blocked):
                with self.assertRaisesRegex(gate.PreflightError, "release commit does not contain"):
                    gate.check(self.payload, self.root, self.root, ancestor=not_in_release)

    def test_wrong_release_commit_blocks(self):
        self.payload["release_commit"] = OLD
        with self.assertRaisesRegex(gate.PreflightError, "release commit does not contain"):
            self.verify()

    def test_wrong_genesis_version_blocks(self):
        for version in (0, 2, True, None):
            with self.subTest(version=version):
                self.payload["new_genesis"]["block_version"] = version
                with self.assertRaisesRegex(gate.PreflightError, "block version 1"):
                    self.verify()

    def test_negative_or_boolean_scan_counts_are_not_zero(self):
        self.old_evidence()
        for value in (-1, True, None):
            with self.subTest(value=value):
                self.evidence("invalid_count", {
                    "schema_aware_expr_scan": True, "complete_state_root_export": True,
                    "scanned_state_roots": 3, "expected_state_roots": 3,
                    "expr_field35_count": value,
                }, "state_scan")
                with self.assertRaisesRegex(gate.PreflightError, "expr_field35_count"):
                    self.verify()

    def test_duplicate_json_evidence_keys_rejected(self):
        self.old_evidence()
        raw = b'{"schema_aware_expr_scan":true,"complete_state_root_export":true,"scanned_state_roots":3,"expected_state_roots":3,"expr_field35_count":1,"expr_field35_count":0}'
        p = self.root / "double.json"
        p.write_bytes(raw)
        self.payload["pre_restart"]["state_scan"] = {
            "file": "double.json", "sha256": hashlib.sha256(raw).hexdigest(),
        }
        with self.assertRaisesRegex(gate.PreflightError, "duplicate JSON field"):
            self.verify()

    def test_missing_old_genesis_build_inventory_blocks(self):
        self.old_evidence()
        self.payload["pre_restart"].pop("validator_build_audit")
        with self.assertRaisesRegex(gate.PreflightError, "validator_build_audit"):
            self.verify()

    def test_incomplete_validator_inventory_and_wrong_source_blocks(self):
        self.payload["validators"][1]["build_commit"] = OLD
        with self.assertRaisesRegex(gate.PreflightError, "differs from pinned"):
            self.verify()
        self.payload["validators"][1]["build_commit"] = NEW
        self.payload["expected_validator_count"] = 3
        with self.assertRaisesRegex(gate.PreflightError, "incomplete"):
            self.verify()

    def test_external_review_is_required_even_on_valid_manifest(self):
        from unittest.mock import patch
        from contextlib import redirect_stdout
        from io import StringIO
        from sys import argv
        manifest = self.root / "manifest.json"
        manifest.write_text(json.dumps(self.payload))
        stdout = StringIO()
        from unittest.mock import patch
        real_check = gate.check
        def check_with_fixture_ancestry(data, directory, repo):
            return real_check(data, directory, repo, ancestor=self.ancestry)
        with patch.object(gate, "check", side_effect=check_with_fixture_ancestry), \
             patch("sys.argv", ["decimal_release_preflight.py", str(manifest),
                                "--repo", str(self.root)]), \
             redirect_stdout(stdout):
            exit_code = gate.main()
        self.assertEqual(exit_code, 3, "successful input check must never authorize release")
        self.assertIn("INPUT_CONSISTENCY_ONLY_RELEASE_NOT_AUTHORIZED", stdout.getvalue())

    def test_duplicate_top_level_manifest_keys_rejected(self):
        from unittest.mock import patch
        from contextlib import redirect_stdout
        from io import StringIO
        raw = json.dumps(self.payload)[:-1] + ',"schema":1}'
        manifest = self.root / "duplicate.json"
        manifest.write_text(raw)
        stdout = StringIO()
        with patch("sys.argv", ["decimal_release_preflight.py", str(manifest)]), redirect_stdout(stdout):
            exit_code = gate.main()
        self.assertEqual(exit_code, 2)
        self.assertIn("duplicate JSON field", stdout.getvalue())


if __name__ == "__main__":
    unittest.main()
