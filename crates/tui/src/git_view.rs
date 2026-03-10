use crate::app::App;
use ca_lib::git_ops::{DiffFile, DiffLineKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn draw_git(frame: &mut Frame, app: &App, area: Rect) {
    if app.git_commits.is_empty() {
        let msg = Paragraph::new("No commit stack. Press 'g' from Sessions to load.")
            .block(Block::default().title(" Git Stack ").borders(Borders::ALL));
        frame.render_widget(msg, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    draw_commit_list(frame, app, chunks[0]);
    draw_commit_diff(frame, app, chunks[1]);
}

fn draw_commit_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .git_commits
        .iter()
        .map(|c| {
            let content = Line::from(vec![
                Span::styled(
                    format!("{} ", c.short_hash),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(&c.message),
            ]);
            ListItem::new(content)
        })
        .collect();

    let title = format!(" Commits ({}) ", app.git_commits.len());
    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.git_commit_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_commit_diff(frame: &mut Frame, app: &App, area: Rect) {
    if app.git_diff_files.is_empty() {
        let msg = Paragraph::new("Press Enter on a commit to view its diff.")
            .block(Block::default().title(" Diff ").borders(Borders::ALL));
        frame.render_widget(msg, area);
        return;
    }

    let file_count = app.git_diff_files.len();
    let file_idx = app.git_file_index.min(file_count.saturating_sub(1));

    let Some(file) = app.git_diff_files.get(file_idx) else {
        return;
    };

    let lines = build_diff_lines(file);
    let title = format!(
        " {} [{}/{}] (h/l: files) ",
        file.new_path,
        file_idx + 1,
        file_count
    );

    let paragraph = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .scroll((app.git_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn build_diff_lines(file: &DiffFile) -> Vec<Line<'static>> {
    if file.is_binary {
        return vec![Line::styled(
            "Binary file differs",
            Style::default().fg(Color::DarkGray),
        )];
    }

    let mut lines = Vec::new();
    for hunk in &file.hunks {
        lines.push(Line::styled(
            hunk.header.clone(),
            Style::default().fg(Color::Cyan),
        ));

        for dl in &hunk.lines {
            let old_no = dl
                .old_lineno
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            let new_no = dl
                .new_lineno
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());

            let (prefix, style) = match dl.kind {
                DiffLineKind::Added => ("+", Style::default().fg(Color::Green)),
                DiffLineKind::Removed => ("-", Style::default().fg(Color::Red)),
                DiffLineKind::Context => (" ", Style::default()),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{old_no} {new_no} "),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{prefix}{}", dl.content), style),
            ]);
            lines.push(line);
        }
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "No changes",
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppAction, ViewMode};
    use ca_lib::git_ops::{CommitInfo, DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_commit(hash: &str, msg: &str) -> CommitInfo {
        CommitInfo {
            hash: hash.to_string(),
            short_hash: hash[..7.min(hash.len())].to_string(),
            author: "Test".to_string(),
            date: "2025-01-01".to_string(),
            message: msg.to_string(),
        }
    }

    fn sample_git_diff_files() -> Vec<DiffFile> {
        vec![DiffFile {
            old_path: "src/main.rs".to_string(),
            new_path: "src/main.rs".to_string(),
            is_binary: false,
            is_rename: false,
            hunks: vec![DiffHunk {
                header: "@@ -1,4 +1,5 @@".to_string(),
                old_start: 1,
                old_count: 4,
                new_start: 1,
                new_count: 5,
                lines: vec![
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
                ],
            }],
        }]
    }

    #[test]
    fn test_draw_git_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new();

        terminal
            .draw(|frame| {
                draw_git(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_draw_git_with_commits() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.view_mode = ViewMode::Git;
        app.git_commits = vec![
            make_commit("abc1234", "Initial commit"),
            make_commit("def5678", "Add feature"),
        ];
        app.git_diff_files = sample_git_diff_files();

        terminal
            .draw(|frame| {
                draw_git(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_navigate_commits() {
        let mut app = App::new();
        app.view_mode = ViewMode::Git;
        app.git_commits = vec![
            make_commit("aaa1111", "First"),
            make_commit("bbb2222", "Second"),
        ];
        assert_eq!(app.git_commit_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.git_commit_index, 1);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.git_commit_index, 0);
    }

    #[test]
    fn test_enter_loads_commit_diff() {
        let mut app = App::new();
        app.view_mode = ViewMode::Git;
        app.git_commits = vec![
            make_commit("aaa1111", "First"),
            make_commit("bbb2222", "Second"),
        ];

        let action = app.handle_key(key(KeyCode::Enter));
        match action {
            AppAction::LoadCommitDiff { commit, .. } => {
                assert_eq!(commit, "aaa1111");
            }
            other => panic!("Expected LoadCommitDiff, got {:?}", other),
        }
    }

    #[test]
    fn test_back_returns_to_sessions() {
        let mut app = App::new();
        app.view_mode = ViewMode::Git;
        app.git_commits = vec![make_commit("aaa1111", "First")];

        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Sessions);
    }

    #[test]
    fn test_build_diff_lines_binary() {
        let file = DiffFile {
            old_path: "img.png".to_string(),
            new_path: "img.png".to_string(),
            is_binary: true,
            is_rename: false,
            hunks: vec![],
        };
        let lines = build_diff_lines(&file);
        assert_eq!(lines.len(), 1);
    }
}
