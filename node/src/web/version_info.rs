//! Node version string (port of `web/VersionInfo.scala`).
//!
//! `VersionInfo.get` is a `val` computed from the sbt-buildinfo `BuildInfo` (`version`,
//! `gitHeadCommit`). The Cargo equivalent of buildinfo is the crate's `build.rs`, which resolves
//! `git rev-parse HEAD` and passes it to the compiler as `GIT_HEAD_COMMIT` — see
//! [`GIT_HEAD_COMMIT`]. The formatter stays a pure function over the two inputs, and
//! [`node_version`] is the single place they are joined, so the three surfaces that report a version
//! (HTTP `/version`, HTTP `/status`, and the gRPC `Status` in `casper`) cannot disagree.

/// The commit this binary was built from, or `None` when the build could not resolve one.
///
/// `option_env!` rather than `env!` on purpose: a build without `git` — a source tarball, a shallow
/// clone `rev-parse` refuses — must compile, and report `commit # unknown` rather than a stale or
/// invented commit. `build.rs` is where the value comes from and where the failure is warned about.
pub const GIT_HEAD_COMMIT: Option<&str> = option_env!("GIT_HEAD_COMMIT");

/// The build commit, or the sentinel that says it is not known.
///
/// One place for the fallback, because it is a claim a reader has to be able to trust: `commit #
/// unknown` must mean *the build could not resolve a commit*, never *the build script was dropped*.
pub fn commit_or_unknown() -> &'static str {
    GIT_HEAD_COMMIT.unwrap_or("commit # unknown")
}

/// The node version string, **as the API reports it** — HTTP `/version`, HTTP `/status` and the gRPC
/// `Status` all read this one function, so a peer cannot be told two different things.
pub fn node_version() -> String {
    get(env!("CARGO_PKG_VERSION"), GIT_HEAD_COMMIT)
}

/// `--version`'s string: the package version and the commit, without the `RChain Node` prefix.
///
/// clap renders `{command} {version}`, so reusing [`node_version`] here would print
/// `rchain RChain Node 0.1.0 (…)` and read as a bug. The *commit* is what the two surfaces have to
/// agree on — it is how a room member ties an attestation to a binary — and they do, because both
/// read [`GIT_HEAD_COMMIT`].
pub fn cli_version() -> String {
    format!("{} ({})", env!("CARGO_PKG_VERSION"), commit_or_unknown())
}

/// Format the node version string (port of `VersionInfo.get`).
pub fn get(version: &str, git_head_commit: Option<&str>) -> String {
    format!(
        "RChain Node {} ({})",
        version,
        git_head_commit.unwrap_or("commit # unknown")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_formats_with_commit() {
        assert_eq!(get("1.0.0", Some("abc123")), "RChain Node 1.0.0 (abc123)");
    }

    #[test]
    fn get_uses_unknown_when_no_commit() {
        assert_eq!(get("1.0.0", None), "RChain Node 1.0.0 (commit # unknown)");
    }

    /// **The build script's value is compiled in, and this is the test that fails if it stops
    /// being.** `node_version` reads `GIT_HEAD_COMMIT`, which `build.rs` emits; deleting that line
    /// turns this red rather than quietly restoring `commit # unknown`, which is the failure mode
    /// worth pinning — a provenance string that reports nothing looks exactly like a provenance
    /// string that works.
    ///
    /// It asserts `is_some` rather than branching on it. A `match` with a fallback arm would pass on
    /// a tree with no provenance at all, and this crate is built from a git checkout (CI checks one
    /// out) so "no commit resolved" is a defect here, not a supported configuration. The length is
    /// checked as 40 or 64 hex characters, because a truncated or abbreviated sha would otherwise
    /// satisfy `is_some` while naming a commit nobody can look up.
    #[test]
    fn the_build_commit_is_embedded_and_is_a_full_sha() {
        let commit = GIT_HEAD_COMMIT.unwrap_or_else(|| {
            panic!(
                "no GIT_HEAD_COMMIT in this build: `node/build.rs` must resolve `git rev-parse HEAD` \
                 and emit it with `cargo:rustc-env` (a build outside a git checkout is the one \
                 legitimate cause, and it warns)"
            )
        });
        assert!(
            matches!(commit.len(), 40 | 64) && commit.chars().all(|c| c.is_ascii_hexdigit()),
            "GIT_HEAD_COMMIT must be a full object id, got {commit:?}"
        );
        assert_eq!(
            node_version(),
            format!("RChain Node {} ({commit})", env!("CARGO_PKG_VERSION")),
            "the commit must be the string the version reports"
        );
    }
}
