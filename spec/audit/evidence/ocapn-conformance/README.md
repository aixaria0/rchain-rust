# OCapN conformance: the first run

**What this is.** The result of running `ocapn/ocapn-test-suite` — the OCapN project's own
conformance suite, the thing every implementation is checked against — against this repo's
`ocapn-tcp-testing` peer. It is the first time this port has spoken CapTP to a foreign
implementation rather than to itself.

## The run

| | |
|---|---|
| Date | 2026-10-05 |
| Implementation under test | `rchain-ocapn`, branch `ocapn/ertp-interop`, `cargo build -p rchain-ocapn --bin ocapn-tcp-testing` |
| Suite | `github.com/ocapn/ocapn-test-suite` at `31f0b80` |
| Netlayer | `tcp-testing-only` (the suite's own; no encryption — it is not a deployment transport) |

`run-1.txt` is the raw output of one whole-suite run. **Read the per-module numbers, not that
run's summary line.** The runner reports `failures=9, errors=8` over 24 tests, but it counts a
`setUp` error against the test it aborted as well as reporting the error, so its tallies exceed the
test count; running each module on its own gives the numbers below.

## Per module

| Module | Passed | Note |
|---|---|---|
| `op_abort` | **1 / 1** | ✅ |
| `op_deliver` | **4 / 4** | ✅ including both promise-pipelining tests and the break-propagation test |
| `op_start_session` | **3 / 5** | the two failures are the crossed-hellos tests, which need the sturdyref enlivener (below) |
| `op_gc` | 0 / 4 | `op:gc-exports` / `op:gc-answers` are not emitted at all |
| `op_listen` | 0 / 3 | `op:listen` is unimplemented |
| `third_party_handoffs` | 1 / 7 | the one pass is an *invalid-signature* rejection that passes incidentally: this port answers every handoff with a `break` |
| **Total** | **9 / 24** | |

So the implemented path is **stages 0–2 of the implementation guide**: the handshake (with the two
refusals it must make), `op:deliver`, the export table, promise pipelining through the answer table,
and `fulfill`/`break` through `resolve-me-desc`. GC, `op:listen`, and handoffs are the stages not yet
built, and the suite says exactly that — which is the point of running it.

**Not implemented, and named in the code rather than guessed at:** the suite's *sturdyref
enlivener* (`gi02I1qghIwPiKGKleCQAOhpy3ZtYRpB`), which must dial a peer back from a sturdyref it is
handed, and the *promise resolver* (`IokCxYmMj04nos2JN1TDoY1bT8dXh6Lr`). A `fetch` of either breaks.

## How to reproduce

```sh
cargo build -p rchain-ocapn --bin ocapn-tcp-testing
./target/debug/ocapn-tcp-testing 127.0.0.1:22045 &   # a fresh session key per session
python3 <suite>/test_runner.py \
    'ocapn://rnode-ocapn.tcp-testing-only?host=127.0.0.1&port=22045' -v
```

`test_runner.py` imports the Tor netlayer at load time, so it wants `python-stem` even for a TCP
run; driving `CapTPTestRunner` directly avoids that. The suite needs Python 3.10+ and
`cryptography`. Add `--test-module tests.op_deliver` (or any module) to run one at a time.

## Why this file exists

The numbers here are a *measurement*, and a measurement whose configuration no longer exists is
worth nothing (the lesson of AUDIT C215). The suite revision, the port's branch, and the per-module
counts are recorded so a later run can be compared against them rather than remembered.
