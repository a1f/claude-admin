mod app;
mod ui;

use app::App;
use ca_lib::ipc::{IpcClient, Request, Response};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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
    let result = run_event_loop(&mut terminal, &mut app, ipc_client).await;

    restore_terminal()?;
    result
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
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
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Check for crossterm events without blocking (zero timeout)
        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        } else if let Some(client) = ipc_client.as_mut() {
            // No terminal event ready — wait briefly for IPC or next tick
            match tokio::time::timeout(Duration::from_millis(50), client.recv_response()).await {
                Ok(Ok(Response::SessionUpdate { sessions })) => {
                    app.update_sessions(sessions);
                }
                Ok(Ok(_)) | Ok(Err(_)) => {
                    ipc_client = None;
                    app.connected = false;
                }
                Err(_) => {} // timeout, loop back to check terminal events
            }
        } else {
            // No IPC connection — just sleep briefly to avoid busy-wait
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
