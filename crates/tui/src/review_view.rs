use crate::app::App;
use ca_lib::git_ops::{DiffFile, DiffLineKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn draw_review(frame: &mut Frame, app: &App, area: Rect) {
    if app.review.is_none() && app.review_diff_files.is_empty() {
        let block = Paragraph::new("No review loaded. Press 'b' to go back.")
            .block(Block::default().title(" Review ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    }

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    draw_file_list(frame, app, panels[0]);
    draw_diff_viewer(frame, app, panels[1]);
}

fn draw_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .review_diff_files
        .iter()
        .map(|f| {
            let path = display_path(f);
            let stats = file_change_stats(f);
            let content = Line::from(vec![
                Span::raw(path),
                Span::raw(" "),
                Span::styled(stats, Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let title = review_title(app);

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    if !app.review_diff_files.is_empty() {
        state.select(Some(app.review_file_index));
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn review_title(app: &App) -> String {
    match &app.review {
        Some(r) => format!(" Review #{} - {} ({}) ", r.id, &r.branch, r.status.as_str()),
        None => " Files ".to_string(),
    }
}

fn display_path(file: &DiffFile) -> String {
    if file.new_path == "/dev/null" {
        format!("(deleted) {}", file.old_path)
    } else if file.old_path == "/dev/null" {
        format!("(new) {}", file.new_path)
    } else if file.is_rename {
        format!("{} -> {}", file.old_path, file.new_path)
    } else {
        file.new_path.clone()
    }
}

fn file_change_stats(file: &DiffFile) -> String {
    let mut added = 0usize;
    let mut removed = 0usize;
    for hunk in &file.hunks {
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Added => added += 1,
                DiffLineKind::Removed => removed += 1,
                DiffLineKind::Context => {}
            }
        }
    }
    format!("+{added} -{removed}")
}

fn draw_diff_viewer(frame: &mut Frame, app: &App, area: Rect) {
    let Some(file) = app.review_diff_files.get(app.review_file_index) else {
        let block = Paragraph::new("No file selected.")
            .block(Block::default().title(" Diff ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    };

    let diff_area = if app.review_comment_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        draw_comment_input(frame, app, chunks[1]);
        chunks[0]
    } else {
        area
    };

    let lines = build_diff_lines(file);
    let title = format!(" {} ", file.new_path);

    let paragraph = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .scroll((app.review_scroll, 0));

    frame.render_widget(paragraph, diff_area);
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

fn draw_comment_input(frame: &mut Frame, app: &App, area: Rect) {
    let line_label = app
        .review_comment_line
        .map(|n| format!("line {n}"))
        .unwrap_or_default();

    let display = app.review_comment_input.value().to_string();
    let paragraph = Paragraph::new(display).block(
        Block::default()
            .title(format!(
                " Comment ({line_label}) [Enter:submit Esc:cancel] "
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ViewMode};
    use ca_lib::git_ops::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_diff_files() -> Vec<DiffFile> {
        vec![
            DiffFile {
                old_path: "src/main.rs".to_string(),
                new_path: "src/main.rs".to_string(),
                is_binary: false,
                is_rename: false,
                hunks: vec![
                    DiffHunk {
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
                                kind: DiffLineKind::Removed,
                                content: "    println!(\"hello\");".to_string(),
                                old_lineno: Some(2),
                                new_lineno: None,
                            },
                            DiffLine {
                                kind: DiffLineKind::Added,
                                content: "    println!(\"hello world\");".to_string(),
                                old_lineno: None,
                                new_lineno: Some(2),
                            },
                        ],
                    },
                    DiffHunk {
                        header: "@@ -10,3 +11,4 @@".to_string(),
                        old_start: 10,
                        old_count: 3,
                        new_start: 11,
                        new_count: 4,
                        lines: vec![DiffLine {
                            kind: DiffLineKind::Added,
                            content: "    new_function();".to_string(),
                            old_lineno: None,
                            new_lineno: Some(11),
                        }],
                    },
                ],
            },
            DiffFile {
                old_path: "src/lib.rs".to_string(),
                new_path: "src/lib.rs".to_string(),
                is_binary: false,
                is_rename: false,
                hunks: vec![DiffHunk {
                    header: "@@ -1,2 +1,3 @@".to_string(),
                    old_start: 1,
                    old_count: 2,
                    new_start: 1,
                    new_count: 3,
                    lines: vec![DiffLine {
                        kind: DiffLineKind::Added,
                        content: "pub mod utils;".to_string(),
                        old_lineno: None,
                        new_lineno: Some(1),
                    }],
                }],
            },
        ]
    }

    fn setup_review_app() -> App {
        let mut app = App::new();
        app.view_mode = ViewMode::Review;
        app.review_diff_files = sample_diff_files();
        app
    }

    fn setup_review_app_with_review() -> App {
        let mut app = setup_review_app();
        app.review = Some(ca_lib::review::Review {
            id: 10,
            session_id: None,
            project_id: None,
            branch: "feature".to_string(),
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            status: ca_lib::review::ReviewStatus::Pending,
            round: 1,
            created_at: 0,
            updated_at: 0,
        });
        app
    }

    #[test]
    fn test_draw_review_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new();

        terminal
            .draw(|frame| {
                draw_review(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_draw_review_with_files() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = setup_review_app();

        terminal
            .draw(|frame| {
                draw_review(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_file_navigation() {
        let mut app = setup_review_app();
        assert_eq!(app.review_file_index, 0);

        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.review_file_index, 1);

        // Already at last file, should stay
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.review_file_index, 1);

        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.review_file_index, 0);

        // Already at first file, should stay
        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.review_file_index, 0);
    }

    #[test]
    fn test_scroll_diff() {
        let mut app = setup_review_app();
        assert_eq!(app.review_scroll, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.review_scroll, 1);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.review_scroll, 2);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.review_scroll, 1);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.review_scroll, 0);

        // Should not go below 0 (saturating)
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.review_scroll, 0);
    }

    #[test]
    fn test_hunk_navigation() {
        let mut app = setup_review_app();
        // First file has 2 hunks. Header positions: 0 and 4 (hunk0 header + 3 lines = 4)
        assert_eq!(app.review_scroll, 0);

        app.handle_key(key(KeyCode::Char('n')));
        // Should jump to second hunk (position 4: header + 3 lines from first hunk)
        assert_eq!(app.review_scroll, 4);

        // No more hunks after second, should stay
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.review_scroll, 4);

        app.handle_key(key(KeyCode::Char('p')));
        assert_eq!(app.review_scroll, 0);

        // Already at first hunk, should stay
        app.handle_key(key(KeyCode::Char('p')));
        assert_eq!(app.review_scroll, 0);
    }

    #[test]
    fn test_enter_comment_mode() {
        let mut app = setup_review_app();
        app.review_scroll = 5;
        assert!(!app.review_comment_mode);

        app.handle_key(key(KeyCode::Char('c')));
        assert!(app.review_comment_mode);
        assert_eq!(app.review_comment_line, Some(5));
    }

    #[test]
    fn test_cancel_comment() {
        let mut app = setup_review_app();
        app.handle_key(key(KeyCode::Char('c')));
        assert!(app.review_comment_mode);

        // Type something
        app.handle_key(key(KeyCode::Char('h')));
        app.handle_key(key(KeyCode::Char('i')));

        // Cancel
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.review_comment_mode);
        assert!(app.review_comment_line.is_none());
        assert!(app.review_comment_input.value().is_empty());
    }

    #[test]
    fn test_back_returns() {
        let mut app = setup_review_app();
        assert_eq!(app.view_mode, ViewMode::Review);

        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::PlanDetail);
        assert!(app.review.is_none());
        assert!(app.review_diff_files.is_empty());
    }

    #[test]
    fn test_submit_comment_returns_action() {
        let mut app = setup_review_app();
        app.review = Some(ca_lib::review::Review {
            id: 42,
            session_id: None,
            project_id: None,
            branch: "main".to_string(),
            base_commit: "aaa".to_string(),
            head_commit: "bbb".to_string(),
            status: ca_lib::review::ReviewStatus::Pending,
            round: 1,
            created_at: 0,
            updated_at: 0,
        });

        app.handle_key(key(KeyCode::Char('c')));
        assert!(app.review_comment_mode);

        // Type comment text
        app.handle_key(key(KeyCode::Char('f')));
        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('x')));

        let action = app.handle_key(key(KeyCode::Enter));
        assert!(!app.review_comment_mode);

        match action {
            crate::app::AppAction::AddReviewComment {
                review_id,
                file_path,
                line_number,
                body,
            } => {
                assert_eq!(review_id, 42);
                assert_eq!(file_path, "src/main.rs");
                assert_eq!(line_number, 0);
                assert_eq!(body, "fix");
            }
            other => panic!("Expected AddReviewComment, got {:?}", other),
        }
    }

    #[test]
    fn test_file_navigation_resets_scroll() {
        let mut app = setup_review_app();
        app.review_scroll = 10;

        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.review_file_index, 1);
        assert_eq!(app.review_scroll, 0);
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

    #[test]
    fn test_display_path_variants() {
        let normal = DiffFile {
            old_path: "a.rs".to_string(),
            new_path: "a.rs".to_string(),
            is_binary: false,
            is_rename: false,
            hunks: vec![],
        };
        assert_eq!(display_path(&normal), "a.rs");

        let deleted = DiffFile {
            old_path: "old.rs".to_string(),
            new_path: "/dev/null".to_string(),
            is_binary: false,
            is_rename: false,
            hunks: vec![],
        };
        assert_eq!(display_path(&deleted), "(deleted) old.rs");

        let new_file = DiffFile {
            old_path: "/dev/null".to_string(),
            new_path: "new.rs".to_string(),
            is_binary: false,
            is_rename: false,
            hunks: vec![],
        };
        assert_eq!(display_path(&new_file), "(new) new.rs");

        let renamed = DiffFile {
            old_path: "old.rs".to_string(),
            new_path: "new.rs".to_string(),
            is_binary: false,
            is_rename: true,
            hunks: vec![],
        };
        assert_eq!(display_path(&renamed), "old.rs -> new.rs");
    }

    #[test]
    fn test_v_returns_vimdiff_action() {
        let mut app = setup_review_app_with_review();
        let action = app.handle_key(key(KeyCode::Char('v')));
        match action {
            crate::app::AppAction::OpenVimdiff {
                base_commit,
                head_commit,
                file_path,
            } => {
                assert_eq!(base_commit, "abc123");
                assert_eq!(head_commit, "def456");
                assert_eq!(file_path, "src/main.rs");
            }
            other => panic!("Expected OpenVimdiff, got {:?}", other),
        }
    }

    #[test]
    fn test_d_returns_delta_action() {
        let mut app = setup_review_app_with_review();
        let action = app.handle_key(key(KeyCode::Char('d')));
        match action {
            crate::app::AppAction::OpenDelta {
                base_commit,
                head_commit,
                file_path,
            } => {
                assert_eq!(base_commit, "abc123");
                assert_eq!(head_commit, "def456");
                assert_eq!(file_path, "src/main.rs");
            }
            other => panic!("Expected OpenDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_v_no_review_returns_none() {
        let mut app = setup_review_app();
        // app has diff files but no Review loaded
        let action = app.handle_key(key(KeyCode::Char('v')));
        assert!(matches!(action, crate::app::AppAction::None));
    }

    #[test]
    fn test_d_no_files_returns_none() {
        let mut app = App::new();
        app.view_mode = ViewMode::Review;
        app.review = Some(ca_lib::review::Review {
            id: 10,
            session_id: None,
            project_id: None,
            branch: "feature".to_string(),
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            status: ca_lib::review::ReviewStatus::Pending,
            round: 1,
            created_at: 0,
            updated_at: 0,
        });
        // review loaded but no diff files
        let action = app.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(action, crate::app::AppAction::None));
    }
}
