use std::path::PathBuf;

use ca_lib::events::{Event, EventType};
use ca_lib::ipc::{IpcClient, IpcError, Request, Response};
use ca_lib::models::{Session, SessionState};
use clap::{Parser, Subcommand};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Daemon is not running. Start it with: claude-admin daemon start")]
    DaemonNotRunning,
    #[error("connection error: {0}")]
    Connection(#[from] IpcError),
    #[error("daemon returned error: {0}")]
    DaemonError(String),
    #[error("config error: {0}")]
    Config(#[from] ca_lib::config::ConfigError),
    #[error("session not found: {0}")]
    NotFound(String),
}

// ---------------------------------------------------------------------------
// CLI argument structs
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "claude-admin",
    about = "CLI for the claude-admin session manager daemon",
    version
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Ping,
    List,
    Status {
        session_id: String,
    },
    Events {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    Start,
    Stop,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Ping => handle_ping(&cli.socket).await,
        Command::List => handle_list(&cli.socket).await,
        Command::Status { session_id } => handle_status(&cli.socket, &session_id).await,
        Command::Events { session_id, limit } => {
            handle_events(&cli.socket, session_id.as_deref(), limit).await
        }
        Command::Daemon { command } => handle_daemon(command),
    }
}

// ---------------------------------------------------------------------------
// Socket resolution and daemon connection
// ---------------------------------------------------------------------------

fn resolve_socket_path(override_path: &Option<PathBuf>) -> Result<PathBuf, CliError> {
    match override_path {
        Some(path) => Ok(path.clone()),
        None => {
            let home = dirs::home_dir().ok_or(ca_lib::config::ConfigError::NoHomeDir)?;
            Ok(home.join(".claude-admin").join("daemon.sock"))
        }
    }
}

fn data_dir() -> Result<PathBuf, CliError> {
    let home = dirs::home_dir().ok_or(ca_lib::config::ConfigError::NoHomeDir)?;
    Ok(home.join(".claude-admin"))
}

fn map_connection_error(e: IpcError) -> CliError {
    if let IpcError::Io(ref io_err) = e {
        if io_err.kind() == std::io::ErrorKind::ConnectionRefused
            || io_err.kind() == std::io::ErrorKind::NotFound
        {
            return CliError::DaemonNotRunning;
        }
    }
    CliError::Connection(e)
}

