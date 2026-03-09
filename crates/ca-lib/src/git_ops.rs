use std::path::Path;
use std::process::Command;

use crate::git::{GitError, is_git_repo};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub old_path: String,
    pub new_path: String,
    pub is_binary: bool,
    pub is_rename: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Diff parsing
// ---------------------------------------------------------------------------

/// Parse unified diff output (e.g. from `git diff --no-color`) into structured data.
pub fn parse_diff(diff_text: &str) -> Vec<DiffFile> {
    let lines: Vec<&str> = diff_text.lines().collect();
    let mut files: Vec<DiffFile> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let (file, next) = parse_file_diff(&lines, i);
            files.push(file);
            i = next;
        } else {
            i += 1;
        }
    }

    files
}

/// Parse one file's diff starting at the `diff --git` line.
/// Returns the parsed `DiffFile` and the index of the next unprocessed line.
fn parse_file_diff(lines: &[&str], start: usize) -> (DiffFile, usize) {
    let mut old_path = String::new();
    let mut new_path = String::new();
    let mut is_binary = false;
    let mut is_rename = false;
    let mut hunks: Vec<DiffHunk> = Vec::new();

    extract_paths_from_diff_header(lines[start], &mut old_path, &mut new_path);

    let mut i = start + 1;

    // Process metadata lines before hunks
    while i < lines.len() && !lines[i].starts_with("diff --git ") {
        let line = lines[i];

        if line.starts_with("--- ") {
            old_path = strip_path_prefix(line, "--- ");
        } else if line.starts_with("+++ ") {
            new_path = strip_path_prefix(line, "+++ ");
        } else if line.starts_with("rename from ") {
            is_rename = true;
            old_path = line.strip_prefix("rename from ").unwrap_or("").to_string();
        } else if line.starts_with("rename to ") {
            is_rename = true;
            new_path = line.strip_prefix("rename to ").unwrap_or("").to_string();
        } else if line.starts_with("Binary files ") {
            is_binary = true;
            i += 1;
            break;
        } else if line.starts_with("@@ ") {
            let (hunk, next) = parse_hunk(lines, i);
            hunks.push(hunk);
            i = next;
            continue;
        }

        i += 1;
    }

    // Continue collecting hunks after metadata
    while i < lines.len() && !lines[i].starts_with("diff --git ") {
        if lines[i].starts_with("@@ ") {
            let (hunk, next) = parse_hunk(lines, i);
            hunks.push(hunk);
            i = next;
        } else {
            i += 1;
        }
    }

    let file = DiffFile {
        old_path,
        new_path,
        is_binary,
        is_rename,
        hunks,
    };
    (file, i)
}

/// Extract a/path and b/path from the `diff --git a/foo b/bar` header.
fn extract_paths_from_diff_header(header: &str, old_path: &mut String, new_path: &mut String) {
    let rest = header.strip_prefix("diff --git ").unwrap_or("");
    // Format: a/<old> b/<new> — find the split point by looking for " b/"
    if let Some(pos) = rest.find(" b/") {
        *old_path = rest[..pos]
            .strip_prefix("a/")
            .unwrap_or(&rest[..pos])
            .to_string();
        *new_path = rest[pos + 1..]
            .strip_prefix("b/")
            .unwrap_or(&rest[pos + 1..])
            .to_string();
    }
}

/// Strip `--- a/path` or `+++ b/path` prefixes, handling `/dev/null`.
fn strip_path_prefix(line: &str, prefix: &str) -> String {
    let raw = line.strip_prefix(prefix).unwrap_or("");
    if raw == "/dev/null" {
        return raw.to_string();
    }
    raw.strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw)
        .to_string()
}

