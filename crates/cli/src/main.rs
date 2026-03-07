use std::path::PathBuf;

use ca_lib::db::{Database, DbError};
use ca_lib::events::{Event, EventType};
use ca_lib::ipc::{IpcClient, IpcError, Request, Response};
use ca_lib::models::{Session, SessionState};
use ca_lib::plan::{Plan, PlanContent, PlanStatus, StepStatus};
use ca_lib::project::{Project, ProjectStatus};
use ca_lib::workspace::Workspace;
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
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
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
    /// Manage Claude Code hook integration
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
    /// Manage workspaces
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Manage plans
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// Spawn a Claude session for a plan step
    Spawn {
        /// Plan ID
        plan_id: i64,
        /// Step ID (e.g., "0.1", "1.2")
        #[arg(long)]
        step: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceCommand {
    /// Add a workspace
    Add {
        path: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// List all workspaces
    List,
    /// Delete a workspace
    Delete { id: i64 },
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommand {
    /// Create a project in a workspace
    Create {
        workspace_id: i64,
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        branch: Option<String>,
    },
    /// List projects
    List {
        #[arg(long)]
        workspace: Option<i64>,
    },
    /// Update project status
    Status { id: i64, status: String },
    /// Delete a project
    Delete { id: i64 },
}

#[derive(Subcommand, Debug)]
pub enum PlanCommand {
    /// Create a plan from a JSON file
    Create {
        project_id: i64,
        name: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// List plans
    List {
        #[arg(long)]
        project: Option<i64>,
    },
    /// Update plan status
    Status { id: i64, status: String },
    /// Update a plan step status
    Step { id: i64, step_id: String, status: String },
    /// Show plan details
    Show { id: i64 },
    /// Delete a plan
    Delete { id: i64 },
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    Start,
    Stop,
}

#[derive(Subcommand, Debug)]
pub enum HooksCommand {
    /// Install Claude Code hooks for session monitoring
    Install,
    /// Remove Claude Code hooks
    Uninstall,
    /// Check hook installation status
    Status,
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
        Command::Hooks { command } => handle_hooks(command),
        Command::Workspace { command } => handle_workspace(command),
        Command::Project { command } => handle_project(command),
        Command::Plan { command } => handle_plan(command),
        Command::Spawn { plan_id, step } => handle_spawn(plan_id, &step),
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

fn handle_hooks(command: HooksCommand) -> Result<(), CliError> {
    let map_err = |e: ca_lib::hook_install::HookInstallError| CliError::DaemonError(e.to_string());

    match command {
        HooksCommand::Install => {
            let script = ca_lib::hook_install::hook_script_path().map_err(map_err)?;
            let settings = ca_lib::hook_install::settings_path().map_err(map_err)?;
            let result = ca_lib::hook_install::install_hooks(&script, &settings).map_err(map_err)?;

            if result.already_installed {
                println!("Hooks are already installed.");
            } else {
                println!("Installed hooks: {}", result.hook_types_added.join(", "));
                println!("Settings: {}", result.settings_path.display());
            }
            Ok(())
        }
        HooksCommand::Uninstall => {
            let settings = ca_lib::hook_install::settings_path().map_err(map_err)?;
            let removed = ca_lib::hook_install::uninstall_hooks(&settings).map_err(map_err)?;

            if removed {
                println!("Hooks removed from {}", settings.display());
            } else {
                println!("No hooks found to remove.");
            }
            Ok(())
        }
        HooksCommand::Status => {
            let settings = ca_lib::hook_install::settings_path().map_err(map_err)?;
            let status = ca_lib::hook_install::hooks_status(&settings).map_err(map_err)?;

            println!("Hook status:");
            for (hook_type, installed) in &status {
                let indicator = if *installed { "[x]" } else { "[ ]" };
                println!("  {indicator} {hook_type}");
            }
            Ok(())
        }
    }
}

fn open_db() -> Result<Database, CliError> {
    let data = data_dir()?;
    let db_path = data.join("claude-admin.db");
    Ok(Database::open(&db_path)?)
}

// ---------------------------------------------------------------------------
// Workspace / Project / Plan handlers
// ---------------------------------------------------------------------------

fn handle_workspace(command: WorkspaceCommand) -> Result<(), CliError> {
    let db = open_db()?;
    match command {
        WorkspaceCommand::Add { path, name } => {
            let ws = db.create_workspace(&path, name.as_deref())?;
            println!("Created workspace {} (id={})", ws.name, ws.id);
            Ok(())
        }
        WorkspaceCommand::List => {
            let workspaces = db.list_workspaces()?;
            if workspaces.is_empty() {
                println!("No workspaces.");
            } else {
                print!("{}", format_workspaces_table(&workspaces));
            }
            Ok(())
        }
        WorkspaceCommand::Delete { id } => {
            if db.delete_workspace(id)? {
                println!("Deleted workspace {id}.");
            } else {
                println!("Workspace {id} not found.");
            }
            Ok(())
        }
    }
}

fn handle_project(command: ProjectCommand) -> Result<(), CliError> {
    let db = open_db()?;
    match command {
        ProjectCommand::Create {
            workspace_id,
            name,
            description,
            branch: _,
        } => {
            let proj = db.create_project(workspace_id, &name, description.as_deref())?;
            println!("Created project \"{}\" (id={})", proj.name, proj.id);
            Ok(())
        }
        ProjectCommand::List { workspace } => {
            let projects = match workspace {
                Some(ws_id) => db.list_projects_by_workspace(ws_id)?,
                None => db.list_projects()?,
            };
            if projects.is_empty() {
                println!("No projects.");
            } else {
                print!("{}", format_projects_table(&projects));
            }
            Ok(())
        }
        ProjectCommand::Status { id, status } => {
            let parsed: ProjectStatus = status
                .parse()
                .map_err(|_| CliError::InvalidInput(format!("invalid status: {status}. Use: active, running, completed, archived")))?;
            db.update_project_status(id, parsed)?;
            println!("Updated project {id} status to {parsed}.");
            Ok(())
        }
        ProjectCommand::Delete { id } => {
            if db.delete_project(id)? {
                println!("Deleted project {id}.");
            } else {
                println!("Project {id} not found.");
            }
            Ok(())
        }
    }
}

fn handle_plan(command: PlanCommand) -> Result<(), CliError> {
    let db = open_db()?;
    match command {
        PlanCommand::Create {
            project_id,
            name,
            file,
        } => {
            let json_str = std::fs::read_to_string(&file)?;
            let content: PlanContent = serde_json::from_str(&json_str)
                .map_err(|e| CliError::InvalidInput(format!("invalid plan JSON: {e}")))?;
            let plan = db.create_plan(project_id, &name, &content)?;
            println!("Created plan \"{}\" (id={})", plan.name, plan.id);
            Ok(())
        }
        PlanCommand::List { project } => {
            let plans = match project {
                Some(proj_id) => db.list_plans_by_project(proj_id)?,
                None => {
                    // No list_plans_all method exists, so we'll handle this
                    // by listing for project=0 which returns empty, or show error
                    return Err(CliError::InvalidInput(
                        "please specify --project <id>".to_string(),
                    ));
                }
            };
            if plans.is_empty() {
                println!("No plans.");
            } else {
                print!("{}", format_plans_table(&plans));
            }
            Ok(())
        }
        PlanCommand::Status { id, status } => {
            let parsed: PlanStatus = status
                .parse()
                .map_err(|_| CliError::InvalidInput(format!("invalid status: {status}. Use: draft, active, completed, abandoned")))?;
            db.update_plan_status(id, parsed)?;
            println!("Updated plan {id} status to {parsed}.");
            Ok(())
        }
        PlanCommand::Step {
            id,
            step_id,
            status,
        } => {
            let parsed: StepStatus = status
                .parse()
                .map_err(|_| CliError::InvalidInput(format!("invalid step status: {status}. Use: pending, in_progress, completed, blocked, skipped")))?;
            db.update_step_status(id, &step_id, parsed)?;
            println!("Updated plan {id} step {step_id} to {parsed}.");
            Ok(())
        }
        PlanCommand::Show { id } => {
            match db.get_plan(id)? {
                Some(plan) => print!("{}", format_plan_detail(&plan)),
                None => println!("Plan {id} not found."),
            }
            Ok(())
        }
        PlanCommand::Delete { id } => {
            if db.delete_plan(id)? {
                println!("Deleted plan {id}.");
            } else {
                println!("Plan {id} not found.");
            }
            Ok(())
        }
    }
}

fn handle_spawn(plan_id: i64, step_id: &str) -> Result<(), CliError> {
    let map_spawn_err = |e: ca_lib::spawn::SpawnError| CliError::DaemonError(e.to_string());
    let db = open_db()?;

    let plan = db
        .get_plan(plan_id)?
        .ok_or_else(|| CliError::NotFound(format!("plan {plan_id}")))?;

    let project = db
        .get_project(plan.project_id)?
        .ok_or_else(|| CliError::NotFound(format!("project {}", plan.project_id)))?;

    let workspace = db
        .get_workspace(project.workspace_id)?
        .ok_or_else(|| CliError::NotFound(format!("workspace {}", project.workspace_id)))?;

    let working_dir = project
        .worktree_path
        .as_deref()
        .unwrap_or(&workspace.path);

    let context = ca_lib::spawn::generate_plan_context(&plan, step_id)
        .map_err(|e| CliError::InvalidInput(e.to_string()))?;

    let context_path =
        ca_lib::spawn::write_context_file(&context).map_err(&map_spawn_err)?;

    db.update_step_status(plan_id, step_id, StepStatus::InProgress)?;

    let window_name = format!("step-{step_id}");
    let opts = ca_lib::spawn::SpawnOptions {
        working_dir: working_dir.to_string(),
        context_file: Some(context_path.to_string_lossy().to_string()),
        window_name: Some(window_name),
    };

    let pane_id =
        ca_lib::spawn::spawn_tmux_session(&opts).map_err(map_spawn_err)?;

    println!("Spawned Claude session for step {step_id}");
    println!("  Plan: {} (id={})", plan.name, plan.id);
    println!("  Dir:  {working_dir}");
    println!("  Pane: {pane_id}");
    println!("  Context: {}", context_path.display());

    Ok(())
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

pub fn format_workspaces_table(workspaces: &[Workspace]) -> String {
    let mut out = String::new();
    out.push_str(&format!("{:<6} {:<20} {}\n", "ID", "NAME", "PATH"));
    out.push_str(&format!("{}\n", "-".repeat(60)));
    for ws in workspaces {
        out.push_str(&format!("{:<6} {:<20} {}\n", ws.id, ws.name, ws.path));
    }
    out
}

pub fn format_projects_table(projects: &[Project]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<6} {:<6} {:<20} {:<12} {}\n",
        "ID", "WS", "NAME", "STATUS", "DESCRIPTION"
    ));
    out.push_str(&format!("{}\n", "-".repeat(70)));
    for proj in projects {
        out.push_str(&format!(
            "{:<6} {:<6} {:<20} {:<12} {}\n",
            proj.id,
            proj.workspace_id,
            proj.name,
            proj.status,
            proj.description.as_deref().unwrap_or("")
        ));
    }
    out
}

pub fn format_plans_table(plans: &[Plan]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<6} {:<6} {:<30} {:<12} {}\n",
        "ID", "PROJ", "NAME", "STATUS", "STEPS"
    ));
    out.push_str(&format!("{}\n", "-".repeat(70)));
    for plan in plans {
        let step_count: usize = plan.content.phases.iter().map(|p| p.steps.len()).sum();
        out.push_str(&format!(
            "{:<6} {:<6} {:<30} {:<12} {}\n",
            plan.id, plan.project_id, plan.name, plan.status, step_count
        ));
    }
    out
}

