//! Review — output of one reviewer agent (security / bugs / quality).
//!
//! Critic output lives in [`crate::critique`]. Reviewers find defects in the
//! code; critics judge goal-fit. Two different jobs, two different shapes.

use serde::{Deserialize, Serialize};

/// What the reviewer agent found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResult {
    /// Which kind of reviewer produced this output.
    pub kind: ReviewerKind,
    /// Top-level one-liner.
    pub summary: String,
    /// Concrete issues. Empty when the diff is clean for this kind.
    pub findings: Vec<Finding>,
}

/// The three reviewer kinds. Tagged in JSON via `"kind"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerKind {
    Security,
    Bugs,
    Quality,
}

/// A single defect a reviewer flagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    /// Repo-relative file path.
    pub file: String,
    /// Inclusive `(start, end)` line range in the diff. None when unscoped.
    pub lines: Option<(u32, u32)>,
    /// What's wrong.
    pub desc: String,
    /// Concrete suggested change. None when no fix proposed.
    pub suggested_fix: Option<String>,
}

/// Severity tier. Ordered: blocker > major > minor > nit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// PR must not merge as-is.
    Blocker,
    /// Should fix before merge but not catastrophic.
    Major,
    /// Worth fixing.
    Minor,
    /// Style / nice-to-have.
    Nit,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_finding() -> Finding {
        Finding {
            severity: Severity::Major,
            file: "crates/ca-daemon/src/main.rs".to_owned(),
            lines: Some((42, 47)),
            desc: "Unhandled error path returns silently".to_owned(),
            suggested_fix: Some("propagate via `?` and surface in the response".to_owned()),
        }
    }

    #[test]
    fn review_result_serde_roundtrip() {
        let r = ReviewResult {
            kind: ReviewerKind::Bugs,
            summary: "Two findings, one major.".to_owned(),
            findings: vec![sample_finding()],
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: ReviewResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, parsed);
    }

    #[test]
    fn review_result_empty_findings_roundtrip() {
        let r = ReviewResult {
            kind: ReviewerKind::Security,
            summary: "Nothing to flag.".to_owned(),
            findings: vec![],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"kind\":\"security\""));
        let parsed: ReviewResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.findings.is_empty());
    }

    #[test]
    fn finding_without_fix_or_lines_roundtrips() {
        let f = Finding {
            severity: Severity::Nit,
            file: "README.md".to_owned(),
            lines: None,
            desc: "Trailing whitespace".to_owned(),
            suggested_fix: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        let parsed: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(f, parsed);
    }
}
