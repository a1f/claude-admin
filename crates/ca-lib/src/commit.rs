//! Commit — a coder commit observed by the task-processor.

use serde::{Deserialize, Serialize};

/// One commit pushed by the coder to a task's branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    /// Full git SHA.
    pub sha: String,
    /// Branch the commit is on (e.g. `"M0-T3"`).
    pub branch: String,
    /// First line of the commit message (subject).
    pub subject: String,
    /// ISO-8601 timestamp from `git log --format=%aI`.
    pub authored_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_serde_roundtrip() {
        let c = Commit {
            sha: "ec03b18a1b2c3d4e5f6789abcdef0123456789ab".to_owned(),
            branch: "M0-T2".to_owned(),
            subject: "[M0-T2] Cargo workspace scaffold".to_owned(),
            authored_at: "2026-05-03T22:15:00Z".to_owned(),
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: Commit = serde_json::from_str(&json).unwrap();
        assert_eq!(c, parsed);
    }
}
