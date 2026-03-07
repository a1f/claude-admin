use crate::plan::{Plan, Step, StepStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrchestratorError {
    #[error("no pending steps found")]
    NoPendingSteps,
    #[error("step not found: {0}")]
    StepNotFound(String),
    #[error("spawn error: {0}")]
    Spawn(#[from] crate::spawn::SpawnError),
}

pub struct BatchResult {
    pub spawned: Vec<SpawnedStep>,
    pub skipped: Vec<String>,
}

pub struct SpawnedStep {
    pub step_id: String,
    pub pane_id: String,
    pub context_file: String,
}

/// Return all steps with Pending status, preserving phase order.
pub fn get_pending_steps(plan: &Plan) -> Vec<&Step> {
    plan.content
        .phases
        .iter()
        .flat_map(|phase| &phase.steps)
        .filter(|step| step.status == StepStatus::Pending)
        .collect()
}

/// Group pending steps by phase for parallel execution.
///
/// Steps within the same phase are considered parallelizable (no ordering
/// dependency). Phases themselves are sequential -- phase N must finish
/// before phase N+1 starts. Only Pending steps are included.
pub fn suggest_parallelizable_steps(plan: &Plan) -> Vec<Vec<String>> {
    plan.content
        .phases
        .iter()
        .map(|phase| {
            phase
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Pending)
                .map(|s| s.id.clone())
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

/// Choose which steps to spawn, respecting max concurrency.
///
/// If `step_ids` is provided, validate they exist and return up to
/// `max_concurrent`. Otherwise auto-select from the first parallelizable
/// group of pending steps.
pub fn select_batch_steps(
    plan: &Plan,
    step_ids: Option<&[String]>,
    max_concurrent: usize,
) -> Result<Vec<String>, OrchestratorError> {
    match step_ids {
        Some(ids) => select_explicit_steps(plan, ids, max_concurrent),
        None => select_auto_steps(plan, max_concurrent),
    }
}

fn select_explicit_steps(
    plan: &Plan,
    step_ids: &[String],
    max_concurrent: usize,
) -> Result<Vec<String>, OrchestratorError> {
    let all_step_ids: Vec<&str> = plan
        .content
        .phases
        .iter()
        .flat_map(|phase| &phase.steps)
        .map(|s| s.id.as_str())
        .collect();

    for id in step_ids {
        if !all_step_ids.contains(&id.as_str()) {
            return Err(OrchestratorError::StepNotFound(id.clone()));
        }
    }

    Ok(step_ids.iter().take(max_concurrent).cloned().collect())
}

fn select_auto_steps(
    plan: &Plan,
    max_concurrent: usize,
) -> Result<Vec<String>, OrchestratorError> {
    let groups = suggest_parallelizable_steps(plan);
    let first_group = groups.first().ok_or(OrchestratorError::NoPendingSteps)?;
    Ok(first_group.iter().take(max_concurrent).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ExitCriteria, Phase, PlanContent, PlanStatus, Step, StepStatus};

    fn make_step(id: &str, status: StepStatus) -> Step {
        Step {
            id: id.to_string(),
            description: format!("Step {id}"),
            status,
            exit_criteria: ExitCriteria {
                description: "done".to_string(),
                commands: vec![],
            },
        }
    }

    fn sample_plan() -> Plan {
        Plan {
            id: 1,
            project_id: 1,
            name: "Test Plan".to_string(),
            content: PlanContent {
                phases: vec![
                    Phase {
                        name: "Setup".to_string(),
                        steps: vec![
                            make_step("0.1", StepStatus::Completed),
                            make_step("0.2", StepStatus::Pending),
                        ],
                    },
                    Phase {
                        name: "Implementation".to_string(),
                        steps: vec![
                            make_step("1.1", StepStatus::Pending),
                            make_step("1.2", StepStatus::Pending),
                            make_step("1.3", StepStatus::Pending),
                        ],
                    },
                    Phase {
                        name: "Polish".to_string(),
                        steps: vec![make_step("2.1", StepStatus::Pending)],
                    },
                ],
            },
            status: PlanStatus::Active,
            created_at: 1706400000,
            updated_at: 1706500000,
        }
    }

    fn all_done_plan() -> Plan {
        Plan {
            id: 2,
            project_id: 1,
            name: "Done Plan".to_string(),
            content: PlanContent {
                phases: vec![Phase {
                    name: "Setup".to_string(),
                    steps: vec![
                        make_step("0.1", StepStatus::Completed),
                        make_step("0.2", StepStatus::Completed),
                    ],
                }],
            },
            status: PlanStatus::Completed,
            created_at: 1706400000,
            updated_at: 1706500000,
        }
    }

    #[test]
    fn test_get_pending_steps() {
        let plan = sample_plan();
        let pending = get_pending_steps(&plan);
        let ids: Vec<&str> = pending.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["0.2", "1.1", "1.2", "1.3", "2.1"]);
    }

    #[test]
    fn test_get_pending_steps_all_completed() {
        let plan = all_done_plan();
        let pending = get_pending_steps(&plan);
        assert!(pending.is_empty());
    }

    #[test]
    fn test_suggest_parallelizable_steps_basic() {
        let plan = sample_plan();
        let groups = suggest_parallelizable_steps(&plan);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], vec!["0.2"]);
        assert_eq!(groups[1], vec!["1.1", "1.2", "1.3"]);
        assert_eq!(groups[2], vec!["2.1"]);
    }

    #[test]
    fn test_suggest_parallelizable_steps_mixed_status() {
        let mut plan = sample_plan();
        plan.content.phases[1].steps[0].status = StepStatus::InProgress;
        plan.content.phases[1].steps[1].status = StepStatus::Completed;

        let groups = suggest_parallelizable_steps(&plan);
        // Phase 0 has 0.2 pending, Phase 1 has only 1.3 pending, Phase 2 has 2.1
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], vec!["0.2"]);
        assert_eq!(groups[1], vec!["1.3"]);
        assert_eq!(groups[2], vec!["2.1"]);
    }

    #[test]
    fn test_suggest_parallelizable_steps_all_done() {
        let plan = all_done_plan();
        let groups = suggest_parallelizable_steps(&plan);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_select_batch_steps_explicit() {
        let plan = sample_plan();
        let ids = vec!["0.2".to_string(), "1.1".to_string(), "1.2".to_string()];
        let result = select_batch_steps(&plan, Some(&ids), 5).unwrap();
        assert_eq!(result, vec!["0.2", "1.1", "1.2"]);
    }

    #[test]
    fn test_select_batch_steps_auto() {
        let plan = sample_plan();
        let result = select_batch_steps(&plan, None, 5).unwrap();
        // First pending group is phase 0 with just "0.2"
        assert_eq!(result, vec!["0.2"]);
    }

    #[test]
    fn test_select_batch_steps_max_respected() {
        let plan = sample_plan();
        // Phase 1 has 3 pending steps; request with --steps and max=2
        let ids = vec![
            "1.1".to_string(),
            "1.2".to_string(),
            "1.3".to_string(),
        ];
        let result = select_batch_steps(&plan, Some(&ids), 2).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result, vec!["1.1", "1.2"]);
    }

    #[test]
    fn test_select_batch_steps_no_pending() {
        let plan = all_done_plan();
        let result = select_batch_steps(&plan, None, 2);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrchestratorError::NoPendingSteps
        ));
    }

    #[test]
    fn test_select_batch_steps_invalid_step() {
        let plan = sample_plan();
        let ids = vec!["0.2".to_string(), "9.9".to_string()];
        let result = select_batch_steps(&plan, Some(&ids), 5);
        assert!(result.is_err());
        match result.unwrap_err() {
            OrchestratorError::StepNotFound(id) => assert_eq!(id, "9.9"),
            other => panic!("expected StepNotFound, got: {other}"),
        }
    }
}
