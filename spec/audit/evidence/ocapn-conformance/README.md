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

`run-1.txt` is the raw output of the first whole-suite run (`failures=9, errors=8`);
`run-2.txt` is the same run after `op:listen` was implemented (`failures=6, errors=8`). **Read the
per-module numbers, not a summary line.** The runner reports a `setUp` error against the test it
aborted as well as the error itself, so its tallies exceed the test count; running each module on its
own gives the numbers below.

## Per module

| Module | Passed | Note |
|---|---|---|
| `op_abort` | **1 / 1** | ✅ |
| `op_deliver` | **4 / 4** | ✅ including both promise-pipelining tests and the break-propagation test |
| `op_start_session` | **3 / 5** | the two failures are the crossed-hellos tests, which need the sturdyref enlivener (below) |
| `op_listen` | **3 / 3** | ✅ the promise-resolver fixture, heard both before and after the settlement |
| `op_gc` | 0 / 4 | `op:gc-exports` / `op:gc-answers` are not emitted at all |
| `third_party_handoffs` | 1 / 7 | the one pass is an *invalid-signature* rejection that passes incidentally: this port answers every handoff with a `break` |
| **Total** | **12 / 24** | |

So the implemented path is **stages 0–2 of the implementation guide**: the handshake (with the two
refusals it must make), `op:deliver`, the export table, promise pipelining through the answer table,
`fulfill`/`break` through `resolve-me-desc`, and `op:listen` with its promise/resolver pair. GC
(stage 3) and handoffs (stage 6) are the stages not yet built, and the suite says exactly that —
which is the point of running it.

**Not implemented, and named in the code rather than guessed at:** the suite's *sturdyref
enlivener* (`gi02I1qghIwPiKGKleCQAOhpy3ZtYRpB`), which must dial a peer back from a sturdyref it is
handed. A `fetch` of it breaks, which is why the two crossed-hellos tests and most of the handoff
tests fail at `setUp`.

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
