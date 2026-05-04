//! ca-lib — shared types and utilities for the claude_admin v1 orchestrator.
//!
//! This crate is consumed by `ca-daemon`, `ca`, and `ca-tui`.
//! Real types (`Architector`, `Task`, `Commit`, `ReviewResult`,
//! `CritiqueResult`, `RpcRequest`, `RpcResponse`) land in M0-T3.

/// Returns the package version string for `ca-lib`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_returns_non_empty_string() {
        assert!(!version().is_empty());
    }

    #[test]
    fn version_matches_cargo_toml() {
        // CARGO_PKG_VERSION is set at compile time from Cargo.toml; cross-check that
        // the literal we ship matches what the surrounding workspace expects.
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
