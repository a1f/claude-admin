use crate::app::App;
use ca_lib::events::EventType;
use ca_lib::models::SessionState;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if app.sessions.is_empty() {
        let msg = if app.connected {
            "No sessions found. Waiting for data..."
        } else {
            "Not connected to daemon. Is it running? (claude-admin daemon start)"
        };
        let block = Paragraph::new(msg)
            .block(Block::default().title(" claude-admin ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    draw_session_list(frame, app, chunks[0]);
    draw_preview(frame, app, chunks[1]);
}

fn draw_session_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            let indicator = state_indicator(&s.state);
            let color = state_color(&s.state);
            let content = Line::from(vec![
                Span::styled(indicator, Style::default().fg(color)),
                Span::raw(&s.id),
                Span::raw("  "),
                Span::styled(s.state.as_str(), Style::default().fg(color)),
                Span::raw("  "),
                Span::raw(&s.working_dir),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(" Sessions ").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut list_state = ListState::default();
    list_state.select(Some(app.selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    let content = match app.selected_session() {
        None => vec![Line::from("No session selected.")],
        Some(session) => build_preview_lines(app, session),
    };

    let preview = Paragraph::new(content)
        .block(Block::default().title(" Preview ").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(preview, area);
}

fn build_preview_lines<'a>(app: &'a App, session: &'a ca_lib::models::Session) -> Vec<Line<'a>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("ID: ", bold),
            Span::raw(&session.id),
        ]),
        Line::from(vec![
            Span::styled("State: ", bold),
            Span::styled(
                session.state.as_str(),
                Style::default().fg(state_color(&session.state)),
            ),
        ]),
        Line::from(vec![
            Span::styled("Pane: ", bold),
            Span::raw(&session.pane_id),
        ]),
        Line::from(vec![
            Span::styled("Dir: ", bold),
            Span::raw(&session.working_dir),
        ]),
        Line::from(""),
        Line::styled("--- Events ---", bold),
    ];

    if app.preview_events.is_empty() {
        lines.push(Line::from("No events yet."));
    } else {
        for event in &app.preview_events {
            let detail = format_event_type(&event.event_type);
            lines.push(Line::from(format!("  {} {}", event.id, detail)));
        }
    }

    lines
}

fn format_event_type(event_type: &EventType) -> String {
    match event_type {
        EventType::StateChanged { from, to } => format!("state: {} -> {}", from, to),
        EventType::HookReceived { hook_type } => format!("hook: {}", hook_type),
        EventType::SessionDiscovered => "session_discovered".to_string(),
        EventType::SessionRemoved => "session_removed".to_string(),
    }
}

fn state_indicator(state: &SessionState) -> &'static str {
    match state {
        SessionState::Working => "* ",
        SessionState::NeedsInput => "! ",
        SessionState::Done => "- ",
        SessionState::Idle => "  ",
    }
}

fn state_color(state: &SessionState) -> Color {
    match state {
        SessionState::Working => Color::Green,
        SessionState::NeedsInput => Color::Yellow,
        SessionState::Done => Color::DarkGray,
        SessionState::Idle => Color::White,
    }
}
