#!/usr/bin/env python3
"""Fail-closed, evidence-bound checks for the #373 fresh-genesis release preflight.

This checks a supplied operator inventory. It cannot create or authenticate the
historical native-state export, nor attest that every real validator was inventoried.
Those duties remain separate review/operational acceptance requirements.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import subprocess
import sys
from pathlib import Path

FORMAT = 1
FIELDS = 10
DECIMAL_MERGE = "666ddb75e2d2787f099e05f983e31fb9728e7ed6"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class PreflightError(ValueError):
    pass


def require(cond: bool, message: str) -> None:
    if not cond:
        raise PreflightError(message)


def hex_value(value: object, n: int, name: str) -> str:
    require(isinstance(value, str) and bool((HEX40 if n == 40 else HEX64).fullmatch(value)), name + ": invalid digest")
    return value


def object_at(data: dict, key: str) -> dict:
    val = data.get(key)
    require(isinstance(val, dict), key + ": object required")
    return val


def json_evidence(ref: dict, directory: Path, label: str) -> dict:
    name = ref.get("file")
    expected = hex_value(ref.get("sha256"), 64, label + ".sha256")
    require(isinstance(name, str) and bool(name) and not Path(name).is_absolute(), label + ": relative filename required")
    base = directory.resolve()
    path = (base / name).resolve()
    require(path.is_relative_to(base) and path.is_file(), label + ": missing or unsafe evidence path")
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == expected, label + ": evidence checksum mismatch")
    try:
        data = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PreflightError(label + ": malformed JSON evidence") from exc
    require(isinstance(data, dict), label + ": expected evidence object")
    return data


def git_descends(repo: Path, earlier: str, later: str) -> bool:
    try:
        proc = subprocess.run(
            ["git", "-C", str(repo), "merge-base", "--is-ancestor", earlier, later],
            check=False, capture_output=True, timeout=25,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PreflightError("cannot verify build ancestry") from exc
    require(proc.returncode in (0, 1), "cannot verify git ancestry (missing objects?)")
    return proc.returncode == 0


def check(data: dict, directory: Path, repo: Path, *, ancestor=git_descends) -> dict:
    require(data.get("schema") == 1, "schema 1 required")
    require(data.get("activation") == "fresh-genesis-uniform-validator-upgrade",
            "old-genesis binary swap is not an approved activation")
    genesis = object_at(data, "new_genesis")
    block_hash = hex_value(genesis.get("block_hash"), 64, "new_genesis.block_hash")
    state_hash = hex_value(genesis.get("state_hash"), 64, "new_genesis.state_hash")
    raw_record = genesis.get("pos_params_hex")
    require(isinstance(raw_record, str), "new_genesis.pos_params_hex missing")
    try:
        params = bytes.fromhex(raw_record)
    except ValueError as exc:
        raise PreflightError("invalid PosParams hex") from exc
    require(len(params) == 1 + 8 * FIELDS and params[0] == FORMAT,
            "expected format 1 followed by exactly ten little-endian i64 fields")
    fields = struct.unpack("<10q", params[1:])
    require(fields[8] >= 0 and fields[9] >= 0, "invalid committed validation bounds")
    require(fields[8] == genesis.get("expected_min_phlo_price")
            and fields[9] == genesis.get("expected_max_number_of_parents"),
            "declared validation bounds differ from encoded genesis params")
    validators = data.get("validators")
    expected_count = data.get("expected_validator_count")
    require(isinstance(expected_count, int) and not isinstance(expected_count, bool)
            and expected_count > 0, "positive expected validator count required")
    require(isinstance(validators, list) and len(validators) == expected_count,
            "validator inventory incomplete")
    seen = set()
    binaries = set()
    builds = set()
    for item in validators:
        require(isinstance(item, dict), "validator entry must be an object")
        ident = item.get("identity")
        require(isinstance(ident, str) and ident and ident not in seen, "duplicate/missing validator identity")
        seen.add(ident)
        digest = item.get("binary_sha256")
        require(isinstance(digest, str) and digest.startswith("sha256:"), "binary digest missing")
        binaries.add(hex_value(digest[7:], 64, "binary digest"))
        build = hex_value(item.get("build_commit"), 40, "validator build commit")
        builds.add(build)
        require(item.get("supports_decimal_tag35") is True, "validator lacks decimal capability")
        require(item.get("genesis_block_hash") == block_hash
                and item.get("genesis_state_hash") == state_hash,
                "validator genesis mismatch")
        require(ancestor(repo, DECIMAL_MERGE, build),
                "validator build does not contain decimal merge #389")
    require(len(binaries) == 1 and len(builds) == 1,
            "mixed binaries/builds are forbidden at the new genesis")
    old = object_at(data, "pre_restart")
    present = old.get("old_genesis_present")
    require(isinstance(present, bool), "pre_restart.old_genesis_present required")
    if present:
        scanned = json_evidence(object_at(old, "state_scan"), directory, "state_scan")
        require(scanned.get("schema_aware_expr_scan") is True
                and scanned.get("complete_state_root_export") is True,
                "native-state scan completeness is not demonstrated")
        require(isinstance(scanned.get("scanned_state_roots"), int)
                and scanned["scanned_state_roots"] > 0
                and scanned["scanned_state_roots"] == scanned.get("expected_state_roots"),
                "state-root scan coverage incomplete")
        require(scanned.get("expr_field35_count") == 0, "tag-35 expression found in old state")
        sources = json_evidence(object_at(old, "deploy_scan"), directory, "deploy_scan")
        require(sources.get("complete_block_export") is True
                and isinstance(sources.get("scanned_blocks"), int)
                and sources["scanned_blocks"] > 0
                and sources["scanned_blocks"] == sources.get("expected_blocks"),
                "historical block coverage incomplete")
        require(sources.get("decimal_source_literal_count") == 0,
                "decimal literal found in old block's deploy source")
        boots = json_evidence(object_at(old, "validator_build_audit"), directory, "validator_build_audit")
        require(boots.get("complete_legacy_validator_inventory") is True,
                "old-genesis validator build inventory incomplete")
        observed = boots.get("old_genesis_build_commits")
        require(isinstance(observed, list) and bool(observed), "old-genesis build evidence absent")
        for sha in observed:
            sha = hex_value(sha, 40, "old-genesis build commit")
            require(not ancestor(repo, DECIMAL_MERGE, sha),
                    "a decimal-capable build ran against the old genesis")
    else:
        require(old.get("no_existing_chain_evidence") is not None,
                "fresh network requires operator evidence that no old chain existed")
        evidence = json_evidence(object_at(old, "no_existing_chain_evidence"),
                                 directory, "no_existing_chain_evidence")
        require(evidence.get("verified_no_prior_genesis") is True,
                "absence of an earlier genesis not established")
    return {
        "result": "INPUT_CONSISTENCY_PASS_OPERATOR_REVIEW_REQUIRED",
        "validator_count": len(validators),
        "binary_sha256": next(iter(binaries)),
        "build_commit": next(iter(builds)),
        "genesis_block_hash": block_hash,
        "params_format": FORMAT,
        "min_phlo_price": fields[8],
        "max_number_of_parents": fields[9],
        "old_genesis_evidence_checked": present,
        "limitations": [
            "Operator-supplied archive completeness and build provenance require independent review",
            "No claim that a public-network snapshot was scanned unless supplied and verified",
            "Does not prove consensus finality, long-history replay or rollback safety",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path, help="operator-generated release evidence manifest")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    try:
        data = json.loads(args.manifest.read_text())
        outcome = check(data, args.manifest.parent, args.repo)
    except (PreflightError, OSError, ValueError, TypeError) as exc:
        print(json.dumps({"result": "BLOCKED", "reason": str(exc)}, sort_keys=True))
        return 2
    print(json.dumps(outcome, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
