use crate::plan::{Plan, PlanContent, StepStatus};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum VersionError {
    Io(std::io::Error),
    Git(String),
    Json(serde_json::Error),
    NoRepo,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::Io(e) => write!(f, "IO error: {e}"),
            VersionError::Git(msg) => write!(f, "Git error: {msg}"),
            VersionError::Json(e) => write!(f, "JSON error: {e}"),
            VersionError::NoRepo => write!(f, "Not a git repository"),
        }
    }
}

impl std::error::Error for VersionError {}

impl From<std::io::Error> for VersionError {
    fn from(e: std::io::Error) -> Self {
        VersionError::Io(e)
    }
}

impl From<serde_json::Error> for VersionError {
    fn from(e: serde_json::Error) -> Self {
        VersionError::Json(e)
    }
}

#[derive(Debug, Clone)]
pub struct PlanVersion {
    pub hash: String,
    pub short_hash: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct StepDiff {
    pub step_id: String,
    pub old_status: Option<StepStatus>,
    pub new_status: Option<StepStatus>,
}

/// Returns the plan storage directory, creating it if needed.
fn plans_dir(repo_root: &Path) -> Result<PathBuf, VersionError> {
    let dir = repo_root.join(".claude-admin").join("plans");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Returns the file path for a plan's JSON snapshot.
fn plan_file(repo_root: &Path, plan_id: i64) -> Result<PathBuf, VersionError> {
    let dir = plans_dir(repo_root)?;
    Ok(dir.join(format!("{plan_id}.json")))
}

fn rel_plan_path(repo_root: &Path, plan_id: i64) -> Result<String, VersionError> {
    let file = plan_file(repo_root, plan_id)?;
    Ok(file
        .strip_prefix(repo_root)
        .unwrap_or(&file)
        .to_string_lossy()
        .to_string())
}

/// Find the git repository root, or return NoRepo.
pub fn find_repo_root(start: &Path) -> Result<PathBuf, VersionError> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()?;
    if !output.status.success() {
        return Err(VersionError::NoRepo);
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Save a plan snapshot as JSON and create a git commit.
pub fn save_version(repo_root: &Path, plan: &Plan, message: &str) -> Result<String, VersionError> {
    let file = plan_file(repo_root, plan.id)?;
    let json = serde_json::to_string_pretty(&plan.content)?;
    std::fs::write(&file, &json)?;

    let rel_path = rel_plan_path(repo_root, plan.id)?;
    git_command(repo_root, &["add", &rel_path])?;

    let commit_msg = format!("plan({}): {}", plan.id, message);
    git_command(repo_root, &["commit", "-m", &commit_msg, "--", &rel_path])?;

    let hash = git_command(repo_root, &["rev-parse", "HEAD"])?;
    Ok(hash.trim().to_string())
}

/// List all versions (git commits) for a plan file.
pub fn list_versions(repo_root: &Path, plan_id: i64) -> Result<Vec<PlanVersion>, VersionError> {
    let rel_path = rel_plan_path(repo_root, plan_id)?;
    let output = git_command(
        repo_root,
        &["log", "--format=%H|%h|%ai|%s", "--follow", "--", &rel_path],
    )?;

    let versions = output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            PlanVersion {
                hash: parts.first().unwrap_or(&"").to_string(),
                short_hash: parts.get(1).unwrap_or(&"").to_string(),
                date: parts.get(2).unwrap_or(&"").to_string(),
                message: parts.get(3).unwrap_or(&"").to_string(),
            }
        })
        .collect();

    Ok(versions)
}

/// Get plan content from a specific version (git commit hash).
pub fn get_version_content(
    repo_root: &Path,
    plan_id: i64,
    hash: &str,
) -> Result<PlanContent, VersionError> {
    let rel_path = rel_plan_path(repo_root, plan_id)?;
    let json = git_command(repo_root, &["show", &format!("{hash}:{rel_path}")])?;
    let content: PlanContent = serde_json::from_str(&json)?;
    Ok(content)
}

/// Diff two plan versions, returning step status changes.
pub fn diff_versions(old: &PlanContent, new: &PlanContent) -> Vec<StepDiff> {
    let old_steps: HashMap<String, StepStatus> = old
        .phases
        .iter()
        .flat_map(|p| p.steps.iter())
        .map(|s| (s.id.clone(), s.status))
        .collect();

    let new_steps: HashMap<String, StepStatus> = new
        .phases
        .iter()
        .flat_map(|p| p.steps.iter())
        .map(|s| (s.id.clone(), s.status))
        .collect();

    let mut diffs = Vec::new();

    // Steps changed or added in new
    for (id, new_status) in &new_steps {
        let old_status = old_steps.get(id).copied();
        if old_status != Some(*new_status) {
            diffs.push(StepDiff {
                step_id: id.clone(),
                old_status,
                new_status: Some(*new_status),
            });
        }
    }

    // Steps removed in new
    for (id, old_status) in &old_steps {
        if !new_steps.contains_key(id) {
            diffs.push(StepDiff {
                step_id: id.clone(),
                old_status: Some(*old_status),
                new_status: None,
            });
        }
    }

    diffs.sort_by(|a, b| a.step_id.cmp(&b.step_id));
    diffs
}

fn git_command(repo_root: &Path, args: &[&str]) -> Result<String, VersionError> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(VersionError::Git(stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ExitCriteria, Phase, PlanContent, PlanStatus, Step, StepStatus};
    use tempfile::TempDir;

    fn make_plan(id: i64, steps: Vec<(&str, StepStatus)>) -> Plan {
        Plan {
            id,
            project_id: 1,
            name: "Test Plan".to_string(),
            content: make_content(steps),
            status: PlanStatus::Active,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    fn make_content(steps: Vec<(&str, StepStatus)>) -> PlanContent {
        PlanContent {
            phases: vec![Phase {
                name: "Phase 1".to_string(),
                steps: steps
                    .into_iter()
                    .map(|(id, status)| Step {
                        id: id.to_string(),
                        description: format!("Step {id}"),
                        status,
                        exit_criteria: ExitCriteria {
                            description: "Done".to_string(),
                            commands: vec![],
                        },
                    })
                    .collect(),
            }],
        }
    }

    fn setup_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // Initial commit so HEAD exists
        std::fs::write(dir.path().join("README"), "init").unwrap();
        std::process::Command::new("git")
            .args(["add", "README"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn test_save_creates_git_commit() {
        let dir = setup_git_repo();
        let plan = make_plan(
            1,
            vec![("1.1", StepStatus::Pending), ("1.2", StepStatus::Pending)],
        );

        let hash = save_version(dir.path(), &plan, "initial save").unwrap();
        assert!(!hash.is_empty());
        assert!(hash.len() >= 7);

        // Verify file exists
        let file = dir
            .path()
            .join(".claude-admin")
            .join("plans")
            .join("1.json");
        assert!(file.exists());
    }

    #[test]
    fn test_list_versions_from_git_log() {
        let dir = setup_git_repo();
        let plan = make_plan(1, vec![("1.1", StepStatus::Pending)]);
        save_version(dir.path(), &plan, "v1").unwrap();

        let mut plan2 = plan.clone();
        plan2.content.phases[0].steps[0].status = StepStatus::Completed;
        save_version(dir.path(), &plan2, "v2").unwrap();

        let versions = list_versions(dir.path(), 1).unwrap();
        assert_eq!(versions.len(), 2);
        assert!(versions[0].message.contains("v2"));
        assert!(versions[1].message.contains("v1"));
    }

    #[test]
    fn test_diff_between_versions_shows_step_changes() {
        let old = make_content(vec![
            ("1.1", StepStatus::Pending),
            ("1.2", StepStatus::Pending),
        ]);
        let new = make_content(vec![
            ("1.1", StepStatus::Completed),
            ("1.2", StepStatus::Pending),
        ]);

        let diffs = diff_versions(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].step_id, "1.1");
        assert_eq!(diffs[0].old_status, Some(StepStatus::Pending));
        assert_eq!(diffs[0].new_status, Some(StepStatus::Completed));
    }

    #[test]
    fn test_restore_old_version() {
        let dir = setup_git_repo();
        let plan = make_plan(1, vec![("1.1", StepStatus::Pending)]);
        save_version(dir.path(), &plan, "v1").unwrap();

        let mut plan2 = plan.clone();
        plan2.content.phases[0].steps[0].status = StepStatus::Completed;
        save_version(dir.path(), &plan2, "v2").unwrap();

        let versions = list_versions(dir.path(), 1).unwrap();
        let old_hash = &versions[1].hash; // v1 is second (older)
        let restored = get_version_content(dir.path(), 1, old_hash).unwrap();

        assert_eq!(restored.phases[0].steps[0].status, StepStatus::Pending);
    }

    #[test]
    fn test_no_git_repo_graceful_fallback() {
        let dir = TempDir::new().unwrap();
        let result = find_repo_root(dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            VersionError::NoRepo => {}
            other => panic!("Expected NoRepo, got: {other}"),
        }
    }

    #[test]
    fn test_save_empty_plan_content() {
        let dir = setup_git_repo();
        let plan = make_plan(1, vec![]);
        let hash = save_version(dir.path(), &plan, "empty plan").unwrap();
        assert!(!hash.is_empty());

        let versions = list_versions(dir.path(), 1).unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn test_diff_added_and_removed_steps() {
        let old = make_content(vec![
            ("1.1", StepStatus::Pending),
            ("1.2", StepStatus::Pending),
        ]);
        let new = make_content(vec![
            ("1.1", StepStatus::Pending),
            ("1.3", StepStatus::InProgress),
        ]);

        let diffs = diff_versions(&old, &new);
        assert_eq!(diffs.len(), 2);

        let removed = diffs.iter().find(|d| d.step_id == "1.2").unwrap();
        assert_eq!(removed.old_status, Some(StepStatus::Pending));
        assert_eq!(removed.new_status, None);

        let added = diffs.iter().find(|d| d.step_id == "1.3").unwrap();
        assert_eq!(added.old_status, None);
        assert_eq!(added.new_status, Some(StepStatus::InProgress));
    }

    #[test]
    fn test_get_version_content() {
        let dir = setup_git_repo();
        let plan = make_plan(1, vec![("1.1", StepStatus::Pending)]);
        let hash = save_version(dir.path(), &plan, "test").unwrap();

        let content = get_version_content(dir.path(), 1, &hash).unwrap();
        assert_eq!(content.phases.len(), 1);
        assert_eq!(content.phases[0].steps[0].id, "1.1");
    }
}
