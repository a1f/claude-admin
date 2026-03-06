use crate::app::App;
use ca_lib::models::SessionState;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if app.sessions.is_empty() {
        let msg = Paragraph::new("No sessions found. Waiting for data...")
            .block(Block::default().title(" claude-admin ").borders(Borders::ALL));
        frame.render_widget(msg, area);
        return;
    }

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
