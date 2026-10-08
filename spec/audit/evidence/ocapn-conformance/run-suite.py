#!/usr/bin/env python3
"""Drive the OCapN conformance suite against one peer, and report per-module counts.

    python3 run-suite.py <suite-checkout> ocapn://<designator>.<transport>?host=127.0.0.1&port=22045 --all
    python3 run-suite.py <suite-checkout> <locator> --module tests.op_deliver --verbose

**Why this is in the tree.** The OCapN project's suite is the oracle for this port — `README.md` in
this directory says so, and the runs recorded beside it were made with a driver like this one — but
the suite's own `test_runner.py` imports the Tor netlayer at load, so a `tcp-testing-only` run needs a
driver that does not. This is that driver, committed so the gate is reproducible by anyone rather than
being a command someone once typed (the rule the ERTP close-out learned the hard way about CI gates no
one can re-run locally).

**One module per invocation, and counts from the result object.** The suite's printed tally counts a
`setUp` error against the aborted test as well as itself, so it exceeds the number of tests; the
summary here is `testsRun - failures - errors`, which does not. Modules are run separately for the
same reason: a module that fails in `setUp` would otherwise make every later module's numbers a
statement about the first one.

Run conditions: the peer must already be listening at the locator. The repository root is two
directories up, so the peer builds with `cargo build -p rchain-ocapn --bin ocapn-tcp-testing`.
"""
import argparse
import subprocess
import sys


MODULES = [
    "tests.op_abort",
    "tests.op_start_session",
    "tests.op_deliver",
    "tests.op_listen",
    "tests.op_gc",
    "tests.third_party_handoffs",
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", help="path to the ocapn-test-suite checkout")
    parser.add_argument("locator", help="ocapn://<designator>.<transport>?host=..&port=..")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--module", help="one module, e.g. tests.op_deliver")
    group.add_argument(
        "--all", action="store_true", help="every module in MODULES, one after another"
    )
    parser.add_argument("--captp-version", default="1.0")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()

    sys.path.insert(0, args.suite)
    from netlayers.testing_only_tcp import TestingOnlyTCPNetlayer  # noqa: E402
    from utils.ocapn_uris import OCapNPeer  # noqa: E402
    from utils.test_suite import CapTPTestRunner  # noqa: E402

    peer = OCapNPeer.from_uri(args.locator)
    if str(peer.transport) != "tcp-testing-only":
        print(f"this driver speaks tcp-testing-only, and the locator says {peer.transport}")
        return 2

    revision = subprocess.run(
        ["git", "-C", args.suite, "rev-parse", "--short", "HEAD"],
        capture_output=True,
        text=True,
    ).stdout.strip()
    print(f"suite {args.suite} at {revision}, peer {args.locator}")

    modules = [args.module] if args.module else MODULES
    failed = False
    for module in modules:
        # A fresh netlayer per module: the suite's cases each open their own session.
        netlayer = TestingOnlyTCPNetlayer(peer.hints.get("host"))
        runner = CapTPTestRunner(
            netlayer, peer, args.captp_version, verbosity=2 if args.verbose else 0
        )
        suite = runner.loadTests(module)
        result = runner.run(suite)
        total = result.testsRun
        # **Distinct cases, not entries.** A test that fails in `setUp` is recorded in *both*
        # `failures` and `errors`, so counting the entries would report a negative pass count
        # (measured: `-1/7` on `third_party_handoffs`, where one case is listed twice).
        failed_ids = {case.id() for case, _ in result.failures + result.errors}
        passed = total - len(failed_ids)
        print(f"{module}: {passed}/{total}", flush=True)
        for case_id in sorted(failed_ids):
            print(f"    FAILED {case_id}", flush=True)
        if not result.wasSuccessful():
            failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
