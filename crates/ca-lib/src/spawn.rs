use crate::plan::{Plan, StepStatus};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("tmux not running")]
    TmuxNotRunning,
    #[error("tmux command failed: {0}")]
    TmuxFailed(String),
    #[error("step not found: {0}")]
    StepNotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct SpawnOptions {
    pub working_dir: String,
    pub context_file: Option<String>,
    pub window_name: Option<String>,
}

pub fn generate_plan_context(plan: &Plan, step_id: &str) -> Result<String, SpawnError> {
    let all_steps: Vec<_> = plan
        .content
        .phases
        .iter()
        .flat_map(|phase| &phase.steps)
        .collect();

    let current_step = all_steps
        .iter()
        .find(|s| s.id == step_id)
        .ok_or_else(|| SpawnError::StepNotFound(step_id.to_string()))?;

    let completed = all_steps
        .iter()
        .filter(|s| s.status == StepStatus::Completed)
        .count();
    let in_progress = all_steps
        .iter()
        .filter(|s| s.status == StepStatus::InProgress)
        .count();
    let total = all_steps.len();
    let remaining = total - completed - in_progress;

    let mut out = String::new();

    writeln!(out, "# Plan: {}", plan.name).unwrap();
    writeln!(out).unwrap();

    write_goal_section(&mut out, plan);
    write_progress_section(&mut out, completed, in_progress, remaining, total);
    write_current_step_section(&mut out, current_step);
    write_completed_steps(&mut out, &all_steps);
    write_remaining_steps(&mut out, &all_steps, step_id);

    Ok(out)
}

fn write_goal_section(out: &mut String, plan: &Plan) {
    writeln!(out, "## Goal").unwrap();
    if let Some(first_phase) = plan.content.phases.first() {
        writeln!(out, "{}", first_phase.name).unwrap();
    }
    writeln!(out).unwrap();
}

fn write_progress_section(
    out: &mut String,
    completed: usize,
    in_progress: usize,
    remaining: usize,
    total: usize,
) {
    writeln!(out, "## Progress").unwrap();
    writeln!(out, "- Completed: {completed}/{total} steps").unwrap();
    writeln!(out, "- In Progress: {in_progress} steps").unwrap();
    writeln!(out, "- Remaining: {remaining} steps").unwrap();
    writeln!(out).unwrap();
}

