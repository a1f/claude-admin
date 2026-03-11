use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct DocComment {
    pub line: u32,
    pub body: String,
    pub resolved: bool,
}

pub fn draw_document(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref doc) = app.doc_lines else {
        let msg = if let Some(ref err) = app.doc_error {
            format!("Error: {err}")
        } else {
            "No document loaded. Use ':doc <path>' to open a file.".to_string()
        };
        let block =
            Paragraph::new(msg).block(Block::default().title(" Document ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    };

    let content_area = if app.doc_comment_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        draw_comment_input(frame, app, chunks[1]);
        chunks[0]
    } else {
        area
    };

    let title = format!(
        " {} ({} lines, {} comments) ",
        app.doc_path.as_deref().unwrap_or("Document"),
        doc.len(),
        app.doc_comments.iter().filter(|c| !c.resolved).count(),
    );

    let commented_lines: HashSet<u32> = app
        .doc_comments
        .iter()
        .filter(|c| !c.resolved)
        .map(|c| c.line)
        .collect();

    let current_line = app.doc_scroll as u32 + 1;
    let lines: Vec<Line> = doc
        .iter()
        .enumerate()
        .map(|(i, content)| {
            let line_no = (i + 1) as u32;
            let is_current = line_no == current_line;
            let has_comment = commented_lines.contains(&line_no);

            let marker = if has_comment { ">" } else { " " };
            let marker_style = Style::default().fg(Color::Yellow);

            let no_style = if is_current {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            Line::from(vec![
                Span::styled(marker.to_string(), marker_style),
                Span::styled(format!("{:>4} ", line_no), no_style),
                Span::raw(content.clone()),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .scroll((app.doc_scroll, 0));

    frame.render_widget(paragraph, content_area);
}

fn draw_comment_input(frame: &mut Frame, app: &App, area: Rect) {
    let line_label = app
        .doc_comment_line
        .map(|n| format!("line {n}"))
        .unwrap_or_default();

    let display = app.doc_comment_input.value().to_string();
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
    use crate::app::{App, AppAction, ViewMode};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_load_file() {
        let mut app = App::new();
        app.doc_lines = Some(vec![
            "line one".to_string(),
            "line two".to_string(),
            "line three".to_string(),
        ]);
        app.doc_path = Some("test.md".to_string());
        app.view_mode = ViewMode::Document;

        assert_eq!(app.doc_lines.as_ref().unwrap().len(), 3);
        assert_eq!(app.doc_path.as_deref(), Some("test.md"));
    }

    #[test]
    fn test_scroll_works() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into(), "b".into(), "c".into()]);

        assert_eq!(app.doc_scroll, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.doc_scroll, 1);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.doc_scroll, 2);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.doc_scroll, 1);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.doc_scroll, 0);
        // Can't go below 0
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.doc_scroll, 0);
    }

    #[test]
    fn test_add_comment_at_line() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into(), "b".into()]);
        app.doc_scroll = 1;

        // Press 'c' to enter comment mode
        app.handle_key(key(KeyCode::Char('c')));
        assert!(app.doc_comment_mode);
        assert_eq!(app.doc_comment_line, Some(2)); // scroll 1 => line 2

        // Type a comment
        app.handle_key(key(KeyCode::Char('H')));
        app.handle_key(key(KeyCode::Char('i')));

        // Submit with Enter
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(!app.doc_comment_mode);
        match action {
            AppAction::AddDocComment { line, body } => {
                assert_eq!(line, 2);
                assert_eq!(body, "Hi");
            }
            other => panic!("Expected AddDocComment, got {:?}", other),
        }
    }

    #[test]
    fn test_list_comments() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into(), "b".into(), "c".into()]);
        app.doc_comments = vec![
            DocComment {
                line: 1,
                body: "First".into(),
                resolved: false,
            },
            DocComment {
                line: 3,
                body: "Third".into(),
                resolved: false,
            },
        ];

        assert_eq!(app.doc_comments.len(), 2);
        assert_eq!(app.doc_comments[0].line, 1);
        assert_eq!(app.doc_comments[1].line, 3);
    }

    #[test]
    fn test_resolve_comment() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into(), "b".into()]);
        app.doc_comments = vec![DocComment {
            line: 1,
            body: "Fix".into(),
            resolved: false,
        }];
        app.doc_comment_index = 0;

        // Press 'r' to resolve
        let action = app.handle_key(key(KeyCode::Char('r')));
        match action {
            AppAction::ResolveDocComment(idx) => assert_eq!(idx, 0),
            other => panic!("Expected ResolveDocComment, got {:?}", other),
        }
    }

    #[test]
    fn test_navigate_between_comments() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ]);
        app.doc_comments = vec![
            DocComment {
                line: 1,
                body: "One".into(),
                resolved: false,
            },
            DocComment {
                line: 3,
                body: "Three".into(),
                resolved: false,
            },
            DocComment {
                line: 5,
                body: "Five".into(),
                resolved: false,
            },
        ];

        assert_eq!(app.doc_comment_index, 0);

        // 'n' goes to next comment
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.doc_comment_index, 1);
        // scroll should jump to that comment's line
        assert_eq!(app.doc_scroll, 2); // line 3 => scroll 2

        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.doc_comment_index, 2);
        assert_eq!(app.doc_scroll, 4); // line 5 => scroll 4

        // Wraps around
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.doc_comment_index, 0);
        assert_eq!(app.doc_scroll, 0); // line 1 => scroll 0

        // 'p' goes to previous
        app.handle_key(key(KeyCode::Char('p')));
        assert_eq!(app.doc_comment_index, 2);
    }

    #[test]
    fn test_nonexistent_file_error() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_error = Some("File not found: /no/such/file.txt".to_string());

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_document(frame, &app, frame.area());
            })
            .unwrap();
        // Should render without panic
    }

    #[test]
    fn test_send_feedback() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into()]);
        app.doc_path = Some("test.rs".to_string());
        app.doc_comments = vec![DocComment {
            line: 1,
            body: "Fix this".into(),
            resolved: false,
        }];

        // Set a session for feedback
        app.doc_session_id = Some("sess-42".to_string());

        let action = app.handle_key(key(KeyCode::Char('S')));
        match action {
            AppAction::SendDocFeedback { session_id, .. } => {
                assert_eq!(session_id, "sess-42");
            }
            other => panic!("Expected SendDocFeedback, got {:?}", other),
        }
    }

    #[test]
    fn test_draw_document_with_data() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec![
            "fn main() {".into(),
            "    println!(\"hello\");".into(),
            "}".into(),
        ]);
        app.doc_path = Some("src/main.rs".to_string());
        app.doc_comments = vec![DocComment {
            line: 2,
            body: "Nice print".into(),
            resolved: false,
        }];

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_document(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_back_returns_to_sessions() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into()]);

        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Sessions);
    }

    #[test]
    fn test_cancel_comment() {
        let mut app = App::new();
        app.view_mode = ViewMode::Document;
        app.doc_lines = Some(vec!["a".into()]);

        app.handle_key(key(KeyCode::Char('c')));
        assert!(app.doc_comment_mode);

        app.handle_key(key(KeyCode::Esc));
        // Esc in comment mode cancels, doesn't quit
        assert!(!app.doc_comment_mode);
    }
}
