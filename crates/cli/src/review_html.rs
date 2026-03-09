use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use ca_lib::git_ops::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
use ca_lib::review::{Review, ReviewComment};

/// Generate a standalone HTML page showing a code review with diff and inline comments.
pub fn generate_review_html(
    review: &Review,
    diff_files: &[DiffFile],
    comments: &[ReviewComment],
) -> String {
    let mut html = String::new();
    push_head(&mut html, review);
    push_review_header(&mut html, review);

    let comment_map = build_comment_map(comments);
    for file in diff_files {
        let display_path = display_path_for_file(file);
        render_file_html(&mut html, file, comment_map.get(display_path));
    }

    html.push_str("</body></html>");
    html
}

/// Resolve the repo path for a review by looking up the project and workspace.
pub fn resolve_repo_path(db: &ca_lib::db::Database, review: &Review) -> Option<String> {
    let pid = review.project_id?;
    let project = db.get_project(pid).ok()??;
    if let Some(wt) = project.worktree_path {
        return Some(wt);
    }
    let ws = db.get_workspace(project.workspace_id).ok()??;
    Some(ws.path)
}

/// Fetch parsed diff files for a review's commit range.
pub fn fetch_diff_files(repo_path: &str, review: &Review) -> Vec<DiffFile> {
    if review.base_commit.is_empty() || review.head_commit.is_empty() {
        return Vec::new();
    }
    ca_lib::git_ops::git_diff(
        Path::new(repo_path),
        &review.base_commit,
        &review.head_commit,
    )
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn push_head(html: &mut String, review: &Review) {
    html.push_str("<!DOCTYPE html>\n<html><head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str(&format!(
        "<title>Review #{} - {}</title>\n",
        review.id,
        escape_html(&review.branch)
    ));
    html.push_str("<style>\n");
    html.push_str(REVIEW_CSS);
    html.push_str("</style>\n");
    html.push_str("</head><body>\n");
}

fn push_review_header(html: &mut String, review: &Review) {
    let base_short = &review.base_commit[..7.min(review.base_commit.len())];
    let head_short = &review.head_commit[..7.min(review.head_commit.len())];

    html.push_str("<div class=\"review-header\">\n");
    html.push_str(&format!("<h1>Code Review #{}</h1>\n", review.id));
    html.push_str(&format!(
        "<p>Branch: <code>{}</code></p>\n",
        escape_html(&review.branch)
    ));
    html.push_str(&format!(
        "<p>Commits: <code>{}..{}</code></p>\n",
        escape_html(base_short),
        escape_html(head_short)
    ));
    html.push_str(&format!(
        "<p>Status: <span class=\"status-{}\">{}</span> | Round: {}</p>\n",
        review.status.as_str(),
        review.status,
        review.round
    ));
    html.push_str("</div>\n");
}

type CommentMap<'a> = HashMap<&'a str, BTreeMap<u32, Vec<&'a ReviewComment>>>;

fn build_comment_map(comments: &[ReviewComment]) -> CommentMap<'_> {
    let mut map: CommentMap<'_> = HashMap::new();
    for c in comments {
        map.entry(c.file_path.as_str())
            .or_default()
            .entry(c.line_number)
            .or_default()
            .push(c);
    }
    map
}

fn display_path_for_file(file: &DiffFile) -> &str {
    if file.new_path == "/dev/null" {
        &file.old_path
    } else {
        &file.new_path
    }
}

fn render_file_html(
    html: &mut String,
    file: &DiffFile,
    comments: Option<&BTreeMap<u32, Vec<&ReviewComment>>>,
) {
    let path = display_path_for_file(file);
    html.push_str(&format!(
        "<div class=\"file\">\n<div class=\"file-header\">{}</div>\n",
        escape_html(path)
    ));

    if file.is_binary {
        html.push_str("<div class=\"binary\">Binary file</div>\n");
    } else {
        html.push_str("<table class=\"diff\">\n");
        for hunk in &file.hunks {
            render_hunk_html(html, hunk, comments);
        }
        html.push_str("</table>\n");
    }

    html.push_str("</div>\n");
}

