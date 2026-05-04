mod app;
mod help;
mod ui;

use app::{App, AppAction};
use ca_lib::db::Database;
use ca_lib::ipc::{IpcClient, Request, Response};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let ipc_client = connect_and_subscribe().await;
    app.connected = ipc_client.is_some();

    let db = open_database();

    // Load projects at startup so sessions can be grouped by project
    if let Some(db) = &db {
        if let Ok(projects) = db.list_projects() {
            app.update_projects(projects);
        }
    }

    let result = run_event_loop(&mut terminal, &mut app, ipc_client).await;

    restore_terminal()?;
    result
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
    Ok(())
}

fn open_database() -> Option<Database> {
    let home = dirs::home_dir()?;
    let db_path = home.join(".claude-admin").join("sessions.db");
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
) -> io::Result<()> {
    loop {
        app.clear_stale_status();
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Drain all pending terminal events before yielding
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let action = app.handle_key(key);
                    handle_action(action);
                }
                _ => {}
            }
            if app.should_quit {
                return Ok(());
            }
        }

        // Yield briefly for IPC updates (~60fps)
        if let Some(client) = ipc_client.as_mut() {
            match tokio::time::timeout(Duration::from_millis(16), client.recv_response()).await {
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
            tokio::time::sleep(Duration::from_millis(16)).await;
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_action(action: AppAction) {
    match action {
        AppAction::None | AppAction::Quit | AppAction::ShowHelp | AppAction::ToggleUntracked => {}
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
