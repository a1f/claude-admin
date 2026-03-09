use crate::app::{App, OrchPanel};
use crate::plan_view;
use ca_lib::models::SessionState;
use ca_lib::plan::StepStatus;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn draw_orchestrator(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    draw_orch_steps(frame, app, panels[0]);
    draw_orch_sessions(frame, app, panels[1]);
    draw_orch_status(frame, app, chunks[1]);
}

fn draw_orch_steps(frame: &mut Frame, app: &App, area: Rect) {
    let Some(plan) = &app.current_plan else {
        let block = Paragraph::new("No plan loaded.")
            .block(Block::default().title(" Steps ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    };

    let active = app.orch_panel == OrchPanel::Steps;
    let title = format!(" Steps - {} ", plan.name);
    let border_style = panel_border_style(active);

    let visible = app.visible_steps();
    let linked_step_ids: Vec<&str> = app
        .project_sessions()
        .iter()
        .filter_map(|s| s.plan_step_id.as_deref())
        .collect();

    let mut items: Vec<ListItem> = Vec::new();
    let mut list_index_for_step: Vec<Option<usize>> = Vec::new();

    for (pi, phase) in plan.content.phases.iter().enumerate() {
        let completed = phase
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let total = phase.steps.len();

        let header = Line::from(Span::styled(
            format!("{} [{}/{}]", phase.name, completed, total),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        items.push(ListItem::new(header));
        list_index_for_step.push(None);

        for (si, step) in phase.steps.iter().enumerate() {
            let indicator = plan_view::step_indicator(&step.status);
            let color = plan_view::step_color(&step.status);
            let session_tag = if linked_step_ids.contains(&step.id.as_str()) {
                "[S] "
            } else {
                ""
            };

            let content = Line::from(vec![
                Span::styled(indicator, Style::default().fg(color)),
                Span::styled(&step.id, Style::default().fg(color)),
                Span::raw("  "),
                Span::raw(&step.description),
                Span::raw("  "),
                Span::styled(session_tag, Style::default().fg(Color::Cyan)),
            ]);
            items.push(ListItem::new(content));

            let flat_idx = visible.iter().position(|&(vp, vs)| vp == pi && vs == si);
            list_index_for_step.push(flat_idx);
        }
    }

    let selected_row = list_index_for_step
        .iter()
        .position(|idx| *idx == Some(app.step_index));

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    if active {
        state.select(selected_row);
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_orch_sessions(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.orch_panel == OrchPanel::Sessions;
    let border_style = panel_border_style(active);
    let sessions = app.project_sessions();

    if sessions.is_empty() {
        let block = Paragraph::new("No active sessions. Press 's' to spawn.").block(
            Block::default()
                .title(" Sessions ")
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        frame.render_widget(block, area);
        return;
    }

    let items: Vec<ListItem> = sessions
        .iter()
        .map(|s| {
            let indicator = session_state_indicator(&s.state);
            let color = session_state_color(&s.state);
            let step_label = s.plan_step_id.as_deref().unwrap_or("-");

            let content = Line::from(vec![
                Span::styled(indicator, Style::default().fg(color)),
                Span::raw(&s.id[..8.min(s.id.len())]),
                Span::raw("  "),
                Span::styled(
                    format!("step:{step_label}"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(s.state.as_str(), Style::default().fg(color)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Sessions ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    if active {
        state.select(Some(app.orch_session_index));
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_orch_status(frame: &mut Frame, app: &App, area: Rect) {
    let panel_name = match app.orch_panel {
        OrchPanel::Steps => "steps",
        OrchPanel::Sessions => "sessions",
    };

    let status = Line::from(vec![
        Span::styled(" s", Style::default().fg(Color::Cyan)),
        Span::raw(":spawn  "),
        Span::styled("a", Style::default().fg(Color::Cyan)),
        Span::raw(":attach  "),
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::raw(":switch panel  "),
        Span::styled("b", Style::default().fg(Color::Cyan)),
        Span::raw(":back  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(":quit  "),
        Span::styled(
            format!("[{panel_name}]"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let bar = Paragraph::new(status);
    frame.render_widget(bar, area);
}

fn panel_border_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn session_state_indicator(state: &SessionState) -> &'static str {
    match state {
        SessionState::Working => "* ",
        SessionState::NeedsInput => "! ",
        SessionState::Done => "- ",
        SessionState::Idle => "  ",
    }
}

fn session_state_color(state: &SessionState) -> Color {
    match state {
        SessionState::Working => Color::Green,
        SessionState::NeedsInput => Color::Yellow,
        SessionState::Done => Color::DarkGray,
        SessionState::Idle => Color::White,
    }
}