/// Parse a single hunk starting at the `@@` header line.
/// Returns the parsed `DiffHunk` and the index of the next unprocessed line.
fn parse_hunk(lines: &[&str], start: usize) -> (DiffHunk, usize) {
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header);

    let mut diff_lines: Vec<DiffLine> = Vec::new();
    let mut old_lineno = old_start;
    let mut new_lineno = new_start;

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("diff --git ") || line.starts_with("@@ ") {
            break;
        }

        if let Some(content) = line.strip_prefix('+') {
            diff_lines.push(DiffLine {
                kind: DiffLineKind::Added,
                content: content.to_string(),
                old_lineno: None,
                new_lineno: Some(new_lineno),
            });
            new_lineno += 1;
        } else if let Some(content) = line.strip_prefix('-') {
            diff_lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                content: content.to_string(),
                old_lineno: Some(old_lineno),
                new_lineno: None,
            });
            old_lineno += 1;
        } else if line.starts_with(' ') || line.is_empty() {
            // Context line (starts with space) or empty line within hunk
            let content = if line.is_empty() {
                String::new()
            } else {
                line[1..].to_string()
            };
            diff_lines.push(DiffLine {
                kind: DiffLineKind::Context,
                content,
                old_lineno: Some(old_lineno),
                new_lineno: Some(new_lineno),
            });
            old_lineno += 1;
            new_lineno += 1;
        } else if line == "\\ No newline at end of file" {
            // Skip this git marker
        } else {
            break;
        }

        i += 1;
    }

    let hunk = DiffHunk {
        header: header.to_string(),
        old_start,
        old_count,
        new_start,
        new_count,
        lines: diff_lines,
    };
    (hunk, i)
}

/// Parse `@@ -old_start,old_count +new_start,new_count @@` into numeric components.
fn parse_hunk_header(header: &str) -> (u32, u32, u32, u32) {
    // Strip the @@ markers to get "-old_start,old_count +new_start,new_count"
    let inner = header
        .strip_prefix("@@ ")
        .and_then(|s| s.split(" @@").next())
        .unwrap_or("");

    let parts: Vec<&str> = inner.split_whitespace().collect();
    let (old_start, old_count) = parse_range(parts.first().unwrap_or(&""));
    let (new_start, new_count) = parse_range(parts.get(1).unwrap_or(&""));

    (old_start, old_count, new_start, new_count)
}