fn render_hunk_html(
    html: &mut String,
    hunk: &DiffHunk,
    comments: Option<&BTreeMap<u32, Vec<&ReviewComment>>>,
) {
    html.push_str(&format!(
        "<tr class=\"hunk-header\"><td colspan=\"3\">{}</td></tr>\n",
        escape_html(&hunk.header)
    ));

    for line in &hunk.lines {
        render_diff_line(html, line);
        render_line_comments(html, line, comments);
    }
}

fn render_diff_line(html: &mut String, line: &DiffLine) {
    let (class, prefix) = match line.kind {
        DiffLineKind::Added => ("added", "+"),
        DiffLineKind::Removed => ("removed", "-"),
        DiffLineKind::Context => ("context", " "),
    };
    let old_ln = line.old_lineno.map(|n| n.to_string()).unwrap_or_default();
    let new_ln = line.new_lineno.map(|n| n.to_string()).unwrap_or_default();

    html.push_str(&format!(
        "<tr class=\"{class}\"><td class=\"ln\">{old_ln}</td>\
         <td class=\"ln\">{new_ln}</td>\
         <td><code>{prefix}{}</code></td></tr>\n",
        escape_html(&line.content)
    ));
}

fn render_line_comments(
    html: &mut String,
    line: &DiffLine,
    comments: Option<&BTreeMap<u32, Vec<&ReviewComment>>>,
) {
    let new_lineno = match line.new_lineno {
        Some(n) => n,
        None => return,
    };
    let file_comments = match comments {
        Some(c) => c,
        None => return,
    };
    let line_comments = match file_comments.get(&new_lineno) {
        Some(c) => c,
        None => return,
    };
    for comment in line_comments {
        html.push_str(&format!(
            "<tr class=\"comment\"><td colspan=\"3\">\
             <div class=\"comment-body\">{}</div>\
             </td></tr>\n",
            escape_html(&comment.body)
        ));
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const REVIEW_CSS: &str = r#"
body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; background: #1e1e1e; color: #d4d4d4; }
h1 { color: #569cd6; }
code { font-family: 'SF Mono', 'Fira Code', monospace; }
.review-header { border-bottom: 1px solid #333; padding-bottom: 16px; margin-bottom: 24px; }
.file { margin-bottom: 24px; border: 1px solid #333; border-radius: 6px; overflow: hidden; }
.file-header { background: #252526; padding: 8px 16px; font-weight: bold; border-bottom: 1px solid #333; }
.diff { width: 100%; border-collapse: collapse; font-size: 13px; }
.diff td { padding: 1px 8px; white-space: pre-wrap; word-break: break-all; }
.diff .ln { color: #858585; text-align: right; width: 40px; user-select: none; }
.added { background: rgba(35, 134, 54, 0.2); }
.added code { color: #4ec94e; }
.removed { background: rgba(218, 54, 51, 0.2); }
.removed code { color: #f14c4c; }
.context code { color: #d4d4d4; }
.hunk-header td { background: #1e3a5f; color: #569cd6; font-style: italic; padding: 4px 8px; }
.comment td { background: #2d2d0d; }
.comment-body { background: #3d3d1d; border-left: 3px solid #b5cea8; padding: 8px 12px; margin: 4px 0; border-radius: 4px; color: #b5cea8; }
.binary { padding: 16px; color: #858585; font-style: italic; }
.status-pending { color: #dcdcaa; }
.status-in_progress { color: #569cd6; }
.status-approved { color: #4ec94e; }
.status-changes_requested { color: #f14c4c; }
"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ca_lib::review::ReviewStatus;

    fn make_review() -> Review {
        Review {
            id: 42,
            session_id: Some("sess-1".to_string()),
            project_id: None,
            branch: "feature/login".to_string(),
            base_commit: "abc1234567890".to_string(),
            head_commit: "def4567890abc".to_string(),
            status: ReviewStatus::InProgress,
            round: 2,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_comment(file_path: &str, line: u32, body: &str) -> ReviewComment {
        ReviewComment {
            id: 0,
            review_id: 42,
            commit_sha: "abc1234".to_string(),
            file_path: file_path.to_string(),
            line_number: line,
            body: body.to_string(),
            resolved: false,
            created_at: 0,
        }
    }

    fn make_diff_file(path: &str, hunks: Vec<DiffHunk>) -> DiffFile {
        DiffFile {
            old_path: path.to_string(),
            new_path: path.to_string(),
            is_binary: false,
            is_rename: false,
            hunks,
        }
    }

    fn make_hunk(lines: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            lines,
        }
    }

    #[test]
    fn test_generate_html_basic_structure() {
        let review = make_review();
        let html = generate_review_html(&review, &[], &[]);

        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<title>Review #42 - feature/login</title>"));
        assert!(html.contains("<h1>Code Review #42</h1>"));
        assert!(html.contains("feature/login"));
        assert!(html.contains("abc1234..def4567"));
        assert!(html.contains("in_progress"));
        assert!(html.contains("Round: 2"));
    }

    #[test]
    fn test_generate_html_with_comments() {
        let review = make_review();
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Context,
                content: "fn main() {".to_string(),
                old_lineno: Some(1),
                new_lineno: Some(1),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "    println!(\"hello\");".to_string(),
                old_lineno: None,
                new_lineno: Some(2),
            },
        ];
        let files = vec![make_diff_file("src/main.rs", vec![make_hunk(lines)])];
        let comments = vec![make_comment("src/main.rs", 2, "Use eprintln instead")];

        let html = generate_review_html(&review, &files, &comments);

        assert!(html.contains("comment-body"));
        assert!(html.contains("Use eprintln instead"));
        // Comment should appear after the added line
        let added_pos = html.find("println").unwrap();
        let comment_pos = html.find("Use eprintln instead").unwrap();
        assert!(comment_pos > added_pos);
    }

    #[test]
    fn test_generate_html_empty_diff() {
        let review = make_review();
        let html = generate_review_html(&review, &[], &[]);

        assert!(html.contains("Code Review #42"));
        assert!(!html.contains("class=\"file\""));
        assert!(!html.contains("<table"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("he said \"hi\""), "he said &quot;hi&quot;");
        assert_eq!(escape_html("plain text"), "plain text");
    }

    #[test]
    fn test_generate_html_binary_file() {
        let review = make_review();
        let files = vec![DiffFile {
            old_path: "image.png".to_string(),
            new_path: "image.png".to_string(),
            is_binary: true,
            is_rename: false,
            hunks: vec![],
        }];

        let html = generate_review_html(&review, &files, &[]);

        assert!(html.contains("Binary file"));
        assert!(html.contains("image.png"));
        assert!(!html.contains("<table"));
    }

    #[test]
    fn test_generate_html_diff_line_classes() {
        let review = make_review();
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Removed,
                content: "old line".to_string(),
                old_lineno: Some(1),
                new_lineno: None,
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "new line".to_string(),
                old_lineno: None,
                new_lineno: Some(1),
            },
            DiffLine {
                kind: DiffLineKind::Context,
                content: "unchanged".to_string(),
                old_lineno: Some(2),
                new_lineno: Some(2),
            },
        ];
        let files = vec![make_diff_file("file.rs", vec![make_hunk(lines)])];

        let html = generate_review_html(&review, &files, &[]);

        assert!(html.contains("class=\"removed\""));
        assert!(html.contains("class=\"added\""));
        assert!(html.contains("class=\"context\""));
        assert!(html.contains("-old line"));
        assert!(html.contains("+new line"));
    }

    #[test]
    fn test_build_comment_map_groups_correctly() {
        let comments = vec![
            make_comment("a.rs", 10, "First"),
            make_comment("a.rs", 10, "Second"),
            make_comment("b.rs", 5, "Other file"),
        ];

        let map = build_comment_map(&comments);

        assert_eq!(map.len(), 2);
        assert_eq!(map["a.rs"][&10].len(), 2);
        assert_eq!(map["b.rs"][&5].len(), 1);
    }
}
