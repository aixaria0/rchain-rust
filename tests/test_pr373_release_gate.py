"""Negative controls for the #373 release preflight; fixture evidence is synthetic."""
import hashlib
import importlib.util
import json
import struct
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "pr373_release_gate.py"
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
            "expected_validator_count": 2,
            "validators": [validator("v1"), validator("v2")],
            "new_genesis": {
                "block_hash": GENESIS, "state_hash": STATE,
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
        self.assertEqual(result["result"], "INPUT_CONSISTENCY_PASS_OPERATOR_REVIEW_REQUIRED")
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
        with self.assertRaisesRegex(gate.PreflightError, "does not contain"):
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
        with self.assertRaisesRegex(gate.PreflightError, "tag-35"):
            self.verify()

    def test_decimal_literal_found_is_rejected(self):
        self.old_evidence()
        self.evidence("found", {
            "complete_block_export": True, "scanned_blocks": 10,
            "expected_blocks": 10, "decimal_source_literal_count": 1,
        }, "deploy_scan")
        with self.assertRaisesRegex(gate.PreflightError, "decimal literal"):
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


if __name__ == "__main__":
    unittest.main()
