use ca_lib::models::{Session, SessionState};
use ca_lib::project::Project;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Help,
}

#[derive(Debug)]
pub enum AppAction {
    None,
    Quit,
    AttachSession(String),
    ShowHelp,
    ToggleUntracked,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected_index: usize,
    pub should_quit: bool,
    pub connected: bool,
    pub input_mode: InputMode,
    pub projects: Vec<Project>,
    pub show_untracked: bool,
    pub status_message: Option<(String, Instant)>,
    pub tick_count: u64,
}

impl App {
    pub fn new() -> Self {
        App {
            sessions: Vec::new(),
            selected_index: 0,
            should_quit: false,
            connected: false,
            input_mode: InputMode::Normal,
            projects: Vec::new(),
            show_untracked: false,
            status_message: None,
            tick_count: 0,
        }
    }

    pub fn update_sessions(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;
        if self.sessions.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.sessions.len() {
            self.selected_index = self.sessions.len() - 1;
        }
    }

    pub fn update_projects(&mut self, projects: Vec<Project>) {
        self.projects = projects;
    }

    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
    }

    /// Whether the "blink" is currently in the ON phase (for visual indicators).
    /// Toggles roughly every 500ms (10 ticks at 50ms poll interval).
    pub fn blink_on(&self) -> bool {
        (self.tick_count / 10) % 2 == 0
    }

    pub fn clear_stale_status(&mut self) {
        if let Some((_, instant)) = &self.status_message {
            if instant.elapsed() > Duration::from_secs(5) {
                self.status_message = None;
            }
        }
    }

    pub fn visible_sessions(&self) -> Vec<&Session> {
        if self.show_untracked {
            self.sessions
                .iter()
                .filter(|s| s.project_id.is_none())
                .collect()
        } else {
            self.sessions.iter().collect()
        }
    }

    pub fn select_next(&mut self) {
        let count = self.visible_sessions().len();
        if count == 0 {
            return;
        }
        self.selected_index = (self.selected_index + 1) % count;
    }

    pub fn select_prev(&mut self) {
        let count = self.visible_sessions().len();
        if count == 0 {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = count - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn select_by_number(&mut self, n: usize) {
        let count = self.visible_sessions().len();
        if n > 0 && n <= count {
            self.selected_index = n - 1;
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.visible_sessions().get(self.selected_index).copied()
    }

    /// Groups visible sessions by project. Returns `(group_name, session_indices)` pairs
    /// where indices refer to positions in `visible_sessions()`.
    pub fn grouped_sessions(&self) -> Vec<(String, Vec<usize>)> {
        let visible = self.visible_sessions();
        let mut groups: Vec<(Option<i64>, String, Vec<usize>)> = Vec::new();

        for (i, session) in visible.iter().enumerate() {
            let project_id = session.project_id;
            let group_name = project_id
                .and_then(|pid| self.projects.iter().find(|p| p.id == pid))
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unassigned".to_string());

            if let Some(group) = groups.iter_mut().find(|(gid, _, _)| *gid == project_id) {
                group.2.push(i);
            } else {
                groups.push((project_id, group_name, vec![i]));
            }
        }

        // Sort: assigned projects first (alphabetically), unassigned last
        groups.sort_by(|a, b| match (a.0, b.0) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(_), Some(_)) => a.1.cmp(&b.1),
        });

        groups
            .into_iter()
            .map(|(_, name, idxs)| (name, idxs))
            .collect()
    }

    /// Returns counts of sessions by state: (working, needs_input, done, idle)
    pub fn session_state_counts(&self) -> (usize, usize, usize, usize) {
        let mut working = 0;
        let mut needs_input = 0;
        let mut done = 0;
        let mut idle = 0;
        for s in &self.sessions {
            match s.state {
                SessionState::Working => working += 1,
                SessionState::NeedsInput => needs_input += 1,
                SessionState::Done => done += 1,
                SessionState::Idle => idle += 1,
            }
        }
        (working, needs_input, done, idle)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if self.input_mode == InputMode::Help {
            if key.code == KeyCode::Esc || key.code == KeyCode::Char('?') {
                self.input_mode = InputMode::Normal;
            }
            return AppAction::None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('i') => {
                    self.show_untracked = !self.show_untracked;
                    self.selected_index = 0;
                    AppAction::ToggleUntracked
                }
                _ => AppAction::None,
            };
        }

        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                AppAction::Quit
            }
            KeyCode::Char('?') => {
                self.input_mode = InputMode::Help;
                AppAction::ShowHelp
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                AppAction::None
            }
            KeyCode::Char(c @ '1'..='9') => {
                self.select_by_number((c as u8 - b'0') as usize);
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(session) = self.selected_session() {
                    AppAction::AttachSession(session.pane_id.clone())
                } else {
                    AppAction::None
                }
            }
            _ => AppAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_lib::models::SessionState;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            pane_id: "%0".to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state: SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 0,
            created_at: 0,
            updated_at: 0,
            project_id: None,
            plan_step_id: None,
            host: None,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn test_new_app_defaults() {
        let app = App::new();
        assert!(app.sessions.is_empty());
        assert_eq!(app.selected_index, 0);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_select_next_wraps() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);

        assert_eq!(app.selected_index, 0);
        app.select_next();
        assert_eq!(app.selected_index, 1);
        app.select_next();
        assert_eq!(app.selected_index, 2);
        app.select_next();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_select_prev_wraps() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);

        assert_eq!(app.selected_index, 0);
        app.select_prev();
        assert_eq!(app.selected_index, 2);
    }

    #[test]
    fn test_select_next_empty_list() {
        let mut app = App::new();
        app.select_next();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_select_prev_empty_list() {
        let mut app = App::new();
        app.select_prev();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_q_quits() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
        assert!(matches!(action, AppAction::Quit));
    }

    #[test]
    fn test_jk_navigates() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);

        assert_eq!(app.selected_index, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected_index, 1);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_number_quick_select() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);

        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.selected_index, 2);

        app.handle_key(key(KeyCode::Char('1')));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_number_out_of_range_ignored() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("a"), make_session("b")]);

        app.handle_key(key(KeyCode::Char('5')));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_enter_attaches() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("s1")]);
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::AttachSession(_)));
    }

    #[test]
    fn test_enter_no_session_returns_none() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_question_mark_shows_help() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.input_mode, InputMode::Help);
        assert!(matches!(action, AppAction::ShowHelp));
    }

    #[test]
    fn test_help_mode_esc_returns_to_normal() {
        let mut app = App::new();
        app.input_mode = InputMode::Help;
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_ctrl_i_toggles_untracked() {
        let mut app = App::new();
        assert!(!app.show_untracked);
        let action = app.handle_key(ctrl_key(KeyCode::Char('i')));
        assert!(app.show_untracked);
        assert!(matches!(action, AppAction::ToggleUntracked));
    }

    #[test]
    fn test_update_sessions_clamps_index() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
            make_session("d"),
            make_session("e"),
            make_session("f"),
        ]);
        app.selected_index = 5;

        app.update_sessions(vec![
            make_session("x"),
            make_session("y"),
            make_session("z"),
        ]);
        assert_eq!(app.selected_index, 2);
    }

    #[test]
    fn test_selected_session_returns_correct() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("first"),
            make_session("second"),
            make_session("third"),
        ]);

        assert_eq!(app.selected_session().unwrap().id, "first");
        app.select_next();
        assert_eq!(app.selected_session().unwrap().id, "second");
    }

    #[test]
    fn test_selected_session_empty_returns_none() {
        let app = App::new();
        assert!(app.selected_session().is_none());
    }

    #[test]
    fn test_update_sessions_empty_resets_index() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("a"), make_session("b")]);
        app.selected_index = 1;
        app.update_sessions(vec![]);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_session_state_counts() {
        let mut app = App::new();
        let mut s1 = make_session("a");
        s1.state = SessionState::Working;
        let mut s2 = make_session("b");
        s2.state = SessionState::NeedsInput;
        let mut s3 = make_session("c");
        s3.state = SessionState::Done;
        let s4 = make_session("d"); // Idle

        app.update_sessions(vec![s1, s2, s3, s4]);
        let (w, n, d, i) = app.session_state_counts();
        assert_eq!((w, n, d, i), (1, 1, 1, 1));
    }
}
