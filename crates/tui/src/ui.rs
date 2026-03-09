use crate::app::{App, InputMode, ViewMode};
use crate::command_palette::CommandPalette;
use crate::form::FormOverlay;
use crate::plan_view;
use crate::project_view;
use ca_lib::events::EventType;
use ca_lib::models::SessionState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let (main_area, bar_area) = if app.input_mode == InputMode::Command {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    match app.view_mode {
        ViewMode::Sessions => draw_sessions(frame, app, main_area),
        ViewMode::Projects => plan_view::draw_projects(frame, app, main_area),
        ViewMode::Plans => plan_view::draw_plans(frame, app, main_area),
        ViewMode::PlanDetail => plan_view::draw_plan_detail(frame, app, main_area),
        ViewMode::Orchestrator => project_view::draw_orchestrator(frame, app, main_area),
    }

    if let Some(bar) = bar_area {
        draw_command_bar(frame, app, bar);
        if !app.command_palette.suggestions.is_empty() {
            draw_suggestions(frame, &app.command_palette, bar);
        }
    }

    // Show confirmation or feedback message when not in command mode
    if app.input_mode != InputMode::Command {
        if let Some(msg) = &app.command_palette.message {
            let bar_area = Rect::new(area.x, area.height.saturating_sub(1), area.width, 1);
            let msg_widget = Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Yellow).bg(Color::DarkGray));
            frame.render_widget(msg_widget, bar_area);
        }
    }

    if let Some(form) = &app.form_overlay {
        if app.input_mode == InputMode::Form {
            draw_form_overlay(frame, form, area);
        }
    }

    if app.input_mode == InputMode::Help {
        draw_help_overlay(frame, app, area);
    }
}

fn draw_sessions(frame: &mut Frame, app: &App, area: Rect) {
    if app.sessions.is_empty() {
        let msg = if app.connected {
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
        .block(
            Block::default()
                .title(" Sessions (p:projects N:new-ws ?:help) ")
                .borders(Borders::ALL),
        )
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
        Line::from(vec![Span::styled("ID: ", bold), Span::raw(&session.id)]),
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

fn draw_command_bar(frame: &mut Frame, app: &App, area: Rect) {
    let display = format!(":{}", app.command_palette.input.value());
    let bar = Paragraph::new(display).style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(bar, area);
}

fn draw_suggestions(frame: &mut Frame, palette: &CommandPalette, bar_area: Rect) {
    let count = palette.suggestions.len().min(8) as u16;
    if count == 0 || bar_area.y < count {
        return;
    }

    let popup_area = Rect::new(
        bar_area.x,
        bar_area.y.saturating_sub(count),
        bar_area.width.min(40),
        count,
    );

    frame.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = palette
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == palette.selected_suggestion {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(s.as_str()).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(Color::Black)),
    );
    frame.render_widget(list, popup_area);
}

fn draw_form_overlay(frame: &mut Frame, form: &FormOverlay, area: Rect) {
    let width = area.width * 60 / 100;
    let height = area.height * 50 / 100;
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", form.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let field_count = form.fields.len();
    let mut constraints: Vec<Constraint> = Vec::new();
    for _ in 0..field_count {
        constraints.push(Constraint::Length(2));
    }
    if form.error_message.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    for (i, field) in form.fields.iter().enumerate() {
        let is_focused = i == form.focused_field;
        draw_form_field(frame, field, is_focused, chunks[i]);
    }

    if let Some(err) = &form.error_message {
        let err_idx = field_count;
        if err_idx < chunks.len() {
            let err_widget = Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red));
            frame.render_widget(err_widget, chunks[err_idx]);
        }
    }
}

fn draw_form_field(
    frame: &mut Frame,
    field: &crate::form::FormField,
    is_focused: bool,
    area: Rect,
) {
    if area.height < 2 {
        return;
    }

    let label_style = if is_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    let required_marker = if field.required { " *" } else { "" };
    let label = format!("{}{}", field.input.label(), required_marker);

    let input_style = if is_focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let label_widget = Paragraph::new(label).style(label_style);
    frame.render_widget(label_widget, field_chunks[0]);

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let display = field.input.value().to_string();
    let input_widget = Paragraph::new(display).style(input_style).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(border_style),
    );
    frame.render_widget(input_widget, field_chunks[1]);
}

fn draw_help_overlay(frame: &mut Frame, app: &App, area: Rect) {
    use crate::help::help_content;

    let entries = help_content(app.view_mode);

    let width = area.width * 70 / 100;
    let height = area.height * 80 / 100;
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let view_name = match app.view_mode {
        ViewMode::Sessions => "Sessions",
        ViewMode::Projects => "Projects",
        ViewMode::Plans => "Plans",
        ViewMode::PlanDetail => "Plan Detail",
        ViewMode::Orchestrator => "Orchestrator",
    };

    let block = Block::default()
        .title(format!(" Help \u{2014} {view_name} (? or Esc to close) "))
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
        Cell::from("CLI Command").style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
    ]);

    let rows: Vec<Row> = entries
        .iter()
        .map(|(key_str, action, cli)| {
            Row::new(vec![
                Cell::from(*key_str).style(Style::default().fg(Color::Yellow)),
                Cell::from(*action),
                Cell::from(*cli).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Min(20),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default());

    frame.render_widget(table, inner);
}
