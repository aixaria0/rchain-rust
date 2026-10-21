#!/usr/bin/env python3
"""Exercise the emSign exception with real cargo-deny bans checks, without a network.

The empty local crates below test package/version policy only. The preceding full
`cargo deny check` audits the real dependency graph, sources and RustSec records.
"""
from pathlib import Path
import argparse
import os
import subprocess
import tempfile


CASES = [
    ("0.25.4", False),
    ("0.26.10", False),  # Older releases bundle roots rather than re-exporting 1.x.
    ("0.26.11", True),  # The one reviewed semver-trick shim.
    ("0.26.12", False),  # Any additional shim version needs explicit review.
    ("0.27.0", False),
    ("1.0.0", False),
    ("1.0.9", False),
    ("1.0.10", True),
    ("1.0.11", True),
]


def check_case(directory, version, allowed, config):
    root = directory / version
    roots = root / "roots"
    (root / "src").mkdir(parents=True)
    (roots / "src").mkdir(parents=True)
    (root / "Cargo.toml").write_text(
        '[package]\nname = "emsign-policy-fixture"\nversion = "0.1.0"\n'
        'edition = "2021"\n[workspace]\n[dependencies]\n'
        'webpki-roots = { path = "roots" }\n'
    )
    (roots / "Cargo.toml").write_text(
        f'[package]\nname = "webpki-roots"\nversion = "{version}"\n'
        'edition = "2021"\n'
    )
    (root / "src/lib.rs").write_text("")
    (roots / "src/lib.rs").write_text("")
    env = dict(os.environ, CARGO_TERM_COLOR="never")
    lock = subprocess.run(
        ["cargo", "generate-lockfile", "--offline"],
        cwd=root, env=env, capture_output=True, text=True, timeout=60,
    )
    if lock.returncode:
        raise RuntimeError(f"{version}: fixture resolution failed:\n{lock.stderr}")
    check = subprocess.run(
        ["cargo", "deny", "--manifest-path", str(root / "Cargo.toml"),
         "--config", str(config), "--offline", "--locked", "check", "bans"],
        cwd=root, env=env, capture_output=True, text=True, timeout=60,
    )
    output = check.stdout + check.stderr
    if allowed:
        if check.returncode:
            raise RuntimeError(f"{version}: patched provider/shim rejected:\n{output}")
    elif check.returncode == 0 or "error[banned]" not in output or "webpki-roots" not in output:
        raise RuntimeError(f"{version}: expected an explicit webpki-roots ban:\n{output}")
    print(f"PASS webpki-roots {version}: {'allowed' if allowed else 'banned'}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config", type=Path,
        default=Path(__file__).resolve().parents[1] / "deny.toml",
    )
    args = parser.parse_args()
    config = args.config.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="rchain-emsign-policy-") as temporary:
        for version, allowed in CASES:
            check_case(Path(temporary), version, allowed, config)
    print(f"emSign dependency policy: {len(CASES)} controls passed")


if __name__ == "__main__":
    main()
