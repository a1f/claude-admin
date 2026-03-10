use crate::app::App;
use ca_lib::resource::ResourceSummary;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFilter {
    Today,
    Week,
    All,
}

impl TimeFilter {
    pub fn label(&self) -> &'static str {
        match self {
            TimeFilter::Today => "Today",
            TimeFilter::Week => "Week",
            TimeFilter::All => "All",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            TimeFilter::Today => TimeFilter::Week,
            TimeFilter::Week => TimeFilter::All,
            TimeFilter::All => TimeFilter::Today,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSort {
    Tokens,
    Cost,
}

impl ResourceSort {
    pub fn label(&self) -> &'static str {
        match self {
            ResourceSort::Tokens => "Tokens",
            ResourceSort::Cost => "Cost",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            ResourceSort::Tokens => ResourceSort::Cost,
            ResourceSort::Cost => ResourceSort::Tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionResourceRow {
    pub session_id: String,
    pub working_dir: String,
    pub summary: ResourceSummary,
}

pub fn draw_resources(frame: &mut Frame, app: &App, area: Rect) {
    if app.resource_rows.is_empty() {
        let msg = Paragraph::new("No resource data. Press 'R' from Sessions to load.")
            .block(Block::default().title(" Resources ").borders(Borders::ALL));
        frame.render_widget(msg, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(5)])
        .split(area);

    draw_session_table(frame, app, chunks[0]);
    draw_project_totals(frame, app, chunks[1]);
}

fn draw_session_table(frame: &mut Frame, app: &App, area: Rect) {
    let filter_label = app.resource_time_filter.label();
    let sort_label = app.resource_sort.label();
    let title = format!(
        " Resources ({}) [t:{} s:{}] ",
        app.resource_rows.len(),
        filter_label,
        sort_label,
    );

    let header = Row::new(vec!["Session", "Dir", "In Tokens", "Out Tokens", "Cost"])
        .style(Style::default().fg(Color::Yellow));

    let rows: Vec<Row> = app
        .resource_rows
        .iter()
        .map(|r| {
            Row::new(vec![
                short_id(&r.session_id),
                short_path(&r.working_dir),
                format_tokens(r.summary.input_tokens),
                format_tokens(r.summary.output_tokens),
                format_cost(r.summary.cost),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ratatui::widgets::TableState::default();
    state.select(Some(app.resource_index));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_project_totals(frame: &mut Frame, app: &App, area: Rect) {
    let s = &app.resource_project_summary;
    let total_in = format_tokens(s.input_tokens);
    let total_out = format_tokens(s.output_tokens);
    let total_cache = format_tokens(s.cache_read_tokens + s.cache_creation_tokens);
    let total_cost = format_cost(s.cost);

    let lines = vec![
        Line::from(vec![
            Span::styled(" Totals ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "In: {}  Out: {}  Cache: {}  Cost: {}",
                total_in, total_out, total_cache, total_cost
            )),
        ]),
        Line::from(vec![Span::styled(
            format!(
                " Total tokens: {} ",
                format_tokens(
                    s.input_tokens
                        + s.output_tokens
                        + s.cache_read_tokens
                        + s.cache_creation_tokens
                )
            ),
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let block = Block::default()
        .title(" Project Summary ")
        .borders(Borders::ALL);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn format_tokens(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{:.0}", n)
    }
}

fn format_cost(c: f64) -> String {
    if c < 0.01 {
        format!("${:.4}", c)
    } else {
        format!("${:.2}", c)
    }
}

fn short_id(id: &str) -> String {
    if id.len() > 8 {
        format!("{}…", &id[..8])
    } else {
        id.to_string()
    }
}

fn short_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ViewMode};
    use ca_lib::resource::ResourceSummary;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_row(id: &str, dir: &str, input: f64, output: f64, cost: f64) -> SessionResourceRow {
        SessionResourceRow {
            session_id: id.to_string(),
            working_dir: dir.to_string(),
            summary: ResourceSummary {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: 0.0,
                cache_creation_tokens: 0.0,
                cost,
            },
        }
    }

    #[test]
    fn test_draw_resources_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new();

        terminal
            .draw(|frame| {
                draw_resources(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_draw_resources_with_data() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.view_mode = ViewMode::Resources;
        app.resource_rows = vec![
            make_row("sess-1", "/home/user/proj-a", 5000.0, 1200.0, 0.05),
            make_row("sess-2", "/home/user/proj-b", 3000.0, 800.0, 0.03),
        ];
        app.resource_project_summary = ResourceSummary {
            input_tokens: 8000.0,
            output_tokens: 2000.0,
            cache_read_tokens: 0.0,
            cache_creation_tokens: 0.0,
            cost: 0.08,
        };

        terminal
            .draw(|frame| {
                draw_resources(frame, &app, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn test_navigate_resource_rows() {
        let mut app = App::new();
        app.view_mode = ViewMode::Resources;
        app.resource_rows = vec![
            make_row("sess-1", "/a", 5000.0, 1200.0, 0.05),
            make_row("sess-2", "/b", 3000.0, 800.0, 0.03),
        ];
        assert_eq!(app.resource_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.resource_index, 1);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.resource_index, 0);
    }

    #[test]
    fn test_time_filter_toggle() {
        let mut app = App::new();
        app.view_mode = ViewMode::Resources;
        app.resource_rows = vec![make_row("s1", "/a", 100.0, 50.0, 0.01)];

        assert_eq!(app.resource_time_filter, TimeFilter::All);

        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.resource_time_filter, TimeFilter::Today);

        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.resource_time_filter, TimeFilter::Week);

        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.resource_time_filter, TimeFilter::All);
    }

    #[test]
    fn test_sort_toggle() {
        let mut app = App::new();
        app.view_mode = ViewMode::Resources;
        app.resource_rows = vec![make_row("s1", "/a", 100.0, 50.0, 0.01)];

        assert_eq!(app.resource_sort, ResourceSort::Tokens);

        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.resource_sort, ResourceSort::Cost);

        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.resource_sort, ResourceSort::Tokens);
    }

    #[test]
    fn test_back_returns_to_sessions() {
        let mut app = App::new();
        app.view_mode = ViewMode::Resources;
        app.resource_rows = vec![make_row("s1", "/a", 100.0, 50.0, 0.01)];

        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Sessions);
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500.0), "500");
        assert_eq!(format_tokens(1500.0), "1.5k");
        assert_eq!(format_tokens(2_500_000.0), "2.5M");
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(0.001), "$0.0010");
        assert_eq!(format_cost(1.50), "$1.50");
    }

    #[test]
    fn test_short_id() {
        assert_eq!(short_id("abcdefghij"), "abcdefgh…");
        assert_eq!(short_id("short"), "short");
    }

    #[test]
    fn test_short_path() {
        assert_eq!(short_path("/home/user/myproject"), "myproject");
        assert_eq!(short_path("simple"), "simple");
    }

    #[test]
    fn test_time_filter_next() {
        assert_eq!(TimeFilter::Today.next(), TimeFilter::Week);
        assert_eq!(TimeFilter::Week.next(), TimeFilter::All);
        assert_eq!(TimeFilter::All.next(), TimeFilter::Today);
    }

    #[test]
    fn test_resource_sort_toggle() {
        assert_eq!(ResourceSort::Tokens.toggle(), ResourceSort::Cost);
        assert_eq!(ResourceSort::Cost.toggle(), ResourceSort::Tokens);
    }
}
