use ca_lib::events::Event;
use ca_lib::models::Session;
use crossterm::event::{KeyCode, KeyEvent};

pub enum AppAction {
    None,
    Quit,
    SelectSession(String),
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected_index: usize,
    pub should_quit: bool,
    pub preview_events: Vec<Event>,
    pub connected: bool,
}

impl App {
    pub fn new() -> Self {
        App {
            sessions: Vec::new(),
            selected_index: 0,
            should_quit: false,
            preview_events: Vec::new(),
            connected: false,
        }
    }

    pub fn update_sessions(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;
        if self.sessions.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.sessions.len() {
            self.selected_index = self.sessions.len() - 1;
        }
        self.preview_events.clear();
    }

    pub fn update_preview(&mut self, events: Vec<Event>) {
        self.preview_events = events;
    }

    pub fn clear_preview(&mut self) {
        self.preview_events.clear();
    }

    pub fn select_next(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.sessions.len();
        self.preview_events.clear();
    }

    pub fn select_prev(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.sessions.len() - 1;
        } else {
            self.selected_index -= 1;
        }
        self.preview_events.clear();
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions.get(self.selected_index)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                AppAction::Quit
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(session) = self.selected_session() {
                    AppAction::SelectSession(session.id.clone())
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
    use ca_lib::events::EventType;
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
            state: ca_lib::models::SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
        app.select_next();
        assert_eq!(app.selected_index, 1);
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
    fn test_handle_key_quit() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
        assert!(matches!(action, AppAction::Quit));
    }

    #[test]
    fn test_handle_key_esc_quit() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Esc));
        assert!(app.should_quit);
        assert!(matches!(action, AppAction::Quit));
    }

    #[test]
    fn test_handle_key_j_k_movement() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected_index, 1);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_handle_key_arrow_movement() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("a"), make_session("b")]);

        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected_index, 1);

        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_handle_key_enter_returns_session_id() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("sess-1"), make_session("sess-2")]);
        app.select_next();

        let action = app.handle_key(key(KeyCode::Enter));
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "sess-2"),
            _ => panic!("expected SelectSession"),
        }
    }

    #[test]
    fn test_handle_key_enter_empty_returns_none() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::None));
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
    fn test_unrecognized_key_returns_none() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(action, AppAction::None));
        assert!(!app.should_quit);
    }

    fn make_event(id: i64, event_type: EventType) -> Event {
        Event {
            id,
            session_id: "sess-1".to_string(),
            event_type,
            payload: None,
            timestamp: 1000 + id,
        }
    }

    fn sample_events() -> Vec<Event> {
        vec![
            make_event(1, EventType::SessionDiscovered),
            make_event(2, EventType::StateChanged {
                from: SessionState::Idle,
                to: SessionState::Working,
            }),
        ]
    }

    #[test]
    fn test_preview_events_default_empty() {
        let app = App::new();
        assert!(app.preview_events.is_empty());
    }

    #[test]
    fn test_update_preview_stores_events() {
        let mut app = App::new();
        let events = sample_events();
        app.update_preview(events.clone());
        assert_eq!(app.preview_events.len(), 2);
        assert_eq!(app.preview_events[0].id, 1);
        assert_eq!(app.preview_events[1].id, 2);
    }

    #[test]
    fn test_clear_preview_empties_events() {
        let mut app = App::new();
        app.update_preview(sample_events());
        assert!(!app.preview_events.is_empty());
        app.clear_preview();
        assert!(app.preview_events.is_empty());
    }

    #[test]
    fn test_update_sessions_clears_preview() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("a"), make_session("b")]);
        app.update_preview(sample_events());
        assert!(!app.preview_events.is_empty());

        app.update_sessions(vec![make_session("x")]);
        assert!(app.preview_events.is_empty());
    }

    #[test]
    fn test_select_next_clears_preview() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("a"), make_session("b")]);
        app.update_preview(sample_events());
        assert!(!app.preview_events.is_empty());

        app.select_next();
        assert!(app.preview_events.is_empty());
    }
}