async fn connect_to_daemon(socket_override: &Option<PathBuf>) -> Result<IpcClient, CliError> {
    let socket_path = resolve_socket_path(socket_override)?;
    IpcClient::connect(&socket_path)
        .await
        .map_err(map_connection_error)
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

async fn handle_ping(socket: &Option<PathBuf>) -> Result<(), CliError> {
    let mut client = connect_to_daemon(socket).await?;
    match client.send(&Request::Ping).await? {
        Response::Pong => {
            println!("Daemon is running.");
            Ok(())
        }
        Response::Error { message } => Err(CliError::DaemonError(message)),
        _ => Err(CliError::DaemonError("unexpected response".to_string())),
    }
}

async fn handle_list(socket: &Option<PathBuf>) -> Result<(), CliError> {
    let mut client = connect_to_daemon(socket).await?;
    match client.send(&Request::ListSessions).await? {
        Response::SessionList { sessions } => {
            if sessions.is_empty() {
                println!("No active sessions.");
            } else {
                print!("{}", format_sessions_table(&sessions));
            }
            Ok(())
        }
        Response::Error { message } => Err(CliError::DaemonError(message)),
        _ => Err(CliError::DaemonError("unexpected response".to_string())),
    }
}

async fn handle_status(
    socket: &Option<PathBuf>,
    session_id: &str,
) -> Result<(), CliError> {
    let mut client = connect_to_daemon(socket).await?;
    let request = Request::GetSession {
        id: session_id.to_string(),
    };
    match client.send(&request).await? {
        Response::Session {
            session: Some(session),
        } => {
            print!("{}", format_session_detail(&session));
            Ok(())
        }
        Response::Session { session: None } => Err(CliError::NotFound(session_id.to_string())),
        Response::Error { message } => Err(CliError::DaemonError(message)),
        _ => Err(CliError::DaemonError("unexpected response".to_string())),
    }
}

async fn handle_events(
    socket: &Option<PathBuf>,
    session_id: Option<&str>,
    limit: usize,
) -> Result<(), CliError> {
    let mut client = connect_to_daemon(socket).await?;
    let request = match session_id {
        Some(id) => Request::GetEvents {
            session_id: id.to_string(),
            limit,
        },
        None => Request::GetRecentEvents { limit },
    };
    match client.send(&request).await? {
        Response::Events { events } => {
            if events.is_empty() {
                println!("No events found.");
            } else {
                print!("{}", format_events_list(&events));
            }
            Ok(())
        }
        Response::Error { message } => Err(CliError::DaemonError(message)),
        _ => Err(CliError::DaemonError("unexpected response".to_string())),
    }
}

fn handle_daemon(command: DaemonCommand) -> Result<(), CliError> {
    match command {
        DaemonCommand::Start => daemon_start(),
        DaemonCommand::Stop => daemon_stop(),
    }
}

fn daemon_start() -> Result<(), CliError> {
    use std::process::Command as ProcessCommand;

    let data = data_dir()?;
    let pid_path = data.join("daemon.pid");

    // Check if daemon is already running
    if let Ok(contents) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            if unsafe { libc::kill(pid, 0) } == 0 {
                println!("Daemon is already running (pid {pid}).");
                return Ok(());
            }
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let daemon_path = exe_dir
        .as_ref()
        .map(|d| d.join("daemon"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("daemon"));

    #[cfg(unix)]
    let child = {
        use std::os::unix::process::CommandExt;
        unsafe {
            ProcessCommand::new(&daemon_path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .pre_exec(|| {
                    libc::setsid();
                    Ok(())
                })
                .spawn()
        }
    };

    #[cfg(not(unix))]
    let child = ProcessCommand::new(&daemon_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match child {
        Ok(c) => {
            println!("Daemon started (pid {}).", c.id());
            Ok(())
        }
        Err(e) => Err(CliError::DaemonError(format!(
            "failed to start daemon: {e}"
        ))),
    }
}

fn daemon_stop() -> Result<(), CliError> {
    let data = data_dir()?;
    let pid_path = data.join("daemon.pid");

    let contents = std::fs::read_to_string(&pid_path).map_err(|_| {
        CliError::DaemonError("no PID file found; daemon may not be running".to_string())
    })?;

    let pid: i32 = contents.trim().parse().map_err(|_| {
        CliError::DaemonError("invalid PID file contents".to_string())
    })?;

    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid, libc::SIGTERM) };
        if result == 0 {
            println!("Sent SIGTERM to daemon (pid {pid}).");
            let _ = std::fs::remove_file(&pid_path);
        } else {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ESRCH) {
                println!("Daemon is not running (stale PID file). Cleaning up.");
                let _ = std::fs::remove_file(&pid_path);
            } else {
                return Err(CliError::DaemonError(format!(
                    "failed to send signal to pid {pid}: {err}"
                )));
            }
        }
    }

    #[cfg(not(unix))]
    {
        return Err(CliError::DaemonError(
            "daemon stop is only supported on Unix".to_string(),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

pub fn state_indicator(state: &SessionState) -> &'static str {
    match state {
        SessionState::Idle => "  ",
        SessionState::Working => "* ",
        SessionState::NeedsInput => "! ",
        SessionState::Done => "- ",
    }
}

pub fn format_session_row(session: &Session) -> String {
    format!(
        "{}{:<38} {:<12} {:<8} {:<12} {}",
        state_indicator(&session.state),
        session.id,
        session.state,
        session.pane_id,
        session.session_name,
        session.working_dir,
    )
}

pub fn format_sessions_table(sessions: &[Session]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "  {:<38} {:<12} {:<8} {:<12} {}\n",
        "ID", "STATE", "PANE", "TMUX", "DIR"
    ));
    out.push_str(&format!("  {}\n", "-".repeat(90)));
    for session in sessions {
        out.push_str(&format_session_row(session));
        out.push('\n');
    }
    out
}

pub fn format_session_detail(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!("ID:          {}\n", session.id));
    out.push_str(&format!("State:       {}\n", session.state));
    out.push_str(&format!("Pane:        {}\n", session.pane_id));
    out.push_str(&format!("Tmux:        {}\n", session.session_name));
    out.push_str(&format!("Window:      {}\n", session.window_index));
    out.push_str(&format!("Pane Index:  {}\n", session.pane_index));
    out.push_str(&format!("Directory:   {}\n", session.working_dir));
    out.push_str(&format!("Detection:   {}\n", session.detection_method));
    out.push_str(&format!(
        "Activity:    {}\n",
        format_timestamp(session.last_activity)
    ));
    out.push_str(&format!(
        "Created:     {}\n",
        format_timestamp(session.created_at)
    ));
    out.push_str(&format!(
        "Updated:     {}\n",
        format_timestamp(session.updated_at)
    ));
    out
}

