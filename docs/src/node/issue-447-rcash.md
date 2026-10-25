# Issue #447: exact-source r-cash verified 17-step install

The issue distinguishes two application workloads: the **verified installer**
and a **no-spacing, roughly three-minute burst**. They are not interchangeable.

This job takes a disposable, funded one-validator Rholo Docker genesis at a
specified node source SHA, then fetches the public original r-cash source at
a fixed SHA: 0c1cc6e4b2c72fb80aeaae51ec92a2749b9a50a4.
It builds the genuine r-cash Rust CLI and contracts, generates only ephemeral
devnet key files, then calls the upstream install_testnet.py harness. The
installer checks each dependent Root key before moving to the next step;
the independent evidence gate refuses a partial, reused, missing, or unpinned
install and requires exactly 17 distinct freshly included steps.

Run on GitHub Actions:

    gh workflow run devnet-fuzz.yml -R aixaria0/rchain-rust \
      --ref verification/issue-447-rcash-installer \
      -f mode=issue447-rcash

The artifact includes both source SHAs, built image ID, per-step deploy and
block IDs, resolved-key counts, logs and failures. The repository's generated
secret-key files are excluded from artifact uploads. The external Python
requirements specify version floors rather than exact hashes; the run captures
their output but is not a bit-for-bit dependency-reproducible build.

**Do not label this a 4-validator application partition acceptance.** It runs
one validator, solely to isolate installer ordering and verify actual
dependencies. In particular, it does not exercise C336 same-channel rejection,
the 17-deploy unpaced burst, post-partition N=4 recovery, or 8-16 GiB resource
admission. The user-facing production testnet is never modified.
