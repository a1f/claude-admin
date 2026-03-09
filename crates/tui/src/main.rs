mod app;
mod command_palette;
mod commands;
mod form;
mod help;
mod input;
mod plan_view;
mod project_view;
mod review_view;
mod ui;

use app::{App, AppAction, InputMode};
use ca_lib::db::Database;
use ca_lib::ipc::{IpcClient, Request, Response};
use ca_lib::plan::PlanContent;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let ipc_client = connect_and_subscribe().await;
    app.connected = ipc_client.is_some();

    let db = open_database();

    let result = run_event_loop(&mut terminal, &mut app, ipc_client, db.as_ref()).await;

    restore_terminal()?;
    result
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn open_database() -> Option<Database> {
    let home = dirs::home_dir()?;
    let db_path = home.join(".claude-admin").join("claude-admin.db");
    Database::open(&db_path).ok()
}

/// Connect to daemon and subscribe for push updates.
/// Returns None if the daemon is not running.
async fn connect_and_subscribe() -> Option<IpcClient> {
    let home = dirs::home_dir()?;
    let socket_path = home.join(".claude-admin").join("daemon.sock");

    let mut client = IpcClient::connect(&socket_path).await.ok()?;

    match client.send(&Request::Subscribe).await {
        Ok(Response::Subscribed) => Some(client),
        _ => None,
    }
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mut ipc_client: Option<IpcClient>,
    db: Option<&Database>,
) -> io::Result<()> {
    loop {
        app.clear_stale_status();
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let action = app.handle_key(key);
                    match &action {
                        AppAction::OpenVimdiff { .. } | AppAction::OpenDelta { .. } => {
                            handle_external_tool(terminal, app, db, action)?;
                        }
                        _ => handle_action(action, app, db),
                    }
                }
            }
        } else if let Some(client) = ipc_client.as_mut() {
            match tokio::time::timeout(Duration::from_millis(50), client.recv_response()).await {
                Ok(Ok(Response::SessionUpdate { sessions })) => {
                    app.update_sessions(sessions);
                }
                Ok(Ok(_)) | Ok(Err(_)) => {
                    ipc_client = None;
                    app.connected = false;
                }
                Err(_) => {}
            }
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_external_tool(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    db: Option<&Database>,
    action: AppAction,
) -> io::Result<()> {
    let repo_path = resolve_review_repo_path(app, db);
    let Some(repo) = repo_path else {
        app.set_status("No repo path found for review");
        return Ok(());
    };

    match action {
        AppAction::OpenVimdiff {
            base_commit,
            head_commit,
            file_path,
        } => {
            let _ = restore_terminal();
            let status = std::process::Command::new("git")
                .args([
                    "difftool",
                    "--no-prompt",
                    "--tool=vimdiff",
                    &format!("{base_commit}..{head_commit}"),
                    "--",
                    &file_path,
                ])
                .current_dir(&repo)
                .status();

            enable_raw_mode()?;
            execute!(io::stdout(), EnterAlternateScreen)?;
            terminal.clear()?;

            if let Err(e) = status {
                app.set_status(format!("vimdiff failed: {e}"));
            }
        }
        AppAction::OpenDelta {
            base_commit,
            head_commit,
            file_path,
        } => {
            let delta_available = std::process::Command::new("which")
                .arg("delta")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if !delta_available {
                app.set_status("delta not found. Install: brew install git-delta");
                return Ok(());
            }

            let _ = restore_terminal();

            let git_child = std::process::Command::new("git")
                .args([
                    "diff",
                    &format!("{base_commit}..{head_commit}"),
                    "--",
                    &file_path,
                ])
                .current_dir(&repo)
                .stdout(std::process::Stdio::piped())
                .spawn();

            if let Ok(mut git_proc) = git_child {
                if let Some(stdout) = git_proc.stdout.take() {
                    // Pipe git diff output through delta with a pager
                    let _ = std::process::Command::new("delta").stdin(stdout).status();
                }
                let _ = git_proc.wait();
            }

            enable_raw_mode()?;
            execute!(io::stdout(), EnterAlternateScreen)?;
            terminal.clear()?;
        }
        _ => {}
    }

    Ok(())
}

fn handle_action(action: AppAction, app: &mut App, db: Option<&Database>) {
    match action {
        AppAction::None | AppAction::Quit => {}
        AppAction::SelectSession(session_id) => {
            if let Some(db) = db {
                if let Ok(events) = db.get_events(&session_id, 20) {
                    app.update_preview(events);
                } else {
                    app.clear_preview();
                }
            }
        }
        AppAction::LoadProjects => {
            if let Some(db) = db {
                if let Ok(projects) = db.list_projects() {
                    app.update_projects(projects);
                }
            }
        }
        AppAction::LoadPlans(project_id) => {
            if let Some(db) = db {
                if let Ok(plans) = db.list_plans_by_project(project_id) {
                    app.update_plans(plans);
                }
            }
        }
        AppAction::LoadPlan(plan_id) => {
            if let Some(db) = db {
                if let Ok(Some(plan)) = db.get_plan(plan_id) {
                    app.update_current_plan(plan);
                }
            }
        }
        AppAction::CycleStepStatus {
            plan_id,
            step_id,
            new_status,
        } => {
            if let Some(db) = db {
                if db.update_step_status(plan_id, &step_id, new_status).is_ok() {
                    if let Ok(Some(plan)) = db.get_plan(plan_id) {
                        app.update_current_plan(plan);
                    }
                }
            }
        }
        AppAction::SpawnStep { plan_id, step_id } => {
            if let Some(db) = db {
                spawn_step_session(db, app, plan_id, &step_id);
            }
        }
        AppAction::AttachSession(pane_id) => {
            let _ = std::process::Command::new("tmux")
                .args(["select-window", "-t", &pane_id])
                .output();
            let _ = std::process::Command::new("tmux")
                .args(["select-pane", "-t", &pane_id])
                .output();
        }
        AppAction::ExecuteCommand(cmd) => match commands::parse_command(&cmd) {
            Ok(action) => {
                handle_action(action, app, db);
            }
            Err(msg) => {
                app.command_palette.message = Some(msg);
            }
        },
        AppAction::SubmitForm => {
            if let Some(form) = app.form_overlay.take() {
                let values = form.field_values();
                handle_form_submit(form.kind, &values, app, db);
            }
        }
        AppAction::OpenForm(kind) => {
            app.open_form(kind);
        }
        AppAction::CreateWorkspace { path, name } => {
            if let Some(db) = db {
                if let Ok(_ws) = db.create_workspace(&path, name.as_deref()) {
                    app.set_status("Created workspace");
                    if let Ok(workspaces) = db.list_workspaces() {
                        app.update_workspaces(workspaces);
                    }
                }
            }
        }
        AppAction::CreateProject {
            workspace_id,
            name,
            description,
        } => {
            if let Some(db) = db {
                if let Ok(_proj) = db.create_project(workspace_id, &name, description.as_deref()) {
                    app.set_status(format!("Created project '{name}'"));
                    if let Ok(projects) = db.list_projects() {
                        app.update_projects(projects);
                    }
                }
            }
        }
        AppAction::CreatePlan { project_id, name } => {
            let content = PlanContent { phases: vec![] };
            if let Some(db) = db {
                if let Ok(_plan) = db.create_plan(project_id, &name, &content) {
                    app.set_status(format!("Created plan '{name}'"));
                    if let Ok(plans) = db.list_plans_by_project(project_id) {
                        app.update_plans(plans);
                    }
                }
            }
        }
        AppAction::DeleteWorkspace(id) => {
            if let Some(db) = db {
                let _ = db.delete_workspace(id);
                app.set_status("Deleted workspace");
                if let Ok(workspaces) = db.list_workspaces() {
                    app.update_workspaces(workspaces);
                }
            }
        }
        AppAction::DeleteProject(id) => {
            if let Some(db) = db {
                let _ = db.delete_project(id);
                app.set_status("Deleted project");
                if let Ok(projects) = db.list_projects() {
                    app.update_projects(projects);
                }
            }
        }
        AppAction::DeletePlan(id) => {
            if let Some(db) = db {
                let _ = db.delete_plan(id);
                app.set_status("Deleted plan");
                if let Some(project) = app.projects.get(app.project_index) {
                    if let Ok(plans) = db.list_plans_by_project(project.id) {
                        app.update_plans(plans);
                    }
                }
            }
        }
        AppAction::LoadWorkspaces => {
            if let Some(db) = db {
                if let Ok(workspaces) = db.list_workspaces() {
                    app.update_workspaces(workspaces);
                }
            }
        }
        AppAction::ShowHelp => {
            app.input_mode = InputMode::Help;
        }
        AppAction::ToggleUntracked => {}
        AppAction::AssignSessionToProject {
            session_id,
            project_id,
        } => {
            if let Some(db) = db {
                if let Ok(Some(mut session)) = db.get_session(&session_id) {
                    session.project_id = Some(project_id);
                    let _ = db.update_session(&session);
                    app.set_status("Session assigned to project");
                }
            }
        }
        AppAction::LoadReview(review_id) => {
            if let Some(db) = db {
                if let Ok(Some(review)) = db.get_review(review_id) {
                    app.review = Some(review);
                    app.view_mode = app::ViewMode::Review;
                    app.review_scroll = 0;
                    app.review_file_index = 0;
                }
            }
        }
        AppAction::LoadReviewDiff => {
            if let Some(review) = &app.review {
                let base = review.base_commit.clone();
                let head = review.head_commit.clone();
                // Attempt to resolve repo path from project -> workspace
                let repo_path = resolve_review_repo_path(app, db);
                if let Some(path) = repo_path {
                    if let Ok(files) =
                        ca_lib::git_ops::git_diff(std::path::Path::new(&path), &base, &head)
                    {
                        app.review_diff_files = files;
                        app.review_file_index = 0;
                        app.review_scroll = 0;
                    }
                }
            }
        }
        AppAction::AddReviewComment {
            review_id,
            file_path,
            line_number,
            body,
        } => {
            if let Some(db) = db {
                let _ = db.add_review_comment(review_id, "", &file_path, line_number, &body);
                app.set_status("Comment added");
            }
        }
        AppAction::SubmitReview {
            review_id,
            session_id,
        } => {
            submit_review(app, db, review_id, &session_id);
        }
        // Handled in run_event_loop before reaching handle_action
        AppAction::OpenVimdiff { .. } | AppAction::OpenDelta { .. } => {}
    }
}

fn resolve_review_repo_path(app: &App, db: Option<&Database>) -> Option<String> {
    let review = app.review.as_ref()?;
    let db = db?;
    let project_id = review.project_id?;
    let project = db.get_project(project_id).ok()??;
    let workspace = db.get_workspace(project.workspace_id).ok()??;
    Some(project.worktree_path.unwrap_or(workspace.path))
}

fn submit_review(app: &mut App, db: Option<&Database>, review_id: i64, session_id: &str) {
    let Some(db) = db else {
        return;
    };

    let comments = match db.get_review_comments(review_id) {
        Ok(c) => c,
        Err(_) => return,
    };

    let review = match db.get_review(review_id) {
        Ok(Some(r)) => r,
        _ => return,
    };

    let feedback = ca_lib::review::format_review_feedback(&review, &comments);
    if feedback.is_empty() {
        app.set_status("No comments to submit");
        return;
    }

    let session = match db.get_session(session_id) {
        Ok(Some(s)) => s,
        _ => {
            app.set_status("Session not found");
            return;
        }
    };

    let escaped = ca_lib::review::escape_for_tmux(&feedback);
    let _ = std::process::Command::new("tmux")
        .args(["send-keys", "-t", &session.pane_id, &escaped, "Enter"])
        .status();

    let _ = db.update_review_status(review_id, ca_lib::review::ReviewStatus::ChangesRequested);
    let _ = db.increment_review_round(review_id);

    app.set_status(format!(
        "Review feedback sent ({} comments)",
        comments.len()
    ));

    if let Ok(Some(updated)) = db.get_review(review_id) {
        app.review = Some(updated);
    }
}

fn handle_form_submit(
    kind: form::FormKind,
    values: &[String],
    app: &mut App,
    db: Option<&Database>,
) {
    let Some(db) = db else { return };

    match kind {
        form::FormKind::CreateWorkspace => {
            let path = &values[0];
            let name = values.get(1).filter(|v| !v.is_empty()).map(String::as_str);
            if let Ok(_ws) = db.create_workspace(path, name) {
                app.set_status(format!("Created workspace '{path}'"));
                if let Ok(workspaces) = db.list_workspaces() {
                    app.update_workspaces(workspaces);
                }
            }
        }
        form::FormKind::CreateProject { workspace_id } => {
            let name = &values[0];
            let desc = values.get(1).filter(|v| !v.is_empty()).map(String::as_str);
            if let Ok(_proj) = db.create_project(workspace_id, name, desc) {
                app.set_status(format!("Created project '{name}'"));
                if let Ok(projects) = db.list_projects() {
                    app.update_projects(projects);
                }
            }
        }
        form::FormKind::CreatePlan { project_id } => {
            let name = &values[0];
            let content = PlanContent { phases: vec![] };
            if let Ok(_plan) = db.create_plan(project_id, name, &content) {
                app.set_status(format!("Created plan '{name}'"));
                if let Ok(plans) = db.list_plans_by_project(project_id) {
                    app.update_plans(plans);
                }
            }
        }
    }
}

fn spawn_step_session(db: &Database, app: &mut App, plan_id: i64, step_id: &str) {
    let plan = match db.get_plan(plan_id) {
        Ok(Some(p)) => p,
        _ => return,
    };
    let project = match db.get_project(plan.project_id) {
        Ok(Some(p)) => p,
        _ => return,
    };
    let workspace = match db.get_workspace(project.workspace_id) {
        Ok(Some(w)) => w,
        _ => return,
    };

    let working_dir = project.worktree_path.as_deref().unwrap_or(&workspace.path);

    let context = match ca_lib::spawn::generate_plan_context(&plan, step_id) {
        Ok(c) => c,
        Err(_) => return,
    };
    let context_path = match ca_lib::spawn::write_context_file(&context) {
        Ok(p) => p,
        Err(_) => return,
    };

    let _ = db.update_step_status(plan_id, step_id, ca_lib::plan::StepStatus::InProgress);

    let opts = ca_lib::spawn::SpawnOptions {
        working_dir: working_dir.to_string(),
        context_file: Some(context_path.to_string_lossy().to_string()),
        window_name: Some(format!("step-{step_id}")),
    };
    let _ = ca_lib::spawn::spawn_tmux_session(&opts);

    if let Ok(Some(updated)) = db.get_plan(plan_id) {
        app.update_current_plan(updated);
    }
}
