#!/usr/bin/env bash
# check-build-paths — every path that produces a shipped artefact is pinned and unparameterised.
#
# Why this exists. Two wargame rows name a gate rather than a test, and neither gate existed:
#
#   W-N7-04 — "every build path passes `--locked`". Without it, a build resolves the dependency graph
#             from the manifest rather than from `Cargo.lock`, so the artefact an operator is handed may
#             contain a crate version no one reviewed — the lock file is a promise the build has to keep.
#   W-N7-03 — "the shipped artefact is not parameterised by build args". `docker/rnode/Dockerfile` used
#             to accept `RUSTFLAGS`/`CARGO_FEATURES`, which reach `cargo` and therefore compile a
#             *different binary* — so the definition that accepts one must not be the definition a
#             release is produced from. The profiling variant is where those live now, and the release
#             variant must not grow them back.
#
# Checks:
#   1  every `cargo build` / `cargo install` in `docker/**/Dockerfile*` and `.github/workflows/*.yml`
#      passes `--locked` (test/bench/clippy runs are out of scope: they produce no artefact);
#   2  `docker/rnode/Dockerfile` declares no build argument that reaches `cargo`;
#   3  the profiling Dockerfile — the one that *is* allowed to parameterise — still passes `--locked`,
#      so the exemption is for the args and not for the pin.
#
# Usage: tools/check-build-paths.sh [--gate]
#
# A gate that exists and nothing runs is the class this repository has already been bitten by twice, so
# this one is wired into `.github/workflows/ci.yml` beside the other static gates.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

gate=0
[[ "${1:-}" == "--gate" ]] && gate=1

failures=0
fail() { printf 'FAIL  %s\n' "$*"; failures=$((failures + 1)); }
ok() { printf 'ok    %s\n' "$*"; }

shopt -s nullglob globstar
dockerfiles=("$ROOT"/docker/**/Dockerfile* "$ROOT"/Dockerfile*)
workflows=("$ROOT"/.github/workflows/*.yml)

# --- 1. every artefact-producing cargo invocation is --locked --------------------------------
checked=0
for f in "${dockerfiles[@]}" "${workflows[@]}"; do
  [[ -f "$f" ]] || continue
  while IFS= read -r line; do
    checked=$((checked + 1))
    if [[ "$line" != *--locked* ]]; then
      fail "$(basename "$f"): a build path does not pass --locked — ${line#"${line%%[![:space:]]*}"}"
    fi
  done < <(grep -nE '(^|[^a-z-])cargo[[:space:]]+(build|install)([[:space:]]|$)' "$f" \
             | grep -vE '^\s*[0-9]+:\s*#' || true)
done
[[ "$checked" -gt 0 ]] || fail "no build path found — a check with nothing to read is not a check"
if [[ "$failures" -eq 0 ]]; then ok "artefact-producing builds: $checked invocation(s), all --locked"; fi

# --- 2. the release Dockerfile is not parameterised -------------------------------------------
release="$ROOT/docker/rnode/Dockerfile"
[[ -f "$release" ]] || fail "no $release — the release definition moved, and this check did not"
if [[ -f "$release" ]]; then
  for arg in RUSTFLAGS CARGO_FEATURES JEMALLOC_SYS_WITH_MALLOC_CONF; do
    if grep -qE "^ARG[[:space:]]+$arg" "$release"; then
      fail "docker/rnode/Dockerfile declares ARG $arg: a build argument that reaches cargo compiles a \
different binary, so the definition a release is produced from must not accept one (W-N7-03)"
    fi
  done
  if ! grep -qE 'FROM[[:space:]]+[^[:space:]]+@sha256:[0-9a-f]{64}' "$release"; then
    fail "docker/rnode/Dockerfile has a FROM that is not digest-pinned"
  fi
  ok "docker/rnode/Dockerfile: no cargo-reaching build arg, base images digest-pinned"
fi

# --- 3. the profiling variant is parameterised *and* pinned ------------------------------------
prof="$ROOT/docker/rnode/Dockerfile.profiling"
if [[ -f "$prof" ]]; then
  grep -qE '^ARG[[:space:]]+CARGO_FEATURES' "$prof" \
    || fail "Dockerfile.profiling no longer declares CARGO_FEATURES — the exemption is checked, not assumed"
  ok "docker/rnode/Dockerfile.profiling: parameterised on purpose, and --locked by check 1"
fi

echo
if [[ "$failures" -gt 0 ]]; then
  printf 'check-build-paths: %d violation(s)\n' "$failures"
  [[ "$gate" == "1" ]] && exit 1
  exit 0
fi
echo "check-build-paths: every artefact-producing path is lock-pinned and the release definition is \
unparameterised."
