#!/usr/bin/env python3
"""Check exact authored §6 coverage. This does not certify category or safety."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLASSIFICATIONS = {
    "IMPLEMENTED / VERIFIED", "IMPLEMENTED / EVIDENCE INCOMPLETE", "CODE DEFECT",
    "INTEGRATION GAP", "RELEASE BLOCKER", "REQUIRES MAINTAINER DECISION",
    "EXPLICITLY DEFERRED",
}

def source_rows(document):
    marker = "## 6. Scala-deviation register"
    if document.count(marker) != 1:
        raise ValueError("exactly one section 6 required")
    section = document.split(marker, 1)[1].split("\n## 7.", 1)[0]
    rows = [line for line in section.splitlines()
            if line.startswith("|") and not line.startswith("|---")
            and not line.startswith("| Deviation")]
    if not rows:
        raise ValueError("empty section 6 inventory")
    return rows

def validate(document, inventory, matrix, ledger):
    expected = source_rows(document)
    rows = inventory.get("rows")
    if not isinstance(rows, list) or len(rows) != len(expected):
        raise ValueError("section 6 row coverage changed")
    if inventory.get("source_dev_sha") != ledger["source_dev_sha"]:
        raise ValueError("inventory and tracker have different source commits")
    if matrix.get("source_dev_sha") != ledger["source_dev_sha"]:
        raise ValueError("matrix and tracker have different source commits")
    if inventory.get("independent_signoff") is not False:
        raise ValueError("this authored inventory cannot supply independent sign-off")
    seen = set()
    for raw, row in zip(expected, rows):
        digest = hashlib.sha256(raw.encode()).hexdigest()
        if row.get("row_sha256") != digest or row.get("id") != "S6:"+digest[:16]:
            raise ValueError("source row digest or order changed")
        if row["id"] in seen:
            raise ValueError("duplicate source row")
        seen.add(row["id"])
        if row.get("existing_evidence", {}).get("source_row") != raw:
            raise ValueError("authored source evidence changed")
        if row.get("fork_class") not in {"A_CANDIDATE", "NON_A_RECORDED"}:
            raise ValueError("every row needs an explicit category disposition")
        if row.get("classification") != "IMPLEMENTED / EVIDENCE INCOMPLETE":
            raise ValueError("source coverage must not be promoted to verified safety")
        for field in ("invariant", "missing_proof", "smallest_action", "acceptance_test", "decision_owner"):
            if not isinstance(row.get(field), str) or not row[field].strip():
                raise ValueError("missing row obligation: "+field)
    entries = matrix.get("items", [])
    ids = [x.get("id") for x in entries]
    if len(set(ids)) != len(ids) or set(ids) != {x["id"] for x in ledger["items"]}:
        raise ValueError("tracker matrix coverage mismatch")
    for row in entries:
        if row.get("classification") not in CLASSIFICATIONS:
            raise ValueError("unclassified tracker item")
        if row.get("recorded_owner_reason_deferral") is not False:
            raise ValueError("matrix cannot fabricate an approved release deferral")
        if row["classification"] in {"IMPLEMENTED / VERIFIED", "EXPLICITLY DEFERRED"}:
            raise ValueError("this source-only matrix cannot certify execution or deferral")
        for field in ("exact_file_function_or_invariant", "missing_proof",
                      "smallest_corrective_action", "required_acceptance_test", "decision_owner"):
            if not isinstance(row.get(field), str) or not row[field].strip():
                raise ValueError("missing tracker obligation: "+field)
    return {"result":"EXACT_SOURCE_COVERAGE_ONLY_INDEPENDENT_REVIEW_REQUIRED",
            "section6_rows":len(rows), "category_a_candidates":sum(x["fork_class"]=="A_CANDIDATE" for x in rows),
            "tracker_obligations":len(entries), "independent_signoff":False, "release_authorized":False}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--register",type=Path,default=ROOT/"spec/audit/passes.md")
    args=parser.parse_args()
    try:
        audit=ROOT/"spec/audit"
        result=validate(args.register.read_text(),
            json.loads((audit/"section6-gap-inventory.json").read_text()),
            json.loads((audit/"final-gap-matrix.json").read_text()),
            json.loads((audit/"hardfork-51-closeout.json").read_text()))
    except (OSError,ValueError,KeyError,TypeError) as exc:
        print(json.dumps({"result":"INVALID_INVENTORY","reason":str(exc)}))
        return 2
    print(json.dumps(result,sort_keys=True))
    return 0

if __name__=="__main__":
    raise SystemExit(main())
