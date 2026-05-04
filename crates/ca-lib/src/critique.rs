//! Critique — output of one adversarial critic agent.
//!
//! Critics judge whether a PR actually achieves the goal of its task — distinct
//! from reviewers ([`crate::review`]) who chase defects in the code itself.
//! Multiple critic instances run in parallel; their scores are aggregated by
//! the task-processor (median over instances).

use serde::{Deserialize, Serialize};

/// One critic's verdict on a PR. Multiple instances per PR; medians are
/// computed by the consumer (task-processor or daemon).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CritiqueResult {
    /// Overall score, 0..=100.
    pub score: u8,
    /// Bucket derived from score.
    pub verdict: Verdict,
    /// Per-axis breakdown.
    pub axes: CritiqueAxes,
    /// 2–4 sentences justifying the score, citing specific evidence.
    pub rationale_md: String,
    /// Concrete concerns. May be empty.
    pub concerns: Vec<String>,
}

/// Score buckets. Thresholds: strong ≥85, acceptable 70–84, weak 50–69, reject <50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Strong,
    Acceptable,
    Weak,
    Reject,
}

/// Per-axis sub-scores (each 0..=100).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CritiqueAxes {
    pub achieves_goal: u8,
    pub test_coverage: u8,
    pub no_scope_creep: u8,
    pub reuses_existing: u8,
    pub validation_evidence: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CritiqueResult {
        CritiqueResult {
            score: 88,
            verdict: Verdict::Strong,
            axes: CritiqueAxes {
                achieves_goal: 92,
                test_coverage: 85,
                no_scope_creep: 95,
                reuses_existing: 80,
                validation_evidence: 90,
            },
            rationale_md: "PR implements the breakdown's goal cleanly.".to_owned(),
            concerns: vec!["Plan drift on User.preferences field".to_owned()],
        }
    }

    #[test]
    fn critique_result_serde_roundtrip() {
        let c = sample();
        let json = serde_json::to_string(&c).unwrap();
        let parsed: CritiqueResult = serde_json::from_str(&json).unwrap();
        assert_eq!(c, parsed);
    }

    #[test]
    fn critique_result_empty_concerns_roundtrips() {
        let c = CritiqueResult {
            concerns: vec![],
            ..sample()
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: CritiqueResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.concerns.is_empty());
    }

    #[test]
    fn verdict_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Verdict::Reject).unwrap(),
            "\"reject\""
        );
        assert_eq!(
            serde_json::to_string(&Verdict::Acceptable).unwrap(),
            "\"acceptable\""
        );
    }
}
