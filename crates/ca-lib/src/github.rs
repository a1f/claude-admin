use std::process::Command;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GithubError {
    #[error("gh CLI not found. Install: https://cli.github.com")]
    GhNotFound,
    #[error("gh command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a git repository")]
    NotGitRepo,
    #[error("git error: {0}")]
    Git(String),
}

pub struct PrCreateOptions {
    pub title: Option<String>,
    pub body: Option<String>,
    pub base: Option<String>,
    pub draft: bool,
    pub repo_path: String,
}

pub struct PrCreateResult {
    pub url: String,
    pub number: u32,
}

/// Check if `gh` CLI is available.
pub fn is_gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Auto-generate PR title from the first commit message on the branch.
pub fn auto_title(repo_path: &str, base: Option<&str>) -> Result<String, GithubError> {
    let base_branch = base.unwrap_or("main");
    let range = format!("{base_branch}..HEAD");
    let output = Command::new("git")
        .args(["log", "--format=%s", "--reverse", &range])
        .current_dir(repo_path)
        .output()
        .map_err(GithubError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GithubError::Git(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().unwrap_or("Update").to_string())
}

/// Auto-generate PR body from commit messages on the branch.
pub fn auto_body(repo_path: &str, base: Option<&str>) -> Result<String, GithubError> {
    let base_branch = base.unwrap_or("main");
    let range = format!("{base_branch}..HEAD");
    let output = Command::new("git")
        .args(["log", "--format=- %s", "--reverse", &range])
        .current_dir(repo_path)
        .output()
        .map_err(GithubError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GithubError::Git(stderr));
    }

    let commits = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(format!("## Changes\n\n{commits}\n"))
}

/// Build the `gh pr create` argument list from options.
pub fn build_pr_args(opts: &PrCreateOptions) -> Vec<String> {
    let mut args = vec!["pr".to_string(), "create".to_string()];

    if let Some(title) = &opts.title {
        args.push("--title".to_string());
        args.push(title.clone());
    }
    if let Some(body) = &opts.body {
        args.push("--body".to_string());
        args.push(body.clone());
    }
    if let Some(base) = &opts.base {
        args.push("--base".to_string());
        args.push(base.clone());
    }
    if opts.draft {
        args.push("--draft".to_string());
    }

    args
}

/// Create a GitHub PR using `gh pr create`.
pub fn create_pr(opts: &PrCreateOptions) -> Result<PrCreateResult, GithubError> {
    if !is_gh_available() {
        return Err(GithubError::GhNotFound);
    }

    let args = build_pr_args(opts);
    let output = Command::new("gh")
        .args(&args)
        .current_dir(&opts.repo_path)
        .output()
        .map_err(GithubError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GithubError::CommandFailed(stderr));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let number = url
        .rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    Ok(PrCreateResult { url, number })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_opts(
        title: Option<&str>,
        body: Option<&str>,
        base: Option<&str>,
        draft: bool,
    ) -> PrCreateOptions {
        PrCreateOptions {
            title: title.map(String::from),
            body: body.map(String::from),
            base: base.map(String::from),
            draft,
            repo_path: ".".to_string(),
        }
    }

    #[test]
    fn test_build_pr_args_basic() {
        let opts = make_opts(None, None, None, false);
        let args = build_pr_args(&opts);
        assert_eq!(args, vec!["pr", "create"]);
    }

    #[test]
    fn test_build_pr_args_with_title() {
        let opts = make_opts(Some("My PR"), None, None, false);
        let args = build_pr_args(&opts);
        assert_eq!(args, vec!["pr", "create", "--title", "My PR"]);
    }

    #[test]
    fn test_build_pr_args_with_body() {
        let opts = make_opts(None, Some("Description"), None, false);
        let args = build_pr_args(&opts);
        assert_eq!(args, vec!["pr", "create", "--body", "Description"]);
    }

    #[test]
    fn test_build_pr_args_with_base() {
        let opts = make_opts(None, None, Some("develop"), false);
        let args = build_pr_args(&opts);
        assert_eq!(args, vec!["pr", "create", "--base", "develop"]);
    }

    #[test]
    fn test_build_pr_args_draft() {
        let opts = make_opts(None, None, None, true);
        let args = build_pr_args(&opts);
        assert_eq!(args, vec!["pr", "create", "--draft"]);
    }

    #[test]
    fn test_build_pr_args_all_options() {
        let opts = make_opts(Some("title"), Some("body"), Some("main"), true);
        let args = build_pr_args(&opts);
        assert_eq!(
            args,
            vec![
                "pr", "create", "--title", "title", "--body", "body", "--base", "main", "--draft"
            ]
        );
    }

    fn create_temp_git_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().to_path_buf();

        Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "config",
                "user.email",
                "test@test.com",
            ])
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "config",
                "user.name",
                "Test",
            ])
            .output()
            .unwrap();

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

    fn add_commit(repo_path: &std::path::Path, filename: &str, message: &str) {
        std::fs::write(repo_path.join(filename), "content").unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", filename])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", message])
            .output()
            .unwrap();
    }

    #[test]
    fn test_auto_title_format() {
        let (_dir, repo_path) = create_temp_git_repo();

        // Create a branch off main so we have commits to compare
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "branch", "-M", "main"])
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "checkout",
                "-b",
                "feature",
            ])
            .output()
            .unwrap();

        add_commit(&repo_path, "a.txt", "Add feature A");
        add_commit(&repo_path, "b.txt", "Add feature B");

        let title = auto_title(repo_path.to_str().unwrap(), Some("main")).unwrap();
        assert_eq!(title, "Add feature A");
    }

    #[test]
    fn test_auto_body_format() {
        let (_dir, repo_path) = create_temp_git_repo();

        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "branch", "-M", "main"])
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "checkout",
                "-b",
                "feature",
            ])
            .output()
            .unwrap();

        add_commit(&repo_path, "a.txt", "Add feature A");
        add_commit(&repo_path, "b.txt", "Fix bug B");

        let body = auto_body(repo_path.to_str().unwrap(), Some("main")).unwrap();
        assert!(body.starts_with("## Changes\n\n"));
        assert!(body.contains("- Add feature A"));
        assert!(body.contains("- Fix bug B"));
    }

    #[test]
    fn test_github_error_display() {
        let err = GithubError::GhNotFound;
        assert_eq!(
            err.to_string(),
            "gh CLI not found. Install: https://cli.github.com"
        );

        let err = GithubError::CommandFailed("exit 1".to_string());
        assert_eq!(err.to_string(), "gh command failed: exit 1");

        let err = GithubError::NotGitRepo;
        assert_eq!(err.to_string(), "not a git repository");

        let err = GithubError::Git("bad ref".to_string());
        assert_eq!(err.to_string(), "git error: bad ref");
    }
}
