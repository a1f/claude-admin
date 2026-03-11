use crate::app::{App, InputMode, ViewMode};
use crate::command_palette::CommandPalette;
use crate::doc_view;
use crate::form::FormOverlay;
use crate::git_view;
use crate::plan_view;
use crate::project_view;
use crate::resource_view;
use crate::review_view;
use ca_lib::models::{Session, SessionState};
use ca_lib::project::Project;
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

    match app.view_mode {
        ViewMode::Sessions => draw_sessions(frame, app, main_area),
        ViewMode::Projects => plan_view::draw_projects(frame, app, main_area),
        ViewMode::Plans => plan_view::draw_plans(frame, app, main_area),
        ViewMode::PlanDetail => plan_view::draw_plan_detail(frame, app, main_area),
        ViewMode::Orchestrator => project_view::draw_orchestrator(frame, app, main_area),
        ViewMode::Review => review_view::draw_review(frame, app, main_area),
        ViewMode::Git => git_view::draw_git(frame, app, main_area),
        ViewMode::Resources => resource_view::draw_resources(frame, app, main_area),
        ViewMode::Document => doc_view::draw_document(frame, app, main_area),
        ViewMode::PlanHistory => plan_view::draw_plan_history(frame, app, main_area),
    }

    if app.input_mode == InputMode::Command {
        draw_command_bar(frame, app, bar_area);
        if !app.command_palette.suggestions.is_empty() {
            draw_suggestions(frame, &app.command_palette, bar_area);
        }
    } else {
        draw_status_bar(frame, app, bar_area);
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
    let visible = app.visible_sessions();
    if visible.is_empty() {
        let msg = if app.show_untracked {
            "No untracked sessions. Press 'i' to show all."
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

    let in_reply = app.input_mode == InputMode::SessionReply;
    let main_area = if in_reply {
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        draw_reply_bar(frame, app, vert[1]);
        vert[0]
    } else {
        area
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
        .split(main_area);

    draw_session_list(frame, app, chunks[0]);
    draw_preview(frame, app, chunks[1]);

    if let Some(projects) = &app.project_picker {
        draw_project_picker(frame, projects, app.picker_index, area);
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
    let blink = app.blink_on();
    let groups = app.grouped_sessions();

    // Build items with group headers; track which list index maps to selected_index
    let mut items: Vec<ListItem> = Vec::new();
    let mut highlight_index: Option<usize> = None;

    for (group_name, session_indices) in &groups {
        // Group header (non-selectable visual separator)
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
        " Sessions [untracked] (Enter:attach i:all p:assign ?:help) "
    } else {
        " Sessions (Enter:attach p:projects i:untracked ?:help) "
    };

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut list_state = ListState::default();
    list_state.select(highlight_index);

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    let Some(session) = app.selected_session() else {
        let empty = Paragraph::new("No session selected.")
            .block(Block::default().title(" Preview ").borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    };

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let header = Line::from(vec![
        Span::styled(session_display_name(session), bold),
        Span::raw("  "),
        Span::styled(
            session.state.as_str(),
            Style::default().fg(state_color(&session.state)),
        ),
        Span::raw("  "),
        Span::styled(&session.pane_id, Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines = vec![header, Line::from("")];

    if app.pane_preview_lines.is_empty() {
        lines.push(Line::from("Loading pane content..."));
    } else {
        for raw_line in &app.pane_preview_lines {
            lines.push(Line::from(raw_line.as_str()));
        }
    }

    let preview = Paragraph::new(lines).block(
        Block::default()
            .title(" Pane Output ")
            .borders(Borders::ALL),
    );

    frame.render_widget(preview, area);
}

fn draw_reply_bar(frame: &mut Frame, app: &App, area: Rect) {
    let display = format!("Reply> {}", app.session_reply_input);
    let bar = Paragraph::new(display).block(
        Block::default()
            .title(" Reply to session (Enter:send Esc:cancel) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(bar, area);
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

fn state_color(state: &SessionState) -> Color {
    match state {
        SessionState::Working => Color::Green,
        SessionState::NeedsInput => Color::Yellow,
        SessionState::Done => Color::DarkGray,
        SessionState::Idle => Color::White,
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
    let right_msg = if let Some(msg) = &app.command_palette.message {
        msg.clone()
    } else if let Some((msg, _)) = &app.status_message {
        msg.clone()
    } else {
        String::new()
    };

    let left_hints = match app.view_mode {
        ViewMode::Sessions => "Enter/a:attach t:reply n:next-input p:projects ?:help",
        ViewMode::Projects => "n:new d:del b:back ?:help",
        ViewMode::Plans => "n:new d:del b:back ?:help",
        ViewMode::PlanDetail => "s:status o:orch b:back ?:help",
        ViewMode::Orchestrator => "Tab:panel s:spawn a:attach b:back ?:help",
        ViewMode::Review => "j/k:scroll n/p:hunk h/l:file c:comment b:back ?:help",
        ViewMode::Git => "j/k:commits Enter:diff n/p:scroll h/l:file b:back ?:help",
        ViewMode::Resources => "j/k:navigate t:time-filter s:sort b:back ?:help",
        ViewMode::Document => {
            "j/k:scroll c:comment n/p:nav-comments r:resolve S:send b:back ?:help"
        }
        ViewMode::PlanHistory => "j/k:navigate r:restore b:back ?:help",
    };

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
        ViewMode::Review => "Review",
        ViewMode::Git => "Git",
        ViewMode::Resources => "Resources",
        ViewMode::Document => "Document",
        ViewMode::PlanHistory => "Plan History",
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

fn draw_project_picker(frame: &mut Frame, projects: &[Project], selected: usize, area: Rect) {
    let height = (projects.len() as u16 + 2).min(12);
    let width = 40u16.min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let items: Vec<ListItem> = projects
        .iter()
        .map(|p| ListItem::new(p.name.as_str()))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Assign to Project (Enter/Esc) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut list_state = ListState::default();
    list_state.select(Some(selected));
    frame.render_stateful_widget(list, popup, &mut list_state);
}
