//! ca-lib — shared types and utilities for the claude_admin v1 orchestrator.
//!
//! This crate is consumed by `ca-daemon`, `ca`, and `ca-tui`. It exposes the
//! domain types used over the UDS protocol and persisted in the daemon's
//! SQLite store.

pub mod architector;
pub mod commit;
pub mod critique;
pub mod review;
pub mod rpc;
pub mod task;

pub use architector::{Architector, ArchitectorOutcome, ArchitectorState};
pub use commit::Commit;
pub use critique::{CritiqueAxes, CritiqueResult, Verdict};
pub use review::{Finding, ReviewResult, ReviewerKind, Severity};
pub use rpc::{RpcRequest, RpcResponse};
pub use task::{Task, TaskStatus};

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
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
