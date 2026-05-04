//! Task — one PR-shaped unit of work within a milestone breakdown.

use serde::{Deserialize, Serialize};

/// A single dispatchable task. Lifetime: created during breakdown, advances
/// through states, terminates as merged or dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Task identifier, format `<milestone-id>-T<n>` (e.g. `"M0-T1"`).
    pub id: String,
    /// One-line action title.
    pub title: String,
    /// What ships. Concrete description of the deliverable.
    pub deliverable: String,
    /// Other task IDs (or labels) that must be satisfied before dispatch.
    /// Empty vec = no blockers.
    pub blockers: Vec<String>,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Coder-side estimate of net new lines of code. None when not estimated.
    pub estimated_loc: Option<u32>,
}

/// Task lifecycle. Tagged in JSON via `"status"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Defined in a breakdown but not yet dispatchable.
    Planned,
    /// Architector kicked off; coder being spawned.
    Dispatched,
    /// Coder is actively writing code.
    Coding,
    /// Coder pushed a draft PR; reviewers + critics in flight.
    Reviewing,
    /// Architect decided "iterate" or "drop"; awaiting follow-up.
    Iterating,
    /// Architect approved; awaiting merge.
    Drafted,
    /// PR landed on main.
    Merged,
    /// Architect or user closed the PR without merging.
    Dropped,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Task {
        Task {
            id: "M0-T1".to_owned(),
            title: "Archive existing crates to legacy/pre-v1".to_owned(),
            deliverable: "legacy/pre-v1 branch + crates/ removed from main".to_owned(),
            blockers: vec![],
            status: TaskStatus::Merged,
            estimated_loc: Some(30),
        }
    }

    #[test]
    fn task_serde_roundtrip() {
        let t = sample();
        let json = serde_json::to_string(&t).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(t, parsed);
    }

    #[test]
    fn task_status_serializes_snake_case() {
        let json = serde_json::to_string(&TaskStatus::Reviewing).unwrap();
        assert_eq!(json, "\"reviewing\"");
    }

    #[test]
    fn task_with_blockers_roundtrips() {
        let t = Task {
            blockers: vec![
                "M0-T1 merged".to_owned(),
                "label:design-approved".to_owned(),
            ],
            status: TaskStatus::Planned,
            estimated_loc: None,
            ..sample()
        };
        let json = serde_json::to_string(&t).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(t, parsed);
        assert_eq!(parsed.blockers.len(), 2);
    }
}
