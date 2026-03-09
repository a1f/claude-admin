use crate::command_palette::CommandPalette;
use crate::form::{FormKind, FormOverlay};
use ca_lib::events::Event;
use ca_lib::models::Session;
use ca_lib::plan::{Plan, Step, StepStatus};
use ca_lib::project::Project;
use ca_lib::workspace::Workspace;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Form,
    #[allow(dead_code)]
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Sessions,
    Projects,
    Plans,
    PlanDetail,
    Orchestrator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchPanel {
    Steps,
    Sessions,
}

#[derive(Debug)]
pub enum AppAction {
    None,
    Quit,
    SelectSession(String),
    LoadProjects,
    LoadPlans(i64),
    LoadPlan(i64),
    CycleStepStatus {
        plan_id: i64,
        step_id: String,
        new_status: StepStatus,
    },
    SpawnStep {
        plan_id: i64,
        step_id: String,
    },
    AttachSession(String),
    ExecuteCommand(String),
    SubmitForm,
    CreateWorkspace {
        path: String,
        name: Option<String>,
    },
    #[allow(dead_code)]
    CreateProject {
        workspace_id: i64,
        name: String,
        description: Option<String>,
    },
    #[allow(dead_code)]
    CreatePlan {
        project_id: i64,
        name: String,
    },
    DeleteWorkspace(i64),
    DeleteProject(i64),
    DeletePlan(i64),
    OpenForm(FormKind),
    LoadWorkspaces,
    ShowHelp,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected_index: usize,
    pub should_quit: bool,
    pub preview_events: Vec<Event>,
    pub connected: bool,
    pub input_mode: InputMode,
    pub view_mode: ViewMode,
    pub workspaces: Vec<Workspace>,
    pub workspace_index: usize,
    pub projects: Vec<Project>,
    pub project_index: usize,
    pub plans: Vec<Plan>,
    pub plan_index: usize,
    pub current_plan: Option<Plan>,
    pub step_index: usize,
    pub orch_panel: OrchPanel,
    pub orch_session_index: usize,
    pub command_palette: CommandPalette,
    pub form_overlay: Option<FormOverlay>,
}

