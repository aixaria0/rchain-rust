#!/usr/bin/env python3
"""Post-rebuild acceptance for an RChain public/read node.

The drill turns a reviewed commit plus a rebuilt node into a preserved evidence
bundle. It is read-only: no deploy, propose, restart, or configuration change.
PASS means the observed nodes are on the reviewed build, the read returns values,
and the rebuilt subject is within the operator-reviewed factor of a fresh same-build
control. Missing or incomparable evidence is BLOCKED, never silently accepted.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Optional

COMMIT_RE = re.compile(r"\(([0-9a-fA-F]{40}|[0-9a-fA-F]{64})\)")


@dataclass
class ProbeRow:
    url: str
    height: int
    finalized: Optional[int]
    read_ms: int
    noop_ms: int
    delta_ms: int
    result: str


@dataclass
class Decision:
    verdict: str
    reasons: list[str]
    ratio: Optional[float] = None


def http_get(url: str, timeout: int = 20) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "rchain-post-rebuild-acceptance/1"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.read()


def text_get(url: str) -> str:
    return http_get(url).decode("utf-8", errors="replace")


def extract_commit(version: str) -> Optional[str]:
    match = COMMIT_RE.search(version.strip())
    return match.group(1).lower() if match else None


def parse_probe(text: str, base: str) -> ProbeRow:
    for raw in text.splitlines():
        parts = raw.split(maxsplit=7)
        if len(parts) < 7 or parts[0] != base:
            continue
        finalized = None if parts[2] == "?" else int(parts[2])
        result = parts[6] if len(parts) == 7 else parts[6] + " " + parts[7]
        return ProbeRow(
            url=parts[0], height=int(parts[1]), finalized=finalized,
            read_ms=int(parts[3]), noop_ms=int(parts[4]),
            delta_ms=int(parts[5]), result=result.strip(),
        )
    raise ValueError(f"probe produced no summary row for {base}")


def write_bytes(path: Path, data: bytes) -> None:
    path.write_bytes(data)


def build_evidence_manifest(out: Path) -> dict:
    entries = {}
    excluded = {"evidence-manifest.json", "result.json", "report.md"}
    for path in sorted(p for p in out.iterdir() if p.is_file() and p.name not in excluded):
        data = path.read_bytes()
        entries[path.name] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
    return {"schema": "rchain-evidence-manifest/v1", "files": entries}


def decide(*, expected_commit: str, versions: dict[str, str], rows: dict[str, ProbeRow],
           control_name: str, subject_name: str, max_control_height: int,
           max_factor: float) -> Decision:
    reasons: list[str] = []
    commits = {name: extract_commit(value) for name, value in versions.items()}

    for name, commit in commits.items():
        if commit is None:
            reasons.append(f"{name}: build provenance is unknown")
        elif commit != expected_commit.lower():
            reasons.append(f"{name}: build {commit} != reviewed {expected_commit.lower()}")

    if len({c for c in commits.values() if c is not None}) > 1:
        reasons.append("observed nodes are not on one exact build")

    for name, row in rows.items():
        if row.result != "values":
            reasons.append(f"{name}: read result is {row.result!r}, not values")

    control = rows[control_name]
    subject = rows[subject_name]
    if control.height > max_control_height:
        reasons.append(
            f"{control_name}: height {control.height} exceeds fresh-control bound {max_control_height}"
        )
    if control.read_ms <= 0:
        reasons.append(f"{control_name}: non-positive read median")

    if reasons:
        return Decision("BLOCKED", reasons)

    ratio = subject.read_ms / control.read_ms
    if ratio > max_factor:
        return Decision(
            "FAIL",
            [f"subject/control median ratio {ratio:.3f} exceeds reviewed factor {max_factor:.3f}"],
            ratio,
        )
    return Decision("PASS", ["all acceptance predicates observed"], ratio)


def capture_optional(url: str, path: Path, errors: list[str]) -> None:
    try:
        write_bytes(path, http_get(url))
    except Exception as error:  # evidence capture is best effort, never fabricated
        errors.append(f"{url}: {error!r}")
        path.write_text("")


def run_probe(probe: Path, samples: int, urls: list[str], out: Path) -> str:
    command = ["bash", str(probe), "--samples", str(samples), *urls]
    completed = subprocess.run(command, cwd=probe.parent.parent, text=True,
                               capture_output=True, timeout=max(90, samples * len(urls) * 65))
    (out / "probe.stdout.txt").write_text(completed.stdout)
    (out / "probe.stderr.txt").write_text(completed.stderr)
    (out / "probe.command.json").write_text(json.dumps(command, indent=2) + "\n")
    if completed.returncode:
        raise RuntimeError(f"probe exited {completed.returncode}; see probe.stderr.txt")
    return completed.stdout


def render_report(*, args, versions, rows, decision: Decision, capture_errors: list[str]) -> str:
    lines = [
        "# Post-rebuild node acceptance",
        "",
        f"**Verdict: {decision.verdict}**",
        "",
        f"Reviewed commit: `{args.expected_commit}`",
        f"Samples per arm: {args.samples}",
        f"Fresh-control bound: height ≤ {args.max_control_height}",
        f"Accepted subject/control factor: ≤ {args.max_factor:g}×",
        "",
        "## Observations",
        "",
        "| role | URL | build | height | final | read ms | no-op ms | delta ms | result |",
        "|---|---|---|---:|---:|---:|---:|---:|---|",
    ]
    for name, row in rows.items():
        commit = extract_commit(versions[name]) or "unknown"
        final = "?" if row.finalized is None else str(row.finalized)
        lines.append(
            f"| {name} | `{row.url}` | `{commit}` | {row.height} | {final} | "
            f"{row.read_ms} | {row.noop_ms} | {row.delta_ms} | {row.result} |"
        )
    lines += ["", "## Decision", ""]
    lines += [f"- {reason}" for reason in decision.reasons]
    if decision.ratio is not None:
        lines.append(f"- subject/control median ratio: **{decision.ratio:.3f}×**")
    if capture_errors:
        lines += ["", "## Non-gating capture warnings", ""]
        lines += [f"- {item}" for item in capture_errors]
    lines += [
        "", "## Boundary", "",
        "This is a finite post-rebuild observation, not a universal performance theorem. ",
        "PASS is licensed only for the reviewed commit and the evidence captured by this run.", "",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True, help="path to tools/probe-read-latency.sh")
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--subject", required=True, help="rebuilt node loopback URL")
    parser.add_argument("--control", required=True, help="fresh same-box/same-build control loopback URL")
    parser.add_argument("--public", help="optional public URL for the rebuilt subject")
    parser.add_argument("--admin", help="optional admin-loopback URL to capture operator status")
    parser.add_argument("--samples", type=int, default=9)
    parser.add_argument("--max-factor", type=float, required=True)
    parser.add_argument("--max-control-height", type=int, required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    if not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", args.expected_commit):
        parser.error("--expected-commit must be a full 40- or 64-hex object id")
    if args.samples < 1 or args.max_factor < 1 or args.max_control_height < 0:
        parser.error("samples >= 1, max-factor >= 1, max-control-height >= 0")
    for value in [args.subject, args.control]:
        if not re.fullmatch(r"http://(?:127\.0\.0\.1|localhost)(?::[0-9]+)?/?", value):
            parser.error("--subject and --control must be loopback HTTP URLs")

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    started = time.time()
    capture_errors: list[str] = []
    urls = {"subject": args.subject, "control": args.control}
    if args.public:
        urls["public"] = args.public

    try:
        versions = {}
        for name, base in urls.items():
            versions[name] = text_get(base.rstrip("/") + "/version").strip()
            (out / f"{name}.version.txt").write_text(versions[name] + "\n")
            write_bytes(out / f"{name}.status.json", http_get(base.rstrip("/") + "/api/status"))
            write_bytes(out / f"{name}.last-finalized.json", http_get(base.rstrip("/") + "/api/last-finalized-block"))
            capture_optional(base.rstrip("/") + "/metrics", out / f"{name}.metrics.txt", capture_errors)

        if args.admin:
            capture_optional(args.admin.rstrip("/") + "/api/v1/status/operator",
                             out / "subject.operator-status.json", capture_errors)

        probe_text = run_probe(Path(args.probe).resolve(), args.samples, list(urls.values()), out)
        rows = {name: parse_probe(probe_text, base) for name, base in urls.items()}
        decision = decide(
            expected_commit=args.expected_commit, versions=versions, rows=rows,
            control_name="control", subject_name="subject",
            max_control_height=args.max_control_height, max_factor=args.max_factor,
        )
    except Exception as error:
        decision = Decision("BLOCKED", [repr(error)])
        versions, rows = {}, {}

    manifest = build_evidence_manifest(out)
    manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    write_bytes(out / "evidence-manifest.json", manifest_bytes)

    result = {
        "schema": "rchain-post-rebuild-acceptance/v1",
        "evidence_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "started_unix": started,
        "finished_unix": time.time(),
        "expected_commit": args.expected_commit.lower(),
        "policy": {"samples": args.samples, "max_factor": args.max_factor,
                   "max_control_height": args.max_control_height},
        "versions": versions,
        "observations": {name: asdict(row) for name, row in rows.items()},
        "capture_warnings": capture_errors,
        "decision": asdict(decision),
    }
    (out / "result.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    (out / "report.md").write_text(render_report(args=args, versions=versions, rows=rows,
                                                   decision=decision, capture_errors=capture_errors))
    print(json.dumps(result["decision"], sort_keys=True))
    return {"PASS": 0, "FAIL": 1, "BLOCKED": 3}[decision.verdict]


if __name__ == "__main__":
    sys.exit(main())
