mod app;
mod ui;

use app::App;
use ca_lib::ipc::{IpcClient, Request, Response};
use ca_lib::models::Session;
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

enum IpcEvent {
    Update(Vec<Session>),
    Disconnected,
}

async fn recv_ipc_event(client: &mut IpcClient) -> IpcEvent {
    match client.recv_response().await {
        Ok(Response::SessionUpdate { sessions }) => IpcEvent::Update(sessions),
        _ => IpcEvent::Disconnected,
    }
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mut ipc_client: Option<IpcClient>,
) -> io::Result<()> {
    let poll_duration = Duration::from_millis(50);

    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        tokio::select! {
            terminal_event = poll_crossterm_event(poll_duration) => {
                if let Some(Event::Key(key)) = terminal_event? {
                    if key.kind == KeyEventKind::Press {
                        app.handle_key(key);
                    }
                }
            }

            // Only active when we have an IPC connection
            event = async { recv_ipc_event(ipc_client.as_mut().unwrap()).await },
                if ipc_client.is_some() =>
            {
                match event {
                    IpcEvent::Update(sessions) => app.update_sessions(sessions),
                    IpcEvent::Disconnected => { ipc_client = None; }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Poll crossterm for a terminal event, yielding to tokio between checks.
async fn poll_crossterm_event(timeout: Duration) -> io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}
