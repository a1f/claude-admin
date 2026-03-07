use crate::app::App;
use ca_lib::plan::StepStatus;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw_projects(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if app.projects.is_empty() {
        let block = Paragraph::new("No projects found.")
            .block(Block::default().title(" Projects (b: back) ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    }

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|p| {
            let desc = p
                .description
                .as_deref()
                .unwrap_or("");
            let content = Line::from(vec![
                Span::styled(
                    format!("[{}] ", p.status.as_str()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(&p.name),
                Span::raw("  "),
                Span::styled(desc, Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(" Projects (b: back) ").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.project_index));

    frame.render_stateful_widget(list, area, &mut state);
}

pub fn draw_plans(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let project_name = app
        .projects
        .get(app.project_index)
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let title = format!(" Plans - {} (b: back) ", project_name);

    if app.plans.is_empty() {
        let block = Paragraph::new("No plans found.")
            .block(Block::default().title(title).borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    }

    let items: Vec<ListItem> = app
        .plans
        .iter()
        .map(|p| {
            let content = Line::from(vec![
                Span::styled(
                    format!("[{}] ", p.status.as_str()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(&p.name),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.plan_index));

    frame.render_stateful_widget(list, area, &mut state);
}

pub fn draw_plan_detail(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let Some(plan) = &app.current_plan else {
        let block = Paragraph::new("No plan loaded.")
            .block(Block::default().title(" Plan ").borders(Borders::ALL));
        frame.render_widget(block, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    draw_step_list(frame, app, plan, chunks[0]);
    draw_step_detail(frame, app, chunks[1]);
}

fn draw_step_list(
    frame: &mut Frame,
    app: &App,
    plan: &ca_lib::plan::Plan,
    area: Rect,
) {
    let title = format!(" Plan: {} (s: cycle status, b: back) ", plan.name);
    let visible = app.visible_steps();

    let mut items: Vec<ListItem> = Vec::new();
    let mut list_index_for_step: Vec<Option<usize>> = Vec::new();

    for (pi, phase) in plan.content.phases.iter().enumerate() {
        let completed = phase
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let total = phase.steps.len();

        // Phase header line (not selectable, but still in list)
        let header = Line::from(Span::styled(
            format!("Phase: {} [{}/{}]", phase.name, completed, total),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        items.push(ListItem::new(header));
        list_index_for_step.push(None);

        for (si, step) in phase.steps.iter().enumerate() {
            let indicator = step_indicator(&step.status);
            let color = step_color(&step.status);
            let content = Line::from(vec![
                Span::styled(indicator, Style::default().fg(color)),
                Span::styled(&step.id, Style::default().fg(color)),
                Span::raw("  "),
                Span::raw(&step.description),
            ]);
            items.push(ListItem::new(content));

            // Map this list row to the flat step index
            let flat_idx = visible
                .iter()
                .position(|&(vp, vs)| vp == pi && vs == si);
            list_index_for_step.push(flat_idx);
        }
    }

    // Find the list row that corresponds to the current step_index
    let selected_row = list_index_for_step
        .iter()
        .position(|idx| *idx == Some(app.step_index));

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(selected_row);

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_step_detail(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.selected_step() {
        None => vec![Line::from("No step selected.")],
        Some((phase_name, step)) => {
            let bold = Style::default().add_modifier(Modifier::BOLD);
            let color = step_color(&step.status);

            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Step: ", bold),
                    Span::raw(&step.id),
                ]),
                Line::from(vec![
                    Span::styled("Phase: ", bold),
                    Span::raw(phase_name),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", bold),
                    Span::styled(step.status.as_str(), Style::default().fg(color)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Description: ", bold),
                    Span::raw(&step.description),
                ]),
                Line::from(""),
                Line::styled("Exit Criteria:", bold),
                Line::from(format!("  {}", step.exit_criteria.description)),
            ];

            if !step.exit_criteria.commands.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::styled("Commands:", bold));
                for cmd in &step.exit_criteria.commands {
                    lines.push(Line::from(format!("  $ {}", cmd)));
                }
            }

            lines
        }
    };

    let detail = Paragraph::new(lines)
        .block(Block::default().title(" Step Detail ").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(detail, area);
}

pub(crate) fn step_indicator(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "o ",
        StepStatus::InProgress => "* ",
        StepStatus::Completed => "v ",
        StepStatus::Blocked => "x ",
        StepStatus::Skipped => "- ",
    }
}

pub(crate) fn step_color(status: &StepStatus) -> Color {
    match status {
        StepStatus::Pending => Color::White,
        StepStatus::InProgress => Color::Yellow,
        StepStatus::Completed => Color::Green,
        StepStatus::Blocked => Color::Red,
        StepStatus::Skipped => Color::DarkGray,
    }
}