pub fn format_event_row(event: &Event) -> String {
    let detail = match &event.event_type {
        EventType::StateChanged { from, to } => format!("state: {from} -> {to}"),
        EventType::HookReceived { hook_type } => format!("hook: {hook_type}"),
        EventType::SessionDiscovered => "session_discovered".to_string(),
        EventType::SessionRemoved => "session_removed".to_string(),
    };
    format!(
        "{:<6} {:<14} {:<20} {}",
        event.id,
        format_timestamp(event.timestamp),
        event.session_id,
        detail,
    )
}

pub fn format_events_list(events: &[Event]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<6} {:<14} {:<20} {}\n",
        "ID", "TIME", "SESSION", "EVENT"
    ));
    out.push_str(&format!("{}\n", "-".repeat(70)));
    for event in events {
        out.push_str(&format_event_row(event));
        out.push('\n');
    }
    out
}

pub fn format_timestamp(epoch_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff = now - epoch_secs;

    if diff < 0 {
        "in the future".to_string()
    } else if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str, pane_id: &str, state: SessionState) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
        }
    }

    fn make_event(id: i64, session_id: &str, event_type: EventType) -> Event {
        Event {
            id,
            session_id: session_id.to_string(),
            event_type,
            payload: None,
            timestamp: 1706500000,
        }
    }

    // -- Group 1: Clap parsing --

    #[test]
    fn test_clap_ping_parses() {
        let cli = Cli::parse_from(["claude-admin", "ping"]);
        assert!(matches!(cli.command, Command::Ping));
    }

    #[test]
    fn test_clap_events_with_flags() {
        let cli = Cli::parse_from([
            "claude-admin",
            "events",
            "--session-id",
            "abc",
            "--limit",
            "50",
        ]);
        match cli.command {
            Command::Events { session_id, limit } => {
                assert_eq!(session_id, Some("abc".to_string()));
                assert_eq!(limit, 50);
            }
            _ => panic!("expected Events command"),
        }
    }

    #[test]
    fn test_clap_daemon_stop_parses() {
        let cli = Cli::parse_from(["claude-admin", "daemon", "stop"]);
        match cli.command {
            Command::Daemon { command } => {
                assert!(matches!(command, DaemonCommand::Stop));
            }
            _ => panic!("expected Daemon command"),
        }
    }

    // -- Group 2: Output formatting --

    #[test]
    fn test_format_session_row_contains_fields() {
        let session = make_session("sess-abc", "%3", SessionState::Working);
        let row = format_session_row(&session);
        assert!(row.contains("sess-abc"));
        assert!(row.contains("working"));
        assert!(row.starts_with("* "));
    }

    #[test]
    fn test_format_sessions_table_header_and_rows() {
        let sessions = vec![
            make_session("sess-1", "%0", SessionState::Idle),
            make_session("sess-2", "%1", SessionState::Working),
        ];
        let table = format_sessions_table(&sessions);
        assert!(table.contains("ID"));
        assert!(table.contains("STATE"));
        assert!(table.contains("sess-1"));
        assert!(table.contains("sess-2"));
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_format_event_row_state_changed() {
        let event = make_event(
            1,
            "sess-1",
            EventType::StateChanged {
                from: SessionState::Idle,
                to: SessionState::Working,
            },
        );
        let row = format_event_row(&event);
        assert!(row.contains("state: idle -> working"));
    }

    #[test]
    fn test_format_events_list_header_and_rows() {
        let events = vec![
            make_event(1, "sess-1", EventType::SessionDiscovered),
            make_event(2, "sess-2", EventType::SessionRemoved),
        ];
        let list = format_events_list(&events);
        assert!(list.contains("ID"));
        assert!(list.contains("SESSION"));
        assert!(list.contains("EVENT"));
        let lines: Vec<&str> = list.lines().collect();
        assert_eq!(lines.len(), 4);
    }

    // -- Group 3: Error handling --

    #[test]
    fn test_connection_refused_maps_to_daemon_not_running() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let ipc_err = IpcError::Io(io_err);
        let cli_err = map_connection_error(ipc_err);
        assert!(matches!(cli_err, CliError::DaemonNotRunning));
        assert!(cli_err.to_string().contains("Daemon is not running"));
    }

    #[test]
    fn test_not_found_maps_to_daemon_not_running() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let ipc_err = IpcError::Io(io_err);
        let cli_err = map_connection_error(ipc_err);
        assert!(matches!(cli_err, CliError::DaemonNotRunning));
    }

    // -- Group 4: State indicator --

    #[test]
    fn test_state_indicator_values() {
        assert_eq!(state_indicator(&SessionState::Idle), "  ");
        assert_eq!(state_indicator(&SessionState::Working), "* ");
        assert_eq!(state_indicator(&SessionState::NeedsInput), "! ");
        assert_eq!(state_indicator(&SessionState::Done), "- ");
    }
}
