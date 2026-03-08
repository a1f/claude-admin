mod app;
mod plan_view;
mod project_view;
mod ui;

use app::{App, AppAction};
use ca_lib::db::Database;
use ca_lib::ipc::{IpcClient, Request, Response};
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
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_action(app.handle_key(key), app, db);
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