fn write_current_step_section(out: &mut String, step: &crate::plan::Step) {
    writeln!(out, "## Current Step: {} - {}", step.id, step.description).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "### Exit Criteria").unwrap();
    writeln!(out, "{}", step.exit_criteria.description).unwrap();
    writeln!(out).unwrap();

    if !step.exit_criteria.commands.is_empty() {
        writeln!(out, "### Validation Commands").unwrap();
        for cmd in &step.exit_criteria.commands {
            writeln!(out, "- {cmd}").unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn write_completed_steps(out: &mut String, all_steps: &[&crate::plan::Step]) {
    let completed: Vec<_> = all_steps
        .iter()
        .filter(|s| s.status == StepStatus::Completed)
        .collect();

    if !completed.is_empty() {
        writeln!(out, "## Completed Steps").unwrap();
        for step in completed {
            writeln!(out, "- [x] {}: {}", step.id, step.description).unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn write_remaining_steps(out: &mut String, all_steps: &[&crate::plan::Step], current_id: &str) {
    let remaining: Vec<_> = all_steps
        .iter()
        .filter(|s| s.status != StepStatus::Completed && s.id != current_id)
        .collect();

    if !remaining.is_empty() {
        writeln!(out, "## Remaining Steps").unwrap();
        for step in remaining {
            writeln!(out, "- [ ] {}: {}", step.id, step.description).unwrap();
        }
        writeln!(out).unwrap();
    }
}

pub fn write_context_file(context: &str) -> Result<PathBuf, SpawnError> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let path = PathBuf::from(format!("/tmp/claude-admin-context-{timestamp}.md"));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(context.as_bytes())?;
    Ok(path)
}

pub fn spawn_tmux_session(opts: &SpawnOptions) -> Result<String, SpawnError> {
    if !is_tmux_available() {
        return Err(SpawnError::TmuxNotRunning);
    }

    let window_name = opts.window_name.as_deref().unwrap_or("claude");
    create_tmux_window(window_name, &opts.working_dir)?;

    let pane_id = get_last_window_pane_id()?;
    send_claude_command(&pane_id, opts.context_file.as_deref())?;

    Ok(pane_id)
}

fn is_tmux_available() -> bool {
    Command::new("tmux")
        .args(["list-sessions"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn create_tmux_window(name: &str, working_dir: &str) -> Result<(), SpawnError> {
    let output = Command::new("tmux")
        .args(["new-window", "-n", name, "-c", working_dir])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SpawnError::TmuxFailed(stderr.into_owned()));
    }
    Ok(())
}

fn get_last_window_pane_id() -> Result<String, SpawnError> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "-t", "!", "#{pane_id}"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SpawnError::TmuxFailed(stderr.into_owned()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn send_claude_command(pane_id: &str, context_file: Option<&str>) -> Result<(), SpawnError> {
    let cmd = match context_file {
        Some(path) => format!("claude --resume --prompt-file {path}"),
        None => "claude --resume".to_string(),
    };

    let output = Command::new("tmux")
        .args(["send-keys", "-t", pane_id, &cmd, "Enter"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SpawnError::TmuxFailed(stderr.into_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ExitCriteria, Phase, Plan, PlanContent, PlanStatus, Step, StepStatus};

    fn sample_plan() -> Plan {
        Plan {
            id: 1,
            project_id: 1,
            name: "Auth Feature".to_string(),
            content: PlanContent {
                phases: vec![
                    Phase {
                        name: "Setup".to_string(),
                        steps: vec![Step {
                            id: "0.1".to_string(),
                            description: "Initialize project".to_string(),
                            status: StepStatus::Completed,
                            exit_criteria: ExitCriteria {
                                description: "Project compiles".to_string(),
                                commands: vec!["cargo build".to_string()],
                            },
                        }],
                    },
                    Phase {
                        name: "Implementation".to_string(),
                        steps: vec![
                            Step {
                                id: "1.1".to_string(),
                                description: "Add user model".to_string(),
                                status: StepStatus::Pending,
                                exit_criteria: ExitCriteria {
                                    description: "Tests pass".to_string(),
                                    commands: vec![
                                        "cargo test".to_string(),
                                        "cargo clippy".to_string(),
                                    ],
                                },
                            },
                            Step {
                                id: "1.2".to_string(),
                                description: "Add login endpoint".to_string(),
                                status: StepStatus::Pending,
                                exit_criteria: ExitCriteria {
                                    description: "Endpoint responds".to_string(),
                                    commands: vec![],
                                },
                            },
                        ],
                    },
                ],
            },
            status: PlanStatus::Active,
            created_at: 1706400000,
            updated_at: 1706500000,
        }
    }

    #[test]
    fn test_generate_plan_context_basic() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        assert!(context.contains("# Plan: Auth Feature"));
        assert!(context.contains("## Goal"));
        assert!(context.contains("## Progress"));
        assert!(context.contains("## Current Step: 1.1 - Add user model"));
        assert!(context.contains("## Completed Steps"));
        assert!(context.contains("## Remaining Steps"));
    }

    #[test]
    fn test_generate_plan_context_step_not_found() {
        let plan = sample_plan();
        let result = generate_plan_context(&plan, "9.9");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SpawnError::StepNotFound(_)));
        assert!(err.to_string().contains("9.9"));
    }

    #[test]
    fn test_generate_plan_context_progress_tracking() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        // 1 completed out of 3 total, 0 in progress, 2 remaining
        // (but current step 1.1 is Pending, so it counts as remaining minus itself)
        assert!(context.contains("Completed: 1/3 steps"));
        assert!(context.contains("In Progress: 0 steps"));
        assert!(context.contains("Remaining: 2 steps"));
    }

    #[test]
    fn test_generate_plan_context_includes_exit_criteria() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        assert!(context.contains("### Exit Criteria"));
        assert!(context.contains("Tests pass"));
    }

    #[test]
    fn test_generate_plan_context_includes_commands() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        assert!(context.contains("### Validation Commands"));
        assert!(context.contains("- cargo test"));
        assert!(context.contains("- cargo clippy"));
    }

    #[test]
    fn test_generate_plan_context_no_commands_section_when_empty() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.2").unwrap();

        // Step 1.2 has empty commands vec
        assert!(!context.contains("### Validation Commands"));
    }

    #[test]
    fn test_generate_plan_context_completed_steps_listed() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        assert!(context.contains("[x] 0.1: Initialize project"));
    }

    #[test]
    fn test_generate_plan_context_remaining_excludes_current() {
        let plan = sample_plan();
        let context = generate_plan_context(&plan, "1.1").unwrap();

        // Remaining should list 1.2 but not 1.1 (current) or 0.1 (completed)
        assert!(context.contains("[ ] 1.2: Add login endpoint"));
        assert!(!context.contains("[ ] 1.1"));
        assert!(!context.contains("[ ] 0.1"));
    }

    #[test]
    fn test_write_context_file() {
        let content = "# Test context\nSome content here.";
        let path = write_context_file(content).unwrap();

        assert!(path.exists());
        assert!(
            path.to_string_lossy()
                .starts_with("/tmp/claude-admin-context-")
        );
        assert!(path.to_string_lossy().ends_with(".md"));

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, content);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_spawn_options_construction() {
        let opts = SpawnOptions {
            working_dir: "/home/user/project".to_string(),
            context_file: Some("/tmp/ctx.md".to_string()),
            window_name: Some("step-1.1".to_string()),
        };

        assert_eq!(opts.working_dir, "/home/user/project");
        assert_eq!(opts.context_file.as_deref(), Some("/tmp/ctx.md"));
        assert_eq!(opts.window_name.as_deref(), Some("step-1.1"));
    }

    #[test]
    fn test_spawn_options_no_optionals() {
        let opts = SpawnOptions {
            working_dir: "/tmp".to_string(),
            context_file: None,
            window_name: None,
        };

        assert!(opts.context_file.is_none());
        assert!(opts.window_name.is_none());
    }

    #[test]
    fn test_spawn_error_display() {
        assert_eq!(SpawnError::TmuxNotRunning.to_string(), "tmux not running");
        assert_eq!(
            SpawnError::TmuxFailed("exit 1".to_string()).to_string(),
            "tmux command failed: exit 1"
        );
        assert_eq!(
            SpawnError::StepNotFound("2.3".to_string()).to_string(),
            "step not found: 2.3"
        );
    }
}
