use crate::app::{App, InputMode};
use ca_lib::models::{Session, SessionState};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let main_area = chunks[0];
    let bar_area = chunks[1];

    draw_session_list(frame, app, main_area);
    draw_status_bar(frame, app, bar_area);

    if app.input_mode == InputMode::Help {
        draw_help_overlay(frame, area);
    }
}

fn session_row_style(state: &SessionState, blink: bool) -> (Style, Style, Style) {
    match state {
        SessionState::NeedsInput => {
            if blink {
                let bold_yellow = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                (bold_yellow, bold_yellow, bold_yellow)
            } else {
                let yellow = Style::default().fg(Color::Yellow);
                (yellow, yellow, Style::default())
            }
        }
        SessionState::Working => {
            let green = if blink {
                Color::LightGreen
            } else {
                Color::Green
            };
            let gs = Style::default().fg(green);
            (gs, gs, Style::default())
        }
        SessionState::Done => {
            let dim = Style::default().fg(Color::DarkGray);
            (dim, dim, dim)
        }
        SessionState::Idle => {
            let white = Style::default().fg(Color::White);
            (white, white, Style::default())
        }
    }
}

fn draw_session_list(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible_sessions();
    if visible.is_empty() {
        let msg = if app.show_untracked {
            "No untracked sessions. Ctrl-I to show all."
        } else if app.connected {
            "No sessions found. Waiting for data..."
        } else {
            "Not connected to daemon. Is it running? (claude-admin daemon start)"
        };
        let block = Paragraph::new(msg).block(
            Block::default()
                .title(" claude-admin ")
                .borders(Borders::ALL),
        );
        frame.render_widget(block, area);
        return;
    }

    let blink = app.blink_on();
    let groups = app.grouped_sessions();
    let mut items: Vec<ListItem> = Vec::new();
    let mut highlight_index: Option<usize> = None;

    for (group_name, session_indices) in &groups {
        let header = ListItem::new(Line::from(Span::styled(
            format!("── {group_name} ──"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        items.push(header);

        for &idx in session_indices {
            let s = visible[idx];
            if idx == app.selected_index {
                highlight_index = Some(items.len());
            }

            let pos = if idx < 9 {
                format!("{} ", idx + 1)
            } else {
                "  ".to_string()
            };
            let indicator = state_indicator(&s.state);
            let (indicator_style, state_style, text_style) = session_row_style(&s.state, blink);

            let host_badge = if s.host.is_some() {
                Span::styled("[R] ", Style::default().fg(Color::Cyan))
            } else {
                Span::raw("")
            };

            let content = Line::from(vec![
                Span::styled(pos, Style::default().fg(Color::DarkGray)),
                Span::styled(indicator, indicator_style),
                host_badge,
                Span::styled(session_display_name(s), text_style),
                Span::raw("  "),
                Span::styled(s.state.as_str(), state_style),
            ]);
            items.push(ListItem::new(content));
        }
    }

    let title = if app.show_untracked {
        " Sessions [untracked] "
    } else {
        " claude-admin "
    };

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut list_state = ListState::default();
    list_state.select(highlight_index);

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn session_display_name(session: &Session) -> String {
    if let Some(step_id) = &session.plan_step_id {
        return format!("step-{step_id}");
    }
    let dir = &session.working_dir;
    if let Some(basename) = std::path::Path::new(dir).file_name() {
        if let Some(name) = basename.to_str() {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    session.id.chars().take(8).collect()
}

fn state_indicator(state: &SessionState) -> &'static str {
    match state {
        SessionState::Working => "* ",
        SessionState::NeedsInput => "! ",
        SessionState::Done => "- ",
        SessionState::Idle => "  ",
    }
}

fn build_state_counts(app: &App) -> Vec<Span<'_>> {
    let (working, needs_input, done, _idle) = app.session_state_counts();
    let mut parts: Vec<Span> = Vec::new();

    if working > 0 {
        parts.push(Span::styled(
            format!("{working} working"),
            Style::default().fg(Color::Green),
        ));
    }
    if needs_input > 0 {
        if !parts.is_empty() {
            parts.push(Span::raw(" | "));
        }
        parts.push(Span::styled(
            format!("{needs_input} needs input"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if done > 0 {
        if !parts.is_empty() {
            parts.push(Span::raw(" | "));
        }
        parts.push(Span::styled(
            format!("{done} done"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    parts
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let right_msg = if let Some((msg, _)) = &app.status_message {
        msg.clone()
    } else {
        String::new()
    };

    let left_hints = "j/k:nav 1-9:select Enter:attach ^I:filter ?:help q:quit";

    let left = Span::styled(
        format!(" {left_hints}"),
        Style::default().fg(Color::DarkGray),
    );
    let right = Span::styled(format!("{right_msg} "), Style::default().fg(Color::Yellow));

    let available = area.width as usize;
    let left_len = left_hints.len() + 1;
    let right_len = right_msg.len() + 1;

    let center_spans = build_state_counts(app);
    let center_text_len: usize = center_spans.iter().map(|s| s.content.len()).sum();

    let mut spans = vec![left];
    if center_text_len > 0 {
        spans.push(Span::raw("  "));
    }
    spans.extend(center_spans);
    let used = left_len
        + (if center_text_len > 0 {
            2 + center_text_len
        } else {
            0
        })
        + right_len;
    let filler_len = available.saturating_sub(used);
    spans.push(Span::raw(" ".repeat(filler_len)));
    spans.push(right);

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(bar, area);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    use crate::help::help_content;

    let entries = help_content();

    let width = area.width * 70 / 100;
    let height = area.height * 60 / 100;
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Help (? or Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let header = Row::new(vec![
        Cell::from("Key").style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
        Cell::from("Action").style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
    ]);

    let rows: Vec<Row> = entries
        .iter()
        .map(|(key_str, action)| {
            Row::new(vec![
                Cell::from(*key_str).style(Style::default().fg(Color::Yellow)),
                Cell::from(*action),
            ])
        })
        .collect();

    let table = Table::new(rows, [Constraint::Length(12), Constraint::Min(20)])
        .header(header)
        .row_highlight_style(Style::default());

    frame.render_widget(table, inner);
}
