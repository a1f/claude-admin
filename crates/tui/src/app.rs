use crate::command_palette::CommandPalette;
use crate::form::{FormKind, FormOverlay};
use ca_lib::events::Event;
use ca_lib::git_ops::DiffFile;
use ca_lib::models::{Session, SessionState};
use ca_lib::plan::{Plan, Step, StepStatus};
use ca_lib::project::Project;
use ca_lib::review::Review;
use ca_lib::workspace::Workspace;
use crossterm::event::{KeyCode, KeyEvent};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Form,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Sessions,
    Projects,
    Plans,
    PlanDetail,
    Orchestrator,
    Review,
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
    ToggleUntracked,
    AssignSessionToProject {
        session_id: String,
        project_id: i64,
    },
    #[allow(dead_code)]
    LoadReview(i64),
    #[allow(dead_code)]
    LoadReviewDiff,
    AddReviewComment {
        review_id: i64,
        file_path: String,
        line_number: u32,
        body: String,
    },
    OpenVimdiff {
        base_commit: String,
        head_commit: String,
        file_path: String,
    },
    OpenDelta {
        base_commit: String,
        head_commit: String,
        file_path: String,
    },
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
    pub confirm_action: Option<Box<AppAction>>,
    pub show_untracked: bool,
    pub project_picker: Option<Vec<Project>>,
    pub picker_index: usize,
    pub status_message: Option<(String, Instant)>,
    pub tick_count: u64,
    pub review: Option<Review>,
    pub review_diff_files: Vec<DiffFile>,
    pub review_file_index: usize,
    pub review_scroll: u16,
    pub review_comment_mode: bool,
    pub review_comment_input: crate::input::TextInput,
    pub review_comment_line: Option<u32>,
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
            confirm_action: None,
            show_untracked: false,
            project_picker: None,
            picker_index: 0,
            status_message: None,
            tick_count: 0,
            review: None,
            review_diff_files: Vec::new(),
            review_file_index: 0,
            review_scroll: 0,
            review_comment_mode: false,
            review_comment_input: crate::input::TextInput::new("Comment"),
            review_comment_line: None,
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

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
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
        self.preview_events.clear();
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
        self.preview_events.clear();
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.visible_sessions().get(self.selected_index).copied()
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
                InputMode::Help => {
                    if key.code == KeyCode::Esc || key.code == KeyCode::Char('?') {
                        self.input_mode = InputMode::Normal;
                    }
                    AppAction::None
                }
                _ => {
                    if key.code == KeyCode::Esc {
                        self.input_mode = InputMode::Normal;
                    }
                    AppAction::None
                }
            };
        }

        // Handle pending confirmation (y accepts, anything else cancels)
        if self.confirm_action.is_some() {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let action = self.confirm_action.take().unwrap();
                    self.command_palette.message = None;
                    *action
                }
                _ => {
                    self.confirm_action = None;
                    self.command_palette.message = None;
                    AppAction::None
                }
            };
        }

        if self.project_picker.is_some() {
            return self.handle_picker_key(key);
        }

        // Comment mode intercepts all keys before global handlers
        if self.review_comment_mode {
            return self.handle_review_comment_key(key);
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
            KeyCode::Char('?') => {
                self.input_mode = InputMode::Help;
                AppAction::None
            }
            _ => match self.view_mode {
                ViewMode::Sessions => self.handle_sessions_key(key.code),
                ViewMode::Projects => self.handle_projects_key(key.code),
                ViewMode::Plans => self.handle_plans_key(key.code),
                ViewMode::PlanDetail => self.handle_plan_detail_key(key.code),
                ViewMode::Orchestrator => self.handle_orchestrator_key(key.code),
                ViewMode::Review => self.handle_review_key(key),
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

    pub fn next_needs_input(&mut self) -> AppAction {
        let visible = self.visible_sessions();
        if visible.is_empty() {
            return AppAction::None;
        }
        let count = visible.len();
        for i in 1..=count {
            let idx = (self.selected_index + i) % count;
            if visible[idx].state == SessionState::NeedsInput {
                let id = visible[idx].id.clone();
                self.selected_index = idx;
                self.preview_events.clear();
                return AppAction::SelectSession(id);
            }
        }
        AppAction::None
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
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                let count = self.visible_sessions().len();
                if idx < count {
                    self.selected_index = idx;
                    self.preview_events.clear();
                    if let Some(session) = self.visible_sessions().get(idx) {
                        return AppAction::SelectSession(session.id.clone());
                    }
                }
                AppAction::None
            }
            KeyCode::Tab | KeyCode::Char('n') => self.next_needs_input(),
            KeyCode::Enter => {
                if let Some(session) = self.selected_session() {
                    AppAction::SelectSession(session.id.clone())
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('i') => {
                self.show_untracked = !self.show_untracked;
                self.selected_index = 0;
                AppAction::ToggleUntracked
            }
            KeyCode::Char('p') => {
                if self.show_untracked && self.selected_session().is_some() {
                    self.project_picker = Some(self.projects.clone());
                    self.picker_index = 0;
                    AppAction::LoadProjects
                } else {
                    self.view_mode = ViewMode::Projects;
                    AppAction::LoadProjects
                }
            }
            KeyCode::Char('N') => AppAction::OpenForm(FormKind::CreateWorkspace),
            _ => AppAction::None,
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc => {
                self.project_picker = None;
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(projects) = &self.project_picker {
                    if !projects.is_empty() {
                        self.picker_index = (self.picker_index + 1) % projects.len();
                    }
                }
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(projects) = &self.project_picker {
                    if !projects.is_empty() {
                        if self.picker_index == 0 {
                            self.picker_index = projects.len() - 1;
                        } else {
                            self.picker_index -= 1;
                        }
                    }
                }
                AppAction::None
            }
            KeyCode::Enter => {
                let session_id = self
                    .visible_sessions()
                    .get(self.selected_index)
                    .map(|s| s.id.clone());
                let project_id = self
                    .project_picker
                    .as_ref()
                    .and_then(|projects| projects.get(self.picker_index))
                    .map(|p| p.id);
                self.project_picker = None;

                if let (Some(sid), Some(pid)) = (session_id, project_id) {
                    AppAction::AssignSessionToProject {
                        session_id: sid,
                        project_id: pid,
                    }
                } else {
                    AppAction::None
                }
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
            KeyCode::Char('n') => {
                if let Some(ws) = self.workspaces.get(self.workspace_index) {
                    AppAction::OpenForm(FormKind::CreateProject {
                        workspace_id: ws.id,
                    })
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('d') => {
                if let Some(project) = self.projects.get(self.project_index) {
                    self.confirm_action = Some(Box::new(AppAction::DeleteProject(project.id)));
                    self.command_palette.message =
                        Some(format!("Delete project '{}'? (y/n)", project.name));
                }
                AppAction::None
            }
            KeyCode::Char('N') => AppAction::OpenForm(FormKind::CreateWorkspace),
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
            KeyCode::Char('n') => {
                if let Some(project) = self.projects.get(self.project_index) {
                    AppAction::OpenForm(FormKind::CreatePlan {
                        project_id: project.id,
                    })
                } else {
                    AppAction::None
                }
            }
            KeyCode::Char('d') => {
                if let Some(plan) = self.plans.get(self.plan_index) {
                    self.confirm_action = Some(Box::new(AppAction::DeletePlan(plan.id)));
                    self.command_palette.message =
                        Some(format!("Delete plan '{}'? (y/n)", plan.name));
                }
                AppAction::None
            }
            KeyCode::Char('N') => AppAction::OpenForm(FormKind::CreateWorkspace),
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

    fn handle_review_key(&mut self, key: KeyEvent) -> AppAction {
        if self.review_comment_mode {
            return self.handle_review_comment_key(key);
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.review_scroll = self.review_scroll.saturating_add(1);
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.review_scroll = self.review_scroll.saturating_sub(1);
                AppAction::None
            }
            KeyCode::Char('n') => {
                self.review_scroll = self.next_hunk_scroll();
                AppAction::None
            }
            KeyCode::Char('p') => {
                self.review_scroll = self.prev_hunk_scroll();
                AppAction::None
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if self.review_file_index > 0 {
                    self.review_file_index -= 1;
                    self.review_scroll = 0;
                }
                AppAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if !self.review_diff_files.is_empty()
                    && self.review_file_index < self.review_diff_files.len() - 1
                {
                    self.review_file_index += 1;
                    self.review_scroll = 0;
                }
                AppAction::None
            }
            KeyCode::Char('c') => {
                self.review_comment_mode = true;
                self.review_comment_line = Some(self.review_scroll as u32);
                self.review_comment_input.clear();
                AppAction::None
            }
            KeyCode::Char('v') => {
                self.build_external_diff_action(|b, h, f| AppAction::OpenVimdiff {
                    base_commit: b,
                    head_commit: h,
                    file_path: f,
                })
            }
            KeyCode::Char('d') => self.build_external_diff_action(|b, h, f| AppAction::OpenDelta {
                base_commit: b,
                head_commit: h,
                file_path: f,
            }),
            KeyCode::Char('b') => {
                self.view_mode = ViewMode::PlanDetail;
                self.review = None;
                self.review_diff_files.clear();
                self.review_file_index = 0;
                self.review_scroll = 0;
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    /// Builds an OpenVimdiff or OpenDelta action from the current review state.
    /// Returns AppAction::None if no review is loaded or no files are available.
    fn build_external_diff_action(
        &self,
        build: impl FnOnce(String, String, String) -> AppAction,
    ) -> AppAction {
        let Some(review) = &self.review else {
            return AppAction::None;
        };
        let Some(file) = self.review_diff_files.get(self.review_file_index) else {
            return AppAction::None;
        };
        build(
            review.base_commit.clone(),
            review.head_commit.clone(),
            file.new_path.clone(),
        )
    }

    fn handle_review_comment_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc => {
                self.review_comment_mode = false;
                self.review_comment_line = None;
                self.review_comment_input.clear();
                AppAction::None
            }
            KeyCode::Enter => {
                let body = self.review_comment_input.value().to_string();
                if body.is_empty() {
                    self.review_comment_mode = false;
                    self.review_comment_line = None;
                    return AppAction::None;
                }

                let review_id = self.review.as_ref().map(|r| r.id).unwrap_or(0);
                let file_path = self
                    .review_diff_files
                    .get(self.review_file_index)
                    .map(|f| f.new_path.clone())
                    .unwrap_or_default();
                let line_number = self.review_comment_line.unwrap_or(0);

                self.review_comment_mode = false;
                self.review_comment_line = None;
                self.review_comment_input.clear();

                AppAction::AddReviewComment {
                    review_id,
                    file_path,
                    line_number,
                    body,
                }
            }
            KeyCode::Backspace => {
                self.review_comment_input.backspace();
                AppAction::None
            }
            KeyCode::Delete => {
                self.review_comment_input.delete_char();
                AppAction::None
            }
            KeyCode::Char(c) => {
                self.review_comment_input.insert_char(c);
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    /// Find the scroll position of the next hunk header in the current file.
    fn next_hunk_scroll(&self) -> u16 {
        let positions = self.hunk_header_positions();
        for &pos in &positions {
            if pos > self.review_scroll {
                return pos;
            }
        }
        self.review_scroll
    }

    /// Find the scroll position of the previous hunk header in the current file.
    fn prev_hunk_scroll(&self) -> u16 {
        let positions = self.hunk_header_positions();
        for &pos in positions.iter().rev() {
            if pos < self.review_scroll {
                return pos;
            }
        }
        self.review_scroll
    }

    /// Compute the line positions (as scroll offsets) of each hunk header
    /// for the currently selected diff file.
    fn hunk_header_positions(&self) -> Vec<u16> {
        let Some(file) = self.review_diff_files.get(self.review_file_index) else {
            return Vec::new();
        };
        let mut positions = Vec::new();
        let mut line_offset: u16 = 0;
        for hunk in &file.hunks {
            positions.push(line_offset);
            // +1 for the hunk header line itself
            line_offset += 1 + hunk.lines.len() as u16;
        }
        positions
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

    // -- Help mode tests --

    #[test]
    fn test_question_mark_opens_help() {
        let mut app = App::new();
        let action = app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.input_mode, InputMode::Help);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_question_mark_in_help_closes() {
        let mut app = App::new();
        app.input_mode = InputMode::Help;
        let action = app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_esc_in_help_closes() {
        let mut app = App::new();
        app.input_mode = InputMode::Help;
        let action = app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
    }

    // -- Inline CRUD keybinding tests --

    fn make_workspace(id: i64, name: &str) -> Workspace {
        Workspace {
            id,
            name: name.to_string(),
            path: format!("/home/user/{name}"),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_n_in_projects_opens_form() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_workspaces(vec![make_workspace(1, "test")]);
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(
            action,
            AppAction::OpenForm(FormKind::CreateProject { workspace_id: 1 })
        ));
    }

    #[test]
    fn test_n_in_projects_no_workspace_is_noop() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_d_in_projects_sets_confirmation() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![make_project(1, "MyProject")]);
        let action = app.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_some());
        assert!(app.command_palette.message.is_some());
    }

    #[test]
    fn test_d_on_empty_projects_is_noop() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        let action = app.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_none());
    }

    #[test]
    fn test_confirm_y_executes_delete() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![make_project(42, "ToDelete")]);
        app.handle_key(key(KeyCode::Char('d')));
        let action = app.handle_key(key(KeyCode::Char('y')));
        match action {
            AppAction::DeleteProject(id) => assert_eq!(id, 42),
            _ => panic!("expected DeleteProject, got {:?}", action),
        }
        assert!(app.confirm_action.is_none());
    }

    #[test]
    fn test_confirm_n_cancels_delete() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![make_project(42, "ToDelete")]);
        app.handle_key(key(KeyCode::Char('d')));
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_none());
        assert!(app.command_palette.message.is_none());
    }

    #[test]
    fn test_big_n_in_any_view_creates_workspace() {
        for mode in [ViewMode::Sessions, ViewMode::Projects, ViewMode::Plans] {
            let mut app = App::new();
            app.view_mode = mode;
            let action = app.handle_key(key(KeyCode::Char('N')));
            assert!(
                matches!(action, AppAction::OpenForm(FormKind::CreateWorkspace)),
                "N should open workspace form in {:?}",
                mode
            );
        }
    }

    #[test]
    fn test_n_in_plans_opens_plan_form() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        app.update_projects(vec![make_project(7, "TestProj")]);
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(
            action,
            AppAction::OpenForm(FormKind::CreatePlan { project_id: 7 })
        ));
    }

    #[test]
    fn test_d_in_plans_sets_confirmation() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        app.update_plans(vec![make_plan(3, "MyPlan")]);
        let action = app.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_some());
    }

    #[test]
    fn test_confirm_y_executes_plan_delete() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        app.update_plans(vec![make_plan(3, "MyPlan")]);
        app.handle_key(key(KeyCode::Char('d')));
        let action = app.handle_key(key(KeyCode::Char('y')));
        match action {
            AppAction::DeletePlan(id) => assert_eq!(id, 3),
            _ => panic!("expected DeletePlan, got {:?}", action),
        }
        assert!(app.confirm_action.is_none());
        assert!(app.command_palette.message.is_none());
    }

    #[test]
    fn test_confirm_any_key_cancels() {
        let mut app = App::new();
        app.view_mode = ViewMode::Projects;
        app.update_projects(vec![make_project(1, "P")]);
        app.handle_key(key(KeyCode::Char('d')));
        // Press 'x' instead of 'y'
        let action = app.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_none());
        assert!(app.command_palette.message.is_none());
    }

    #[test]
    fn test_n_in_plans_no_project_is_noop() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_d_on_empty_plans_is_noop() {
        let mut app = App::new();
        app.view_mode = ViewMode::Plans;
        let action = app.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(action, AppAction::None));
        assert!(app.confirm_action.is_none());
    }

    // -- Session import / assign to project tests --

    #[test]
    fn test_toggle_untracked() {
        let mut app = App::new();
        assert!(!app.show_untracked);
        app.handle_key(key(KeyCode::Char('i')));
        assert!(app.show_untracked);
        app.handle_key(key(KeyCode::Char('i')));
        assert!(!app.show_untracked);
    }

    #[test]
    fn test_visible_sessions_all() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), None),
            make_session("s2"),
        ]);
        assert_eq!(app.visible_sessions().len(), 2);
    }

    #[test]
    fn test_visible_sessions_untracked_only() {
        let mut app = App::new();
        app.show_untracked = true;
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), None),
            make_session("s2"),
            make_session("s3"),
        ]);
        let visible = app.visible_sessions();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, "s2");
        assert_eq!(visible[1].id, "s3");
    }

    #[test]
    fn test_p_in_untracked_mode_opens_picker() {
        let mut app = App::new();
        app.show_untracked = true;
        app.update_sessions(vec![make_session("s1")]);
        app.update_projects(vec![make_project(1, "Proj1"), make_project(2, "Proj2")]);
        app.handle_key(key(KeyCode::Char('p')));
        assert!(app.project_picker.is_some());
        assert_eq!(app.project_picker.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_p_in_normal_mode_goes_to_projects() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("s1")]);
        let action = app.handle_key(key(KeyCode::Char('p')));
        assert!(app.project_picker.is_none());
        assert_eq!(app.view_mode, ViewMode::Projects);
        assert!(matches!(action, AppAction::LoadProjects));
    }

    #[test]
    fn test_picker_esc_closes() {
        let mut app = App::new();
        app.project_picker = Some(vec![make_project(1, "P1")]);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.project_picker.is_none());
    }

    #[test]
    fn test_picker_jk_navigation() {
        let mut app = App::new();
        app.project_picker = Some(vec![
            make_project(1, "P1"),
            make_project(2, "P2"),
            make_project(3, "P3"),
        ]);
        assert_eq!(app.picker_index, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.picker_index, 1);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.picker_index, 2);

        // Wrap forward
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.picker_index, 0);

        // Wrap backward
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.picker_index, 2);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.picker_index, 1);
    }

    #[test]
    fn test_picker_enter_assigns() {
        let mut app = App::new();
        app.update_sessions(vec![make_session("s1")]);
        app.project_picker = Some(vec![make_project(1, "P1"), make_project(2, "P2")]);
        app.picker_index = 1;
        let action = app.handle_key(key(KeyCode::Enter));
        match action {
            AppAction::AssignSessionToProject {
                session_id,
                project_id,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(project_id, 2);
            }
            _ => panic!("expected AssignSessionToProject, got {:?}", action),
        }
        assert!(app.project_picker.is_none());
    }

    #[test]
    fn test_picker_enter_empty_sessions_returns_none() {
        let mut app = App::new();
        app.project_picker = Some(vec![make_project(1, "P1")]);
        let action = app.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, AppAction::None));
        assert!(app.project_picker.is_none());
    }

    #[test]
    fn test_picker_does_not_quit_on_q() {
        let mut app = App::new();
        app.project_picker = Some(vec![make_project(1, "P1")]);
        let action = app.handle_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit);
        assert!(matches!(action, AppAction::None));
        // Picker stays open since 'q' is not handled
        assert!(app.project_picker.is_some());
    }

    #[test]
    fn test_toggle_untracked_resets_index() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("a"),
            make_session("b"),
            make_session("c"),
        ]);
        app.selected_index = 2;
        app.handle_key(key(KeyCode::Char('i')));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_select_next_wraps_in_untracked_mode() {
        let mut app = App::new();
        app.show_untracked = true;
        app.update_sessions(vec![
            make_session_with_project("s1", Some(1), None),
            make_session("s2"),
            make_session("s3"),
        ]);
        // Only s2, s3 are visible
        assert_eq!(app.visible_sessions().len(), 2);

        app.select_next();
        assert_eq!(app.selected_index, 1);

        // Wrap
        app.select_next();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_set_status_stores_message() {
        let mut app = App::new();
        app.set_status("Created project");
        assert!(app.status_message.is_some());
        assert_eq!(app.status_message.as_ref().unwrap().0, "Created project");
    }

    #[test]
    fn test_status_message_default_none() {
        let app = App::new();
        assert!(app.status_message.is_none());
    }

    #[test]
    fn test_clear_stale_status_keeps_fresh() {
        let mut app = App::new();
        app.set_status("Fresh");
        app.clear_stale_status();
        assert!(app.status_message.is_some());
    }

    #[test]
    fn test_clear_stale_status_removes_old() {
        let mut app = App::new();
        app.status_message = Some(("Old".to_string(), Instant::now() - Duration::from_secs(6)));
        app.clear_stale_status();
        assert!(app.status_message.is_none());
    }

    // -- Quick-switch tests (M3.4) --

    fn make_session_with_state(id: &str, state: SessionState) -> Session {
        Session {
            id: id.to_string(),
            pane_id: format!("%{id}"),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state,
            detection_method: "process_name".to_string(),
            last_activity: 0,
            created_at: 0,
            updated_at: 0,
            project_id: None,
            plan_step_id: None,
        }
    }

    #[test]
    fn test_number_key_selects_session() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("s1"),
            make_session("s2"),
            make_session("s3"),
        ]);
        let action = app.handle_key(key(KeyCode::Char('1')));
        assert_eq!(app.selected_index, 0);
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "s1"),
            _ => panic!("expected SelectSession, got {:?}", action),
        }
    }

    #[test]
    fn test_number_key_out_of_range_ignored() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session("s1"),
            make_session("s2"),
            make_session("s3"),
        ]);
        app.selected_index = 1;
        let action = app.handle_key(key(KeyCode::Char('5')));
        assert_eq!(app.selected_index, 1);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_tab_jumps_to_needs_input() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_state("s1", SessionState::Working),
            make_session_with_state("s2", SessionState::NeedsInput),
            make_session_with_state("s3", SessionState::Working),
        ]);
        app.selected_index = 0;
        let action = app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.selected_index, 1);
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "s2"),
            _ => panic!("expected SelectSession, got {:?}", action),
        }
    }

    #[test]
    fn test_tab_wraps_around() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_state("s1", SessionState::NeedsInput),
            make_session_with_state("s2", SessionState::Working),
            make_session_with_state("s3", SessionState::Working),
        ]);
        // Start at index 1 so wrapping is needed to find s1 at index 0
        app.selected_index = 1;
        let action = app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.selected_index, 0);
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "s1"),
            _ => panic!("expected SelectSession, got {:?}", action),
        }
    }

    #[test]
    fn test_tab_no_needs_input_is_noop() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_state("s1", SessionState::Working),
            make_session_with_state("s2", SessionState::Working),
            make_session_with_state("s3", SessionState::Done),
        ]);
        app.selected_index = 0;
        let action = app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.selected_index, 0);
        assert!(matches!(action, AppAction::None));
    }

    #[test]
    fn test_n_key_same_as_tab() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_state("s1", SessionState::Working),
            make_session_with_state("s2", SessionState::NeedsInput),
            make_session_with_state("s3", SessionState::Done),
        ]);
        app.selected_index = 0;
        let action = app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.selected_index, 1);
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "s2"),
            _ => panic!("expected SelectSession, got {:?}", action),
        }
    }

    #[test]
    fn test_tab_skips_non_needs_input() {
        let mut app = App::new();
        app.update_sessions(vec![
            make_session_with_state("s1", SessionState::Working),
            make_session_with_state("s2", SessionState::Done),
            make_session_with_state("s3", SessionState::NeedsInput),
            make_session_with_state("s4", SessionState::Working),
        ]);
        app.selected_index = 0;
        let action = app.handle_key(key(KeyCode::Tab));
        // Should skip s2 (Done) and land on s3 (NeedsInput)
        assert_eq!(app.selected_index, 2);
        match action {
            AppAction::SelectSession(id) => assert_eq!(id, "s3"),
            _ => panic!("expected SelectSession, got {:?}", action),
        }
    }

    #[test]
    fn test_number_keys_in_non_sessions_view_ignored() {
        for mode in [
            ViewMode::Projects,
            ViewMode::Plans,
            ViewMode::PlanDetail,
            ViewMode::Orchestrator,
        ] {
            let mut app = App::new();
            app.view_mode = mode;
            app.update_sessions(vec![make_session("s1"), make_session("s2")]);
            let action = app.handle_key(key(KeyCode::Char('1')));
            assert!(
                !matches!(action, AppAction::SelectSession(_)),
                "number key should not select session in {:?}",
                mode
            );
        }
    }

    #[test]
    fn test_session_state_counts_empty() {
        let app = App::new();
        assert_eq!(app.session_state_counts(), (0, 0, 0, 0));
    }

    #[test]
    fn test_session_state_counts_mixed() {
        let mut app = App::new();
        let mut s1 = make_session("s1");
        s1.state = SessionState::Working;
        let mut s2 = make_session("s2");
        s2.state = SessionState::NeedsInput;
        let mut s3 = make_session("s3");
        s3.state = SessionState::Done;
        let s4 = make_session("s4"); // default Idle
        app.update_sessions(vec![s1, s2, s3, s4]);
        assert_eq!(app.session_state_counts(), (1, 1, 1, 1));
    }

    #[test]
    fn test_session_state_counts_all_one_state() {
        let mut app = App::new();
        let mut s1 = make_session("s1");
        s1.state = SessionState::Working;
        let mut s2 = make_session("s2");
        s2.state = SessionState::Working;
        let mut s3 = make_session("s3");
        s3.state = SessionState::Working;
        app.update_sessions(vec![s1, s2, s3]);
        assert_eq!(app.session_state_counts(), (3, 0, 0, 0));
    }

    #[test]
    fn test_session_state_counts_includes_all_sessions() {
        let mut app = App::new();
        let mut s1 = make_session("s1");
        s1.state = SessionState::Working;
        s1.project_id = Some(1);
        let mut s2 = make_session("s2");
        s2.state = SessionState::NeedsInput;
        // s2 has no project_id, so it would be "untracked"
        let mut s3 = make_session("s3");
        s3.state = SessionState::Working;
        s3.project_id = Some(2);
        app.update_sessions(vec![s1, s2, s3]);

        // Counts should include ALL sessions regardless of filter mode
        app.show_untracked = false;
        assert_eq!(app.session_state_counts(), (2, 1, 0, 0));
        app.show_untracked = true;
        assert_eq!(app.session_state_counts(), (2, 1, 0, 0));
    }

    #[test]
    fn test_tick_increments() {
        let mut app = App::new();
        assert_eq!(app.tick_count, 0);
        app.tick();
        assert_eq!(app.tick_count, 1);
        app.tick();
        assert_eq!(app.tick_count, 2);
    }

    #[test]
    fn test_blink_on_initial() {
        let app = App::new();
        assert!(
            app.blink_on(),
            "blink should start in ON phase at tick_count=0"
        );
    }

    #[test]
    fn test_blink_on_toggles() {
        let mut app = App::new();
        // Ticks 0..9 => blink ON (tick_count/10 == 0, 0%2 == 0)
        assert!(app.blink_on());
        for _ in 0..10 {
            app.tick();
        }
        // Ticks 10..19 => blink OFF (tick_count/10 == 1, 1%2 == 1)
        assert!(!app.blink_on());
        for _ in 0..10 {
            app.tick();
        }
        // Ticks 20..29 => blink ON again (tick_count/10 == 2, 2%2 == 0)
        assert!(app.blink_on());
    }

    #[test]
    fn test_tick_wraps() {
        let mut app = App::new();
        app.tick_count = u64::MAX;
        app.tick();
        assert_eq!(app.tick_count, 0, "wrapping_add should wrap to 0");
        // blink_on should still work after wrapping
        assert!(app.blink_on());
    }
}
