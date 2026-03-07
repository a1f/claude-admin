use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("not a git repository: {0}")]
    NotARepo(String),
    #[error("git command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("worktree already exists: {0}")]
    WorktreeExists(String),
}

pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn create_worktree(
    repo_path: &Path,
    branch_name: &str,
    worktree_path: &Path,
) -> Result<(), GitError> {
    if !is_git_repo(repo_path) {
        return Err(GitError::NotARepo(repo_path.display().to_string()));
    }

    let repo = repo_path.to_string_lossy();
    let wt = worktree_path.to_string_lossy();

    // Try creating with a new branch first
    let output = Command::new("git")
        .args(["-C", &repo, "worktree", "add", "-b", branch_name, &wt])
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains("already exists") {
        return Err(GitError::CommandFailed(stderr.to_string()));
    }

    // Branch exists -- attach worktree to existing branch
    let output = Command::new("git")
        .args(["-C", &repo, "worktree", "add", &wt, branch_name])
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("already checked out") || stderr.contains("is a linked worktree") {
        return Err(GitError::WorktreeExists(wt.to_string()));
    }

    Err(GitError::CommandFailed(stderr.to_string()))
}

pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<(), GitError> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "worktree",
            "remove",
            &worktree_path.to_string_lossy(),
            "--force",
        ])
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    // Tolerate "not a working tree" -- already gone
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("is not a working tree") || stderr.contains("No such file or directory") {
        return Ok(());
    }

    Err(GitError::CommandFailed(stderr.to_string()))
}

pub fn list_worktrees(repo_path: &Path) -> Result<Vec<String>, GitError> {
    if !is_git_repo(repo_path) {
        return Err(GitError::NotARepo(repo_path.display().to_string()));
    }

    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "worktree",
            "list",
            "--porcelain",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::CommandFailed(stderr.to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(String::from)
        .collect();

    Ok(paths)
}

pub fn worktree_path_for_project(workspace_path: &str, project_name: &str) -> String {
    let sanitized = sanitize_name(project_name);
    format!("{}-worktrees/{}", workspace_path, sanitized)
}

pub fn sanitize_branch_name(name: &str) -> String {
    let sanitized = sanitize_name(name);
    format!("project/{}", sanitized)
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        // Collapse consecutive hyphens
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_temp_git_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().to_path_buf();

        Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .unwrap();

        // Worktrees require at least one commit
        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ])
            .output()
            .unwrap();

        (dir, repo_path)
    }

    #[test]
    fn test_sanitize_branch_name() {
        assert_eq!(sanitize_branch_name("Auth Feature"), "project/auth-feature");
    }

    #[test]
    fn test_sanitize_branch_name_special_chars() {
        assert_eq!(sanitize_branch_name("my project!@#"), "project/my-project");
    }

    #[test]
    fn test_sanitize_branch_name_already_clean() {
        assert_eq!(sanitize_branch_name("auth"), "project/auth");
    }

    #[test]
    fn test_worktree_path_for_project() {
        assert_eq!(
            worktree_path_for_project("/home/user/myapp", "auth"),
            "/home/user/myapp-worktrees/auth"
        );
    }

    #[test]
    fn test_worktree_path_for_project_with_spaces() {
        assert_eq!(
            worktree_path_for_project("/home/user/my app", "Auth Feature"),
            "/home/user/my app-worktrees/auth-feature"
        );
    }

    #[test]
    fn test_is_git_repo_nonexistent() {
        assert!(!is_git_repo(Path::new("/no/such/path/ever")));
    }

    #[test]
    fn test_is_git_repo_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_git_repo(dir.path()));
    }

    #[test]
    fn test_is_git_repo_real() {
        let (_dir, repo_path) = create_temp_git_repo();
        assert!(is_git_repo(&repo_path));
    }

    #[test]
    fn test_create_and_remove_worktree() {
        let (_dir, repo_path) = create_temp_git_repo();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().join("worktree-test");

        create_worktree(&repo_path, "project/test-branch", &wt_path).unwrap();
        assert!(wt_path.exists());

        remove_worktree(&repo_path, &wt_path).unwrap();
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_list_worktrees() {
        let (_dir, repo_path) = create_temp_git_repo();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().join("wt-list-test");

        let before = list_worktrees(&repo_path).unwrap();
        assert_eq!(before.len(), 1);

        create_worktree(&repo_path, "project/list-branch", &wt_path).unwrap();

        let after = list_worktrees(&repo_path).unwrap();
        assert_eq!(after.len(), 2);

        // Git may canonicalize paths, so compare canonical forms
        let wt_canonical = wt_path.canonicalize().unwrap();
        let found = after.iter().any(|p| {
            Path::new(p)
                .canonicalize()
                .map(|c| c == wt_canonical)
                .unwrap_or(false)
        });
        assert!(found, "worktree path not found in list: {:?}", after);

        remove_worktree(&repo_path, &wt_path).unwrap();
    }

    #[test]
    fn test_remove_worktree_nonexistent_is_ok() {
        let (_dir, repo_path) = create_temp_git_repo();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().join("does-not-exist");

        remove_worktree(&repo_path, &wt_path).unwrap();
    }
}