pub fn format_plan_detail(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str(&format!("ID:       {}\n", plan.id));
    out.push_str(&format!("Project:  {}\n", plan.project_id));
    out.push_str(&format!("Name:     {}\n", plan.name));
    out.push_str(&format!("Status:   {}\n", plan.status));
    out.push_str(&format!(
        "Created:  {}\n",
        format_timestamp(plan.created_at)
    ));
    out.push_str(&format!(
        "Updated:  {}\n",
        format_timestamp(plan.updated_at)
    ));
    out.push('\n');
    for phase in &plan.content.phases {
        out.push_str(&format!("Phase: {}\n", phase.name));
        for step in &phase.steps {
            out.push_str(&format!(
                "  [{:<12}] {} - {}\n",
                step.status.as_str(),
                step.id,
                step.description
            ));
        }
    }
    out
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
            project_id: None,
            plan_step_id: None,
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

    // -- Group 5: Hooks CLI parsing --

    #[test]
    fn test_clap_hooks_install_parses() {
        let cli = Cli::parse_from(["claude-admin", "hooks", "install"]);
        match cli.command {
            Command::Hooks { command } => {
                assert!(matches!(command, HooksCommand::Install));
            }
            _ => panic!("expected Hooks command"),
        }
    }

    #[test]
    fn test_clap_hooks_uninstall_parses() {
        let cli = Cli::parse_from(["claude-admin", "hooks", "uninstall"]);
        match cli.command {
            Command::Hooks { command } => {
                assert!(matches!(command, HooksCommand::Uninstall));
            }
            _ => panic!("expected Hooks command"),
        }
    }

    #[test]
    fn test_clap_hooks_status_parses() {
        let cli = Cli::parse_from(["claude-admin", "hooks", "status"]);
        match cli.command {
            Command::Hooks { command } => {
                assert!(matches!(command, HooksCommand::Status));
            }
            _ => panic!("expected Hooks command"),
        }
    }

    // -- Group 6: Workspace/Project/Plan CLI parsing --

    #[test]
    fn test_clap_workspace_add_parses() {
        let cli = Cli::parse_from(["claude-admin", "workspace", "add", "/home/user/dev"]);
        match cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Add { path, name } => {
                    assert_eq!(path, "/home/user/dev");
                    assert!(name.is_none());
                }
                _ => panic!("expected Add"),
            },
            _ => panic!("expected Workspace command"),
        }
    }

    #[test]
    fn test_clap_workspace_add_with_name_parses() {
        let cli = Cli::parse_from([
            "claude-admin",
            "workspace",
            "add",
            "/home/user/dev",
            "--name",
            "myws",
        ]);
        match cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Add { path, name } => {
                    assert_eq!(path, "/home/user/dev");
                    assert_eq!(name, Some("myws".to_string()));
                }
                _ => panic!("expected Add"),
            },
            _ => panic!("expected Workspace command"),
        }
    }

    #[test]
    fn test_clap_workspace_list_parses() {
        let cli = Cli::parse_from(["claude-admin", "workspace", "list"]);
        assert!(matches!(
            cli.command,
            Command::Workspace {
                command: WorkspaceCommand::List
            }
        ));
    }

    #[test]
    fn test_clap_workspace_delete_parses() {
        let cli = Cli::parse_from(["claude-admin", "workspace", "delete", "5"]);
        match cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Delete { id } => assert_eq!(id, 5),
                _ => panic!("expected Delete"),
            },
            _ => panic!("expected Workspace command"),
        }
    }

    #[test]
    fn test_clap_project_create_parses() {
        let cli = Cli::parse_from([
            "claude-admin",
            "project",
            "create",
            "1",
            "auth-feature",
            "--description",
            "Auth system",
        ]);
        match cli.command {
            Command::Project { command } => match command {
                ProjectCommand::Create {
                    workspace_id,
                    name,
                    description,
                    branch,
                } => {
                    assert_eq!(workspace_id, 1);
                    assert_eq!(name, "auth-feature");
                    assert_eq!(description, Some("Auth system".to_string()));
                    assert!(branch.is_none());
                }
                _ => panic!("expected Create"),
            },
            _ => panic!("expected Project command"),
        }
    }

    #[test]
    fn test_clap_project_list_with_workspace_parses() {
        let cli = Cli::parse_from(["claude-admin", "project", "list", "--workspace", "2"]);
        match cli.command {
            Command::Project { command } => match command {
                ProjectCommand::List { workspace } => {
                    assert_eq!(workspace, Some(2));
                }
                _ => panic!("expected List"),
            },
            _ => panic!("expected Project command"),
        }
    }

    #[test]
    fn test_clap_project_status_parses() {
        let cli = Cli::parse_from(["claude-admin", "project", "status", "3", "completed"]);
        match cli.command {
            Command::Project { command } => match command {
                ProjectCommand::Status { id, status } => {
                    assert_eq!(id, 3);
                    assert_eq!(status, "completed");
                }
                _ => panic!("expected Status"),
            },
            _ => panic!("expected Project command"),
        }
    }

    #[test]
    fn test_clap_plan_create_parses() {
        let cli = Cli::parse_from([
            "claude-admin",
            "plan",
            "create",
            "1",
            "Auth Plan",
            "--file",
            "plan.json",
        ]);
        match cli.command {
            Command::Plan { command } => match command {
                PlanCommand::Create {
                    project_id,
                    name,
                    file,
                } => {
                    assert_eq!(project_id, 1);
                    assert_eq!(name, "Auth Plan");
                    assert_eq!(file, PathBuf::from("plan.json"));
                }
                _ => panic!("expected Create"),
            },
            _ => panic!("expected Plan command"),
        }
    }

    #[test]
    fn test_clap_plan_step_parses() {
        let cli = Cli::parse_from([
            "claude-admin",
            "plan",
            "step",
            "1",
            "0.1",
            "completed",
        ]);
        match cli.command {
            Command::Plan { command } => match command {
                PlanCommand::Step {
                    id,
                    step_id,
                    status,
                } => {
                    assert_eq!(id, 1);
                    assert_eq!(step_id, "0.1");
                    assert_eq!(status, "completed");
                }
                _ => panic!("expected Step"),
            },
            _ => panic!("expected Plan command"),
        }
    }

    #[test]
    fn test_clap_plan_show_parses() {
        let cli = Cli::parse_from(["claude-admin", "plan", "show", "5"]);
        match cli.command {
            Command::Plan { command } => match command {
                PlanCommand::Show { id } => assert_eq!(id, 5),
                _ => panic!("expected Show"),
            },
            _ => panic!("expected Plan command"),
        }
    }

    // -- Group 6b: Spawn CLI parsing --

    #[test]
    fn test_clap_spawn_parses() {
        let cli = Cli::parse_from(["claude-admin", "spawn", "1", "--step", "0.1"]);
        match cli.command {
            Command::Spawn { plan_id, step } => {
                assert_eq!(plan_id, 1);
                assert_eq!(step, "0.1");
            }
            _ => panic!("expected Spawn command"),
        }
    }

    #[test]
    fn test_clap_spawn_dotted_step_id() {
        let cli = Cli::parse_from(["claude-admin", "spawn", "42", "--step", "2.3"]);
        match cli.command {
            Command::Spawn { plan_id, step } => {
                assert_eq!(plan_id, 42);
                assert_eq!(step, "2.3");
            }
            _ => panic!("expected Spawn command"),
        }
    }

    // -- Group 7: Formatting for workspace/project/plan --

    #[test]
    fn test_format_workspaces_table() {
        let workspaces = vec![Workspace {
            id: 1,
            name: "myapp".to_string(),
            path: "/home/user/myapp".to_string(),
            created_at: 1706400000,
            updated_at: 1706500000,
        }];
        let table = format_workspaces_table(&workspaces);
        assert!(table.contains("ID"));
        assert!(table.contains("NAME"));
        assert!(table.contains("myapp"));
        assert!(table.contains("/home/user/myapp"));
    }

    #[test]
    fn test_format_projects_table() {
        let projects = vec![Project {
            id: 1,
            workspace_id: 1,
            name: "auth".to_string(),
            description: Some("Auth feature".to_string()),
            status: ProjectStatus::Active,
            worktree_path: None,
            branch_name: None,
            created_at: 1706400000,
            updated_at: 1706500000,
        }];
        let table = format_projects_table(&projects);
        assert!(table.contains("auth"));
        assert!(table.contains("active"));
        assert!(table.contains("Auth feature"));
    }

    #[test]
    fn test_format_plan_detail_output() {
        let plan = Plan {
            id: 1,
            project_id: 1,
            name: "Test Plan".to_string(),
            content: PlanContent {
                phases: vec![ca_lib::plan::Phase {
                    name: "Setup".to_string(),
                    steps: vec![ca_lib::plan::Step {
                        id: "0.1".to_string(),
                        description: "Init project".to_string(),
                        status: StepStatus::Completed,
                        exit_criteria: ca_lib::plan::ExitCriteria {
                            description: "done".to_string(),
                            commands: vec![],
                        },
                    }],
                }],
            },
            status: PlanStatus::Active,
            created_at: 1706400000,
            updated_at: 1706500000,
        };
        let detail = format_plan_detail(&plan);
        assert!(detail.contains("Test Plan"));
        assert!(detail.contains("Phase: Setup"));
        assert!(detail.contains("0.1"));
        assert!(detail.contains("Init project"));
        assert!(detail.contains("completed"));
    }
}