impl App {
    pub fn new() -> Self {
        App {
            sessions: Vec::new(),
            selected_index: 0,
            should_quit: false,
            preview_events: Vec::new(),
            connected: false,
            input_mode: InputMode::Normal,
            view_mode: ViewMode::Sessions,
            workspaces: Vec::new(),
            workspace_index: 0,
            projects: Vec::new(),
            project_index: 0,
            plans: Vec::new(),
            plan_index: 0,
            current_plan: None,
            step_index: 0,
            orch_panel: OrchPanel::Steps,
            orch_session_index: 0,
            command_palette: CommandPalette::new(),
            form_overlay: None,
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

    pub fn update_workspaces(&mut self, workspaces: Vec<Workspace>) {
        self.workspaces = workspaces;
        self.workspace_index = 0;
    }

    pub fn open_form(&mut self, kind: FormKind) {
        let form = match &kind {
            FormKind::CreateWorkspace => FormOverlay::new_workspace(),
            FormKind::CreateProject { workspace_id } => FormOverlay::new_project(*workspace_id),
            FormKind::CreatePlan { project_id } => FormOverlay::new_plan(*project_id),
        };
        self.form_overlay = Some(form);
        self.input_mode = InputMode::Form;
    }

    pub fn update_preview(&mut self, events: Vec<Event>) {
        self.preview_events = events;
    }

    pub fn clear_preview(&mut self) {
        self.preview_events.clear();
    }

    pub fn update_projects(&mut self, projects: Vec<Project>) {
        self.projects = projects;
        self.project_index = 0;
    }

    pub fn update_plans(&mut self, plans: Vec<Plan>) {
        self.plans = plans;
        self.plan_index = 0;
    }

    pub fn update_current_plan(&mut self, plan: Plan) {
        self.current_plan = Some(plan);
        self.step_index = 0;
    }

    /// Returns (phase_idx, step_idx) pairs for all steps in the current plan,
    /// providing a flat index for navigation through the step list.
    pub fn visible_steps(&self) -> Vec<(usize, usize)> {
        let Some(plan) = &self.current_plan else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (pi, phase) in plan.content.phases.iter().enumerate() {
            for (si, _step) in phase.steps.iter().enumerate() {
                pairs.push((pi, si));
            }
        }
        pairs
    }

    /// Returns the phase name and step reference for the currently selected step.
    pub fn selected_step(&self) -> Option<(&str, &Step)> {
        let plan = self.current_plan.as_ref()?;
        let steps = self.visible_steps();
        let &(pi, si) = steps.get(self.step_index)?;
        let phase = &plan.content.phases[pi];
        Some((&phase.name, &phase.steps[si]))
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

    /// Returns sessions linked to the current plan's project.
    pub fn project_sessions(&self) -> Vec<&Session> {
        let Some(plan) = &self.current_plan else {
            return Vec::new();
        };
        self.sessions
            .iter()
            .filter(|s| s.project_id == Some(plan.project_id))
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if self.input_mode != InputMode::Normal {
            return match self.input_mode {
                InputMode::Command => self.handle_command_key(key),
                InputMode::Form => self.handle_form_key(key),
                _ => {
                    if key.code == KeyCode::Esc {
                        self.input_mode = InputMode::Normal;
                    }
                    AppAction::None
                }
            };
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                AppAction::Quit
            }
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.command_palette.open();
                AppAction::None
            }
            _ => match self.view_mode {
                ViewMode::Sessions => self.handle_sessions_key(key.code),
                ViewMode::Projects => self.handle_projects_key(key.code),
                ViewMode::Plans => self.handle_plans_key(key.code),
                ViewMode::PlanDetail => self.handle_plan_detail_key(key.code),
                ViewMode::Orchestrator => self.handle_orchestrator_key(key.code),
            },
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc => {
                self.command_palette.close();
                self.input_mode = InputMode::Normal;
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(cmd) = self.command_palette.submit() {
                    self.input_mode = InputMode::Normal;
                    self.command_palette.message = Some(format!("Executed: {cmd}"));
                    AppAction::ExecuteCommand(cmd)
                } else {
                    self.command_palette.close();
                    self.input_mode = InputMode::Normal;
                    AppAction::None
                }
            }
            KeyCode::Tab => {
                self.command_palette.accept_suggestion();
                AppAction::None
            }
            KeyCode::Up => {
                self.command_palette.select_prev_suggestion();
                AppAction::None
            }
            KeyCode::Down => {
                self.command_palette.select_next_suggestion();
                AppAction::None
            }
            KeyCode::Backspace => {
                self.command_palette.input.backspace();
                self.command_palette.update_suggestions();
                AppAction::None
            }
            KeyCode::Delete => {
                self.command_palette.input.delete_char();
                self.command_palette.update_suggestions();
                AppAction::None
            }
            KeyCode::Left => {
                self.command_palette.input.move_left();
                AppAction::None
            }
            KeyCode::Right => {
                self.command_palette.input.move_right();
                AppAction::None
            }
            KeyCode::Home => {
                self.command_palette.input.move_home();
                AppAction::None
            }
            KeyCode::End => {
                self.command_palette.input.move_end();
                AppAction::None
            }
            KeyCode::Char(c) => {
                self.command_palette.input.insert_char(c);
                self.command_palette.update_suggestions();
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc => {
                self.form_overlay = None;
                self.input_mode = InputMode::Normal;
                AppAction::None
            }
            KeyCode::Tab => {
                if let Some(form) = &mut self.form_overlay {
                    form.focus_next();
                }
                AppAction::None
            }
            KeyCode::BackTab => {
                if let Some(form) = &mut self.form_overlay {
                    form.focus_prev();
                }
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(form) = &self.form_overlay {
                    match form.validate() {
                        Ok(()) => {
                            self.input_mode = InputMode::Normal;
                            AppAction::SubmitForm
                        }
                        Err(msg) => {
                            if let Some(form) = &mut self.form_overlay {
                                form.error_message = Some(msg);
                            }
                            AppAction::None
                        }
                    }
                } else {
                    AppAction::None
                }
            }
            KeyCode::Backspace => {
                if let Some(form) = &mut self.form_overlay {
                    if let Some(input) = form.focused_input() {
                        input.backspace();
                    }
                }
                AppAction::None
            }
            KeyCode::Delete => {
                if let Some(form) = &mut self.form_overlay {
                    if let Some(input) = form.focused_input() {
                        input.delete_char();
                    }
                }
                AppAction::None
            }
            KeyCode::Left => {
                if let Some(form) = &mut self.form_overlay {
                    if let Some(input) = form.focused_input() {
                        input.move_left();
                    }
                }
                AppAction::None
            }
            KeyCode::Right => {
                if let Some(form) = &mut self.form_overlay {
                    if let Some(input) = form.focused_input() {
                        input.move_right();
                    }
                }
                AppAction::None
            }
            KeyCode::Char(c) => {
                if let Some(form) = &mut self.form_overlay {
                    if let Some(input) = form.focused_input() {
                        input.insert_char(c);
                    }
                }
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_sessions_key(&mut self, code: KeyCode) -> AppAction {
        match code {
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
            KeyCode::Char('p') => {
                self.view_mode = ViewMode::Projects;
                AppAction::LoadProjects
            }
            _ => AppAction::None,
        }
    }

    fn handle_projects_key(&mut self, code: KeyCode) -> AppAction {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.projects.is_empty() {
                    self.project_index = (self.project_index + 1) % self.projects.len();
                }
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.projects.is_empty() {
                    if self.project_index == 0 {
                        self.project_index = self.projects.len() - 1;
                    } else {
                        self.project_index -= 1;
                    }
                }
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(project) = self.projects.get(self.project_index) {
                    self.view_mode = ViewMode::Plans;
                    AppAction::LoadPlans(project.id)
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('b') => {
                self.view_mode = ViewMode::Sessions;
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_plans_key(&mut self, code: KeyCode) -> AppAction {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.plans.is_empty() {
                    self.plan_index = (self.plan_index + 1) % self.plans.len();
                }
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.plans.is_empty() {
                    if self.plan_index == 0 {
                        self.plan_index = self.plans.len() - 1;
                    } else {
                        self.plan_index -= 1;
                    }
                }
                AppAction::None
            }
            KeyCode::Enter => {
                if let Some(plan) = self.plans.get(self.plan_index) {
                    self.view_mode = ViewMode::PlanDetail;
                    AppAction::LoadPlan(plan.id)
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('b') => {
                self.view_mode = ViewMode::Projects;
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_plan_detail_key(&mut self, code: KeyCode) -> AppAction {
        let step_count = self.visible_steps().len();
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if step_count > 0 {
                    self.step_index = (self.step_index + 1) % step_count;
                }
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if step_count > 0 {
                    if self.step_index == 0 {
                        self.step_index = step_count - 1;
                    } else {
                        self.step_index -= 1;
                    }
                }
                AppAction::None
            }
            KeyCode::Char('s') => {
                if let Some(plan) = &self.current_plan {
                    let steps = self.visible_steps();
                    if let Some(&(pi, si)) = steps.get(self.step_index) {
                        let step = &plan.content.phases[pi].steps[si];
                        let new_status = cycle_step_status(step.status);
                        return AppAction::CycleStepStatus {
                            plan_id: plan.id,
                            step_id: step.id.clone(),
                            new_status,
                        };
                    }
                }
                AppAction::None
            }
            KeyCode::Char('o') => {
                self.view_mode = ViewMode::Orchestrator;
                self.orch_panel = OrchPanel::Steps;
                self.orch_session_index = 0;
                AppAction::None
            }
            KeyCode::Char('b') => {
                self.view_mode = ViewMode::Plans;
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_orchestrator_key(&mut self, code: KeyCode) -> AppAction {
        match code {
            KeyCode::Tab => {
                self.orch_panel = match self.orch_panel {
                    OrchPanel::Steps => OrchPanel::Sessions,
                    OrchPanel::Sessions => OrchPanel::Steps,
                };
                AppAction::None
            }
            KeyCode::Char('b') => {
                self.view_mode = ViewMode::PlanDetail;
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.orch_navigate(1);
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.orch_navigate(-1);
                AppAction::None
            }
            KeyCode::Char('s') => {
                if let (Some(plan), Some((_, step))) = (&self.current_plan, self.selected_step()) {
                    AppAction::SpawnStep {
                        plan_id: plan.id,
                        step_id: step.id.clone(),
                    }
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('a') => {
                let sessions = self.project_sessions();
                if let Some(session) = sessions.get(self.orch_session_index) {
                    AppAction::AttachSession(session.pane_id.clone())
                } else {
                    AppAction::None
                }
            }
            _ => AppAction::None,
        }
    }

    fn orch_navigate(&mut self, direction: isize) {
        match self.orch_panel {
            OrchPanel::Steps => {
                let total = self.visible_steps().len();
                if total > 0 {
                    self.step_index = wrap_index(self.step_index, total, direction);
                }
            }
            OrchPanel::Sessions => {
                let total = self.project_sessions().len();
                if total > 0 {
                    self.orch_session_index = wrap_index(self.orch_session_index, total, direction);
                }
            }
        }
    }
}

fn wrap_index(current: usize, total: usize, direction: isize) -> usize {
    if direction > 0 {
        (current + 1) % total
    } else if current == 0 {
        total - 1
    } else {
        current - 1
    }
}

pub fn cycle_step_status(status: StepStatus) -> StepStatus {
    match status {
        StepStatus::Pending => StepStatus::InProgress,
        StepStatus::InProgress => StepStatus::Completed,
        StepStatus::Completed => StepStatus::Blocked,
        StepStatus::Blocked => StepStatus::Skipped,
        StepStatus::Skipped => StepStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_lib::events::EventType;
    use ca_lib::models::SessionState;
    use ca_lib::plan::{ExitCriteria, Phase, PlanContent, PlanStatus};
    use ca_lib::project::ProjectStatus;
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
        }
    }

    fn make_project(id: i64, name: &str) -> Project {
        Project {
            id,
            workspace_id: 1,
            name: name.to_string(),
            description: None,
            status: ProjectStatus::Active,
            worktree_path: None,
            branch_name: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_plan(id: i64, name: &str) -> Plan {
        Plan {
            id,
            project_id: 1,
            name: name.to_string(),
            content: PlanContent {
                phases: vec![
                    Phase {
                        name: "Setup".to_string(),
                        steps: vec![Step {
                            id: "0.1".to_string(),
                            description: "Init".to_string(),
                            status: StepStatus::Completed,
                            exit_criteria: ExitCriteria {
                                description: "Compiles".to_string(),
                                commands: vec!["cargo build".to_string()],
                            },
                        }],
                    },
                    Phase {
                        name: "Core".to_string(),
                        steps: vec![
                            Step {
                                id: "1.1".to_string(),
                                description: "Add models".to_string(),
                                status: StepStatus::Pending,
                                exit_criteria: ExitCriteria {
                                    description: "Tests pass".to_string(),
                                    commands: vec![],
                                },
                            },
                            Step {
                                id: "1.2".to_string(),
                                description: "Add API".to_string(),
                                status: StepStatus::InProgress,
                                exit_criteria: ExitCriteria {
                                    description: "Endpoints work".to_string(),
                                    commands: vec!["cargo test".to_string()],
                                },
                            },
                        ],
                    },
                ],
            },
            status: PlanStatus::Active,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    // -- Original session tests (preserved) --

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
            make_event(
                2,
                EventType::StateChanged {
                    from: SessionState::Idle,
                    to: SessionState::Working,
                },
            ),
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

    // -- New plan viewer tests --

    #[test]
    fn test_view_mode_default_sessions() {
        let app = App::new();
        assert_eq!(app.view_mode, ViewMode::Sessions);
    }

    #[test]
    fn test_handle_key_p_enters_projects() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('p')));
        assert_eq!(app.view_mode, ViewMode::Projects);
        assert!(matches!(action, AppAction::LoadProjects));
    }

    #[test]
    fn test_handle_key_b_returns_to_sessions_from_projects() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        let action = app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Sessions);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_handle_key_enter_in_projects_loads_plans() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![make_project(42, "MyProject")]);

        let action = app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.view_mode, ViewMode::Plans);
        match action {
            AppAction::LoadPlans(id) => assert_eq!(id, 42),
            _ => panic!("expected LoadPlans"),
        }
    }

    #[test]
    fn test_handle_key_b_returns_to_projects_from_plans() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        let action = app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Projects);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_handle_key_enter_in_plans_loads_detail() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        app.update_plans(vec![make_plan(7, "The Plan")]);

        let action = app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.view_mode, ViewMode::PlanDetail);
        match action {
            AppAction::LoadPlan(id) => assert_eq!(id, 7),
            _ => panic!("expected LoadPlan"),
        }
    }

    #[test]
    fn test_handle_key_b_returns_to_plans_from_detail() {
        let mut app = App::new();
        app.view_mode = ViewMode::PlanDetail;
        let action = app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::Plans);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_handle_key_s_cycles_status() {
        let mut app = App::new();
        app.view_mode = ViewMode::PlanDetail;
        app.update_current_plan(make_plan(7, "The Plan"));

        let action = app.handle_key(key(KeyCode::Char('s')));
        match action {
            AppAction::CycleStepStatus {
                plan_id,
                step_id,
                new_status,
            } => {
                assert_eq!(plan_id, 7);
                // First step is "0.1" with status Completed -> cycles to Blocked
                assert_eq!(step_id, "0.1");
                assert_eq!(new_status, StepStatus::Blocked);
            }
            _ => panic!("expected CycleStepStatus"),
        }
    }

    #[test]
    fn test_cycle_step_status_order() {
        assert_eq!(
            cycle_step_status(StepStatus::Pending),
            StepStatus::InProgress
        );
        assert_eq!(
            cycle_step_status(StepStatus::InProgress),
            StepStatus::Completed
        );
        assert_eq!(
            cycle_step_status(StepStatus::Completed),
            StepStatus::Blocked
        );
        assert_eq!(cycle_step_status(StepStatus::Blocked), StepStatus::Skipped);
        assert_eq!(cycle_step_status(StepStatus::Skipped), StepStatus::Pending);
    }

    #[test]
    fn test_update_projects_resets_index() {
        let mut app = App::new();
        app.project_index = 5;
        app.update_projects(vec![make_project(1, "A"), make_project(2, "B")]);
        assert_eq!(app.project_index, 0);
        assert_eq!(app.projects.len(), 2);
    }

    #[test]
    fn test_update_plans_resets_index() {
        let mut app = App::new();
        app.plan_index = 3;
        app.update_plans(vec![make_plan(1, "Plan A")]);
        assert_eq!(app.plan_index, 0);
        assert_eq!(app.plans.len(), 1);
    }

    #[test]
    fn test_visible_steps() {
        let mut app = App::new();
        app.update_current_plan(make_plan(1, "Test"));

        let steps = app.visible_steps();
        // Phase "Setup" has 1 step, Phase "Core" has 2 steps = 3 total
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], (0, 0)); // Setup, step 0
        assert_eq!(steps[1], (1, 0)); // Core, step 0
        assert_eq!(steps[2], (1, 1)); // Core, step 1
    }

    #[test]
    fn test_visible_steps_no_plan() {
        let app = App::new();
        assert!(app.visible_steps().is_empty());
    }

    #[test]
    fn test_selected_step() {
        let mut app = App::new();
        app.update_current_plan(make_plan(1, "Test"));

        let (phase_name, step) = app.selected_step().unwrap();
        assert_eq!(phase_name, "Setup");
        assert_eq!(step.id, "0.1");

        app.step_index = 2;
        let (phase_name, step) = app.selected_step().unwrap();
        assert_eq!(phase_name, "Core");
        assert_eq!(step.id, "1.2");
    }

    #[test]
    fn test_selected_step_no_plan() {
        let app = App::new();
        assert!(app.selected_step().is_none());
    }

    #[test]
    fn test_jk_navigation_in_plan_detail() {
        let mut app = App::new();
        app.view_mode = ViewMode::PlanDetail;
        app.update_current_plan(make_plan(1, "Nav Test"));

        assert_eq!(app.step_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 1);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 2);

        // Wraps around
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 0);

        // k wraps the other way
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.step_index, 2);
    }

    #[test]
    fn test_quit_from_any_view() {
        for mode in [
            ViewMode::Sessions,
            ViewMode::Projects,
            ViewMode::Plans,
            ViewMode::PlanDetail,
            ViewMode::Orchestrator,
        ] {
            let mut app = App::new();
            app.view_mode = mode;
            let action = app.handle_key(key(KeyCode::Char('q')));
            assert!(app.should_quit, "q should quit from {:?}", mode);
            assert!(matches!(action, AppAction::Quit));
        }
    }

    #[test]
    fn test_jk_navigation_in_projects() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![
            make_project(1, "A"),
            make_project(2, "B"),
            make_project(3, "C"),
        ]);

        assert_eq!(app.project_index, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.project_index, 1);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.project_index, 0);
        // Wrap backwards
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.project_index, 2);
    }

    #[test]
    fn test_jk_navigation_in_plans() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        app.update_plans(vec![make_plan(1, "A"), make_plan(2, "B")]);

        assert_eq!(app.plan_index, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.plan_index, 1);
        // Wrap forward
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.plan_index, 0);
    }

    // -- Orchestrator tests --

    fn make_session_with_project(
        id: &str,
        project_id: Option<i64>,
        step_id: Option<&str>,
    ) -> Session {
        Session {
            id: id.to_string(),
            pane_id: format!("%{id}"),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state: SessionState::Working,
            detection_method: "process_name".to_string(),
            last_activity: 0,
            created_at: 0,
            updated_at: 0,
            project_id,
            plan_step_id: step_id.map(String::from),
        }
    }

    #[test]
    fn test_view_mode_orchestrator_from_plan_detail() {
        let mut app = App::new();
        app.view_mode = ViewMode::PlanDetail;
        app.update_current_plan(make_plan(1, "Test"));

        let action = app.handle_key(key(KeyCode::Char('o')));
        assert_eq!(app.view_mode, ViewMode::Orchestrator);
        assert_eq!(app.orch_panel, OrchPanel::Steps);
        assert_eq!(app.orch_session_index, 0);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_orchestrator_tab_switches_panel() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;
        assert_eq!(app.orch_panel, OrchPanel::Steps);

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.orch_panel, OrchPanel::Sessions);

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.orch_panel, OrchPanel::Steps);
    }

    #[test]
    fn test_orchestrator_b_returns_to_plan_detail() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;

        let action = app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.view_mode, ViewMode::PlanDetail);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_orchestrator_jk_steps_panel() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;
        app.orch_panel = OrchPanel::Steps;
        app.update_current_plan(make_plan(1, "Nav"));

        assert_eq!(app.step_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 1);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 2);

        // Wrap
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.step_index, 0);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.step_index, 2);
    }

    #[test]
    fn test_orchestrator_jk_sessions_panel() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;
        app.orch_panel = OrchPanel::Sessions;
        // Plan with project_id=1
        app.update_current_plan(make_plan(1, "Test"));
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), None),
            make_session_with_project("s2", Some(1), None),
            make_session_with_project("s3", Some(1), None),
        ]);

        assert_eq!(app.orch_session_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.orch_session_index, 1);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.orch_session_index, 2);

        // Wrap
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.orch_session_index, 0);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.orch_session_index, 2);
    }

    #[test]
    fn test_orchestrator_s_returns_spawn_step() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;
        app.update_current_plan(make_plan(7, "Spawn Test"));
        app.step_index = 1; // step "1.1"

        let action = app.handle_key(key(KeyCode::Char('s')));
        match action {
            AppAction::SpawnStep { plan_id, step_id } => {
                assert_eq!(plan_id, 7);
                assert_eq!(step_id, "1.1");
            }
            _ => panic!("expected SpawnStep"),
        }
    }

    #[test]
    fn test_orchestrator_a_returns_attach_session() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;
        app.update_current_plan(make_plan(1, "Attach Test"));
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), Some("0.1")),
            make_session_with_project("s2", Some(1), Some("1.1")),
        ]);
        app.orch_session_index = 1;

        let action = app.handle_key(key(KeyCode::Char('a')));
        match action {
            AppAction::AttachSession(pane_id) => assert_eq!(pane_id, "%s2"),
            _ => panic!("expected AttachSession"),
        }
    }

    #[test]
    fn test_project_sessions_filters_correctly() {
        let mut app = App::new();
        app.update_current_plan(make_plan(1, "Filter Test"));
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), Some("0.1")),
            make_session_with_project("s2", Some(2), None),
            make_session_with_project("s3", Some(1), Some("1.1")),
            make_session("s4"), // project_id = None
        ]);

        let filtered = app.project_sessions();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, "s1");
        assert_eq!(filtered[1].id, "s3");
    }

    #[test]
    fn test_project_sessions_empty_when_no_plan() {
        let mut app = App::new();
        app.update_sessions(vec![make_session_with_project("s1", Some(1), None)]);

        let filtered = app.project_sessions();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_orchestrator_quit_works() {
        let mut app = App::new();
        app.view_mode = ViewMode::Orchestrator;

        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
        assert!(matches!(action, AppAction::Quit));
    }

    // -- InputMode gating tests --

    #[test]
    fn test_input_mode_default_normal() {
        let app = App::new();
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_q_in_normal_mode_quits() {
        let mut app = App::new();
        app.input_mode = InputMode::Normal;
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
        assert!(matches!(action, AppAction::Quit));
    }

    #[test]
    fn test_q_in_command_mode_does_not_quit() {
        let mut app = App::new();
        app.input_mode = InputMode::Command;
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_q_in_form_mode_does_not_quit() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_esc_in_command_mode_returns_to_normal() {
        let mut app = App::new();
        app.input_mode = InputMode::Command;
        let action = app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_esc_in_help_mode_returns_to_normal() {
        let mut app = App::new();
        app.input_mode = InputMode::Help;
        let action = app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_keys_ignored_in_non_normal_mode() {
        let mut app = App::new();
        app.input_mode = InputMode::Command;
        app.update_sessions(vec![make_session("a"), make_session("b")]);
        let action = app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected_index, 0);
        assert!(matches!(action, AppAction::None));
    }

    // -- Command palette tests --

    #[test]
    fn test_colon_opens_command_palette() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char(':')));
        assert_eq!(app.input_mode, InputMode::Command);
        assert!(app.command_palette.visible);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_command_mode_typing() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char(':')));
        app.handle_key(key(KeyCode::Char('h')));
        app.handle_key(key(KeyCode::Char('i')));
        assert_eq!(app.command_palette.input.value(), "hi");
    }

    #[test]
    fn test_command_mode_enter_executes() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char(':')));
        app.handle_key(key(KeyCode::Char('h')));
        app.handle_key(key(KeyCode::Char('i')));
        let action = app.handle_key(key(KeyCode::Enter));
        match action {
            AppAction::ExecuteCommand(cmd) => assert_eq!(cmd, "hi"),
            _ => panic!("expected ExecuteCommand"),
        }
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_command_mode_esc_closes() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char(':')));
        app.handle_key(key(KeyCode::Char('h')));
        let action = app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.command_palette.visible);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_command_mode_empty_enter_closes() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char(':')));
        let action = app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_command_mode_backspace() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char(':')));
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Char('b')));
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.command_palette.input.value(), "a");
    }

    // -- Form overlay tests --

    #[test]
    fn test_form_mode_esc_cancels() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        app.form_overlay = Some(FormOverlay::new_workspace());
        let action = app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.form_overlay.is_none());
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_form_mode_tab_cycles_fields() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        app.form_overlay = Some(FormOverlay::new_workspace());
        assert_eq!(app.form_overlay.as_ref().unwrap().focused_field, 0);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.form_overlay.as_ref().unwrap().focused_field, 1);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.form_overlay.as_ref().unwrap().focused_field, 0);
    }

    #[test]
    fn test_form_mode_submit_empty_required_shows_error() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        app.form_overlay = Some(FormOverlay::new_workspace());
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::None));
        assert_eq!(app.input_mode, InputMode::Form);
        assert!(app.form_overlay.as_ref().unwrap().error_message.is_some());
    }

    #[test]
    fn test_form_mode_submit_valid_returns_submit_form() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        let mut form = FormOverlay::new_workspace();
        form.fields[0].input.set_value("/home/user/project");
        app.form_overlay = Some(form);
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::SubmitForm));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_form_mode_typing_goes_to_focused_field() {
        let mut app = App::new();
        app.input_mode = InputMode::Form;
        app.form_overlay = Some(FormOverlay::new_workspace());
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(
            app.form_overlay.as_ref().unwrap().fields[0].input.value(),
            "/d"
        );
    }

    // -- open_form tests --

    #[test]
    fn test_open_form_sets_input_mode() {
        let mut app = App::new();
        app.open_form(FormKind::CreateWorkspace);
        assert_eq!(app.input_mode, InputMode::Form);
        assert!(app.form_overlay.is_some());
        assert_eq!(
            app.form_overlay.as_ref().unwrap().kind,
            FormKind::CreateWorkspace
        );
    }

    #[test]
    fn test_open_form_project() {
        let mut app = App::new();
        app.open_form(FormKind::CreateProject { workspace_id: 42 });
        assert_eq!(app.input_mode, InputMode::Form);
        let form = app.form_overlay.as_ref().unwrap();
        assert_eq!(form.kind, FormKind::CreateProject { workspace_id: 42 });
    }

    #[test]
    fn test_open_form_plan() {
        let mut app = App::new();
        app.open_form(FormKind::CreatePlan { project_id: 7 });
        assert_eq!(app.input_mode, InputMode::Form);
        let form = app.form_overlay.as_ref().unwrap();
        assert_eq!(form.kind, FormKind::CreatePlan { project_id: 7 });
    }

    #[test]
    fn test_update_workspaces() {
        let mut app = App::new();
        app.workspace_index = 5;
        let ws = vec![
            Workspace {
                id: 1,
                name: "ws1".to_string(),
                path: "/a".to_string(),
                created_at: 0,
                updated_at: 0,
            },
            Workspace {
                id: 2,
                name: "ws2".to_string(),
                path: "/b".to_string(),
                created_at: 0,
                updated_at: 0,
            },
        ];
        app.update_workspaces(ws);
        assert_eq!(app.workspace_index, 0);
        assert_eq!(app.workspaces.len(), 2);
    }
}
