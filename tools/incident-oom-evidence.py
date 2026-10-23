#!/usr/bin/env python3
"""Classify bounded operator-supplied kernel OOM evidence for Incident #420.

This parser does not contact validators, execute deploys, read private keys,
authenticate a host, or prove Casper CBC state/finality behavior.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import re
import sys

MAX_BYTES = 1024 * 1024
HEX_DIGEST = re.compile(r"^[0-9a-f]{64}$")
LABEL = re.compile(r"^[A-Za-z][A-Za-z0-9_-]{0,39}$")
STAMP = re.compile(r"(?P<time>\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d{4}))")
UNIT = re.compile(r"task_memcg=[^, \s]*/(?P<unit>rnode(?:-c)?\.service)(?:[ ,]|$)")
KILLED = re.compile(r"(?:Out of memory:|Memory cgroup out of memory:)\s*Killed process\s+\d+\s+\((?P<process>[^)]+)\)")
TARGET_UNITS = {"rnode.service", "rnode-c.service"}


class EvidenceError(ValueError):
    pass


def source_bytes(filename: str) -> bytes:
    with open(filename, "rb") as source:
        raw = source.read(MAX_BYTES + 1)
    if not raw or len(raw) > MAX_BYTES:
        raise EvidenceError("kernel journal sample must be 1 byte to 1 MiB")
    return raw


def parse_time(line: str) -> datetime | None:
    m = STAMP.search(line[:70])
    if m is None:
        return None
    token = m.group("time")
    try:
        instant = datetime.fromisoformat(token.replace("Z", "+00:00"))
    except ValueError:
        return None
    return instant.astimezone(timezone.utc)


def summarize(raw: bytes, host: str, expected_sha256: str | None = None) -> dict:
    if not LABEL.fullmatch(host):
        raise EvidenceError("host must be an unidentifying operator-provided label")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None and (
        not HEX_DIGEST.fullmatch(expected_sha256) or expected_sha256 != sha
    ):
        raise EvidenceError("kernel-journal input SHA-256 mismatch")
    try:
        lines = raw.decode("utf-8", errors="strict").splitlines()
    except UnicodeDecodeError as err:
        raise EvidenceError("expected UTF-8 kernel-journal text") from err
    if len(lines) > 10000:
        raise EvidenceError("too many kernel journal lines")

    signals: list[dict] = []
    victims: list[dict] = []
    for line in lines:
        instant = parse_time(line)
        if instant is None:
            continue
        if "oom-kill:" in line:
            m = UNIT.search(line)
            if m and m.group("unit") in TARGET_UNITS:
                signals.append({"time": instant, "unit": m.group("unit")})
        match = KILLED.search(line)
        if match and match.group("process") == "rnode":
            victims.append({"time": instant, "victim": "rnode"})
    confirmed = []
    for signal in signals:
        seen = any(abs(victim["time"] - signal["time"]) <= timedelta(seconds=3)
                   for victim in victims)
        if seen:
            entry = {"utc": signal["time"].isoformat().replace("+00:00", "Z"),
                     "service": signal["unit"], "process": "rnode",
                     "classification": "KERNEL_OOM_KILL_CONFIRMED_FOR_HOST"}
            if entry not in confirmed:
                confirmed.append(entry)
    confirmed.sort(key=lambda x: (x["utc"], x["service"]))
    return {
        "schema": "rholo-host-kernel-oom-evidence/v1",
        "host_label": host,
        "source": "OPERATOR_SUPPLIED_KERNEL_LOG_BYTES",
        "raw_sha256": sha,
        "external_digest_match": expected_sha256 is not None,
        "read_only": True,
        "kernel_oom_kill_events": confirmed,
        "status": "KERNEL_OOM_KILLS_PRESENT" if confirmed else "NO_MATCHING_EVENTS_IN_SUPPLIED_WINDOW",
        "log_window_complete": False,
        "host_source_authenticated": False,
        "casper_finality_proven": False,
        "finalized_write_loss_proven": False,
        "boundary": ("Matched kernel OOM records apply only to the named host and supplied "
                     "time window. They do not independently prove why the network stalled, "
                     "nor that a finalized application write was lost."),
    }


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--journal", required=True, help="Pre-captured, offline journalctl -k text")
    p.add_argument("--host-label", required=True, help="Neutral host label (e.g., HostBC)")
    p.add_argument("--expect-sha256", help="Externally reviewed SHA-256 of raw journal bytes")
    p.add_argument("--output", help="New result JSON file (never overwritten)")
    args = p.parse_args(argv)
    try:
        packet = summarize(source_bytes(args.journal), args.host_label, args.expect_sha256)
        output = json.dumps(packet, sort_keys=True, indent=2) + "\n"
        if args.output:
            target = Path(args.output)
            with target.open("x", encoding="utf-8") as stream:
                stream.write(output)
            print("wrote", target, packet["status"])
        else:
            sys.stdout.write(output)
    except (EvidenceError, OSError) as err:
        print("kernel-oom-evidence refused:", err, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