/// Parse a range like `-3,7` or `+1,4` or `-3` (count defaults to 1).
fn parse_range(s: &str) -> (u32, u32) {
    let s = s.trim_start_matches(['-', '+']);
    match s.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

// ---------------------------------------------------------------------------
// Git command wrappers
// ---------------------------------------------------------------------------

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String, GitError> {
    if !is_git_repo(repo_path) {
        return Err(GitError::NotARepo(repo_path.display().to_string()));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path.as_os_str())
        .args(args)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::CommandFailed(stderr.to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run `git diff --no-color <base>..<head>` and parse the output.
pub fn git_diff(repo_path: &Path, base: &str, head: &str) -> Result<Vec<DiffFile>, GitError> {
    let range = format!("{base}..{head}");
    let stdout = run_git(repo_path, &["diff", "--no-color", &range])?;
    Ok(parse_diff(&stdout))
}

/// Run `git diff --cached --no-color` and parse staged changes.
pub fn git_diff_staged(repo_path: &Path) -> Result<Vec<DiffFile>, GitError> {
    let stdout = run_git(repo_path, &["diff", "--cached", "--no-color"])?;
    Ok(parse_diff(&stdout))
}

/// Run `git log` and return the last `count` commits.
pub fn git_log(repo_path: &Path, count: usize) -> Result<Vec<CommitInfo>, GitError> {
    let n_arg = format!("-n{count}");
    let stdout = run_git(repo_path, &["log", &n_arg, "--format=%H%n%h%n%an%n%ai%n%s"])?;
    Ok(parse_log_output(&stdout))
}

fn parse_log_output(stdout: &str) -> Vec<CommitInfo> {
    let lines: Vec<&str> = stdout.lines().collect();
    let mut commits = Vec::new();

    // Each commit is 5 consecutive lines: hash, short_hash, author, date, message
    for chunk in lines.chunks_exact(5) {
        commits.push(CommitInfo {
            hash: chunk[0].to_string(),
            short_hash: chunk[1].to_string(),
            author: chunk[2].to_string(),
            date: chunk[3].to_string(),
            message: chunk[4].to_string(),
        });
    }

    commits
}

/// Run `git show` for a single commit and parse its diff output.
pub fn git_show(repo_path: &Path, commit: &str) -> Result<Vec<DiffFile>, GitError> {
    let stdout = run_git(repo_path, &["show", "--no-color", "--format=", commit])?;
    Ok(parse_diff(&stdout))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn test_parse_diff_single_file() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index 1234567..abcdefg 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,4 +1,5 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"hello world\");
+    println!(\"goodbye\");
 }
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path, "src/main.rs");
        assert_eq!(files[0].new_path, "src/main.rs");
        assert!(!files[0].is_binary);
        assert!(!files[0].is_rename);
        assert_eq!(files[0].hunks.len(), 1);

        let hunk = &files[0].hunks[0];
        let added: Vec<_> = hunk
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Added)
            .collect();
        let removed: Vec<_> = hunk
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Removed)
            .collect();
        assert_eq!(added.len(), 2);
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn test_parse_diff_multiple_files() {
        let diff = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,3 @@
 line1
+line2
 line3
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1,2 @@
 alpha
+beta
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].new_path, "a.txt");
        assert_eq!(files[1].new_path, "b.txt");
    }

    #[test]
    fn test_parse_diff_added_lines_only() {
        let diff = "\
diff --git a/new_file.rs b/new_file.rs
new file mode 100644
--- /dev/null
+++ b/new_file.rs
@@ -0,0 +1,3 @@
+fn new() {}
+fn also_new() {}
+fn third() {}
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path, "/dev/null");
        assert_eq!(files[0].new_path, "new_file.rs");

        let hunk = &files[0].hunks[0];
        assert!(hunk.lines.iter().all(|l| l.kind == DiffLineKind::Added));
        assert_eq!(hunk.lines.len(), 3);
    }

    #[test]
    fn test_parse_diff_removed_lines_only() {
        let diff = "\
diff --git a/old.rs b/old.rs
deleted file mode 100644
--- a/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn gone() {}
-fn also_gone() {}
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "/dev/null");

        let hunk = &files[0].hunks[0];
        assert!(hunk.lines.iter().all(|l| l.kind == DiffLineKind::Removed));
        assert_eq!(hunk.lines.len(), 2);
    }

    #[test]
    fn test_parse_diff_context_lines() {
        let diff = "\
diff --git a/ctx.rs b/ctx.rs
--- a/ctx.rs
+++ b/ctx.rs
@@ -1,5 +1,5 @@
 line1
 line2
-old
+new
 line4
 line5
";
        let files = parse_diff(diff);
        let hunk = &files[0].hunks[0];
        let ctx: Vec<_> = hunk
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Context)
            .collect();
        assert_eq!(ctx.len(), 4);
        assert_eq!(ctx[0].content, "line1");
        assert_eq!(ctx[1].content, "line2");
    }

    #[test]
    fn test_parse_diff_binary_file() {
        let diff = "\
diff --git a/image.png b/image.png
Binary files a/image.png and b/image.png differ
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert!(files[0].is_binary);
        assert!(files[0].hunks.is_empty());
    }

    #[test]
    fn test_parse_diff_rename() {
        let diff = "\
diff --git a/old_name.rs b/new_name.rs
similarity index 100%
rename from old_name.rs
rename to new_name.rs
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert!(files[0].is_rename);
        assert_eq!(files[0].old_path, "old_name.rs");
        assert_eq!(files[0].new_path, "new_name.rs");
    }

    #[test]
    fn test_parse_diff_empty() {
        let files = parse_diff("");
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_hunk_header() {
        let (old_start, old_count, new_start, new_count) =
            parse_hunk_header("@@ -10,7 +20,9 @@ fn foo()");
        assert_eq!(old_start, 10);
        assert_eq!(old_count, 7);
        assert_eq!(new_start, 20);
        assert_eq!(new_count, 9);
    }

    #[test]
    fn test_parse_hunk_header_no_count() {
        let (old_start, old_count, new_start, new_count) = parse_hunk_header("@@ -1 +1 @@");
        assert_eq!(old_start, 1);
        assert_eq!(old_count, 1);
        assert_eq!(new_start, 1);
        assert_eq!(new_count, 1);
    }

    #[test]
    fn test_parse_diff_line_numbers() {
        let diff = "\
diff --git a/num.rs b/num.rs
--- a/num.rs
+++ b/num.rs
@@ -5,4 +5,5 @@
 context
-removed
+added1
+added2
 context2
";
        let files = parse_diff(diff);
        let hunk = &files[0].hunks[0];

        // First line: context at old=5, new=5
        assert_eq!(hunk.lines[0].kind, DiffLineKind::Context);
        assert_eq!(hunk.lines[0].old_lineno, Some(5));
        assert_eq!(hunk.lines[0].new_lineno, Some(5));

        // Second line: removed at old=6
        assert_eq!(hunk.lines[1].kind, DiffLineKind::Removed);
        assert_eq!(hunk.lines[1].old_lineno, Some(6));
        assert_eq!(hunk.lines[1].new_lineno, None);

        // Third line: added at new=6
        assert_eq!(hunk.lines[2].kind, DiffLineKind::Added);
        assert_eq!(hunk.lines[2].old_lineno, None);
        assert_eq!(hunk.lines[2].new_lineno, Some(6));

        // Fourth line: added at new=7
        assert_eq!(hunk.lines[3].kind, DiffLineKind::Added);
        assert_eq!(hunk.lines[3].new_lineno, Some(7));

        // Fifth line: context at old=7, new=8
        assert_eq!(hunk.lines[4].kind, DiffLineKind::Context);
        assert_eq!(hunk.lines[4].old_lineno, Some(7));
        assert_eq!(hunk.lines[4].new_lineno, Some(8));
    }

    #[test]
    fn test_git_log_in_repo() {
        let (_dir, repo_path) = create_temp_git_repo();

        // Add a second commit with a file
        std::fs::write(repo_path.join("hello.txt"), "hello").unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "commit",
                "-m",
                "add hello",
            ])
            .output()
            .unwrap();

        let commits = git_log(&repo_path, 5).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].message, "add hello");
        assert_eq!(commits[1].message, "initial");
        assert!(!commits[0].hash.is_empty());
        assert!(!commits[0].short_hash.is_empty());
        assert!(!commits[0].author.is_empty());
        assert!(!commits[0].date.is_empty());
    }

    #[test]
    fn test_git_diff_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = git_diff(dir.path(), "HEAD~1", "HEAD");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not a git repository"));
    }

    #[test]
    fn test_git_show_in_repo() {
        let (_dir, repo_path) = create_temp_git_repo();

        std::fs::write(repo_path.join("file.txt"), "content\n").unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "commit",
                "-m",
                "add file",
            ])
            .output()
            .unwrap();

        let files = git_show(&repo_path, "HEAD").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "file.txt");
        assert!(!files[0].hunks.is_empty());
    }

    #[test]
    fn test_git_diff_staged_in_repo() {
        let (_dir, repo_path) = create_temp_git_repo();

        std::fs::write(repo_path.join("staged.txt"), "staged content\n").unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "staged.txt"])
            .output()
            .unwrap();

        let files = git_diff_staged(&repo_path).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "staged.txt");
    }

    #[test]
    fn test_git_diff_between_commits() {
        let (_dir, repo_path) = create_temp_git_repo();

        // Get initial commit hash
        let output = Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap();
        let base = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Create a second commit
        std::fs::write(repo_path.join("diff_test.txt"), "new content\n").unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", "second"])
            .output()
            .unwrap();

        let files = git_diff(&repo_path, &base, "HEAD").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "diff_test.txt");
    }
}
