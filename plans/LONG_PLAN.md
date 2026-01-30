# Claude Admin - Development Plan

## Design Document

Full high-level design: **`/Users/alf/dev/claude_admin/plans/high-level-design.md`**

---

## Implementation Status

| Step | Description | Status |
|------|-------------|--------|
| 2.1 | Basic Daemon Lifecycle | **IMPLEMENTED** |

### Step 2.1: Basic Daemon Lifecycle - IMPLEMENTED

Implemented daemon initialization with:
- CLI argument parsing (log level, paths)
- Dual logging (human-readable + JSON)
- PID file management with duplicate instance protection
- Unix socket with ping/pong
- SQLite database initialization with WAL mode
- Graceful shutdown on SIGTERM/SIGINT

Files created:
```
crates/daemon/
├── Cargo.toml
└── src/
    ├── main.rs      # Entry point, signal handling
    ├── config.rs    # CLI args, path handling
    ├── logging.rs   # Dual log setup
    ├── pid.rs       # PID file management
    ├── socket.rs    # Unix socket server
    └── db.rs        # SQLite initialization
```

19 unit tests passing.

---

## V1 Scope

Minimal viable session manager:
- **Daemon**: Track Claude sessions, store events in SQLite, auto-discover sessions
- **TUI**: List sessions with status, attach to them
- **States**: IDLE, WORKING, NEEDS_INPUT, DONE
- **Events**: Claude hooks (prompt events) + tmux output watching

---

## Technical Decisions

| Decision | Choice |
|----------|--------|
| Database | SQLite |
| IPC | Unix socket |
| Event source | Claude hooks + log watching |
| Project structure | Cargo workspace (lib + daemon + tui) |
| Log format | Human-readable + JSON option |
| Hooks location | Global `~/.claude/` |
| TUI testing | Snapshot testing |
| Integration tests | Mock tmux for unit, real tmux for integration |

---

## Project Structure

```
claude-admin/
├── Cargo.toml                 # Workspace manifest
├── crates/
│   ├── lib/      # Shared library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── db/            # SQLite operations
│   │   │   ├── models/        # Session, Event, State types
│   │   │   ├── tmux/          # Tmux command wrapper
│   │   │   ├── ipc/           # Unix socket protocol
│   │   │   └── hooks/         # Claude hooks config
│   │   └── Cargo.toml
│   ├── daemon/         # Daemon binary
│   │   ├── src
│   │   │   ├── main.rs
│   │   │   ├── server.rs      # IPC server
│   │   │   ├── monitor.rs     # Session monitoring
│   │   │   └── discovery.rs   # Auto-discover sessions
│   │   └── Cargo.toml
│   └── tui/          # TUI binary
│       ├── src/
│       │   ├── main.rs
│       │   ├── app.rs         # App state
│       │   ├── ui.rs          # Render logic
│       │   └── client.rs      # IPC client
│       └── Cargo.toml
├── tests/                     # Integration tests
│   ├── daemon_tests.rs
│   └── tui_tests.rs
└── scripts/
    └── install-hooks.sh       # Install Claude hooks
```

---

# Phase 1: Foundation

## Step 1.1: Project Setup

**Goal**: Cargo workspace with all crates compiling

**Tasks**:
1. Create workspace `Cargo.toml`
2. Create `lib` crate with stub modules
3. Create `daemon` crate with main.rs that prints "daemon starting"
4. Create `tui` crate with main.rs that prints "tui starting"
5. Add shared dependencies (tokio, serde, thiserror)

**Files to create**:
```
Cargo.toml                           # workspace manifest
crates/lib/Cargo.toml
crates/lib/src/lib.rs   # pub mod db, models, tmux, ipc;
crates/lib/src/db/mod.rs
crates/lib/src/models/mod.rs
crates/lib/src/tmux/mod.rs
crates/lib/src/ipc/mod.rs
crates/daemon/Cargo.toml
crates/daemon/src/main.rs
crates/tui/Cargo.toml
crates/tui/src/main.rs
```

**Dependencies**:
```toml
# lib
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
rusqlite = { version = "0.31", features = ["bundled"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
tracing = "0.1"

# daemon (additional)
tracing-subscriber = { version = "0.3", features = ["json"] }
clap = { version = "4", features = ["derive"] }

# tui (additional)
ratatui = "0.26"
crossterm = "0.27"
```

**Exit Criteria**:
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo test --workspace` passes (no tests yet, but compiles)
- [ ] `cargo run -p daemon` prints "daemon starting"
- [ ] `cargo run -p tui` prints "tui starting"

**Tests**: None yet (setup step)

**Validation commands**:
```bash
cargo build --workspace
cargo test --workspace
cargo run -p daemon
cargo run -p tui
```

---

## Step 1.2: Core Models & Database

**Goal**: Define data models and SQLite schema

**Tasks**:
1. Define `Session` struct
2. Define `SessionState` enum
3. Define `Event` struct
4. Define `EventType` enum
5. Create SQLite schema
6. Implement `Database` struct with CRUD

**Files to create/modify**:
```
crates/lib/src/models/mod.rs      # Re-exports
crates/lib/src/models/session.rs  # Session, SessionState
crates/lib/src/models/event.rs    # Event, EventType
crates/lib/src/db/mod.rs          # Database struct
crates/lib/src/db/schema.rs       # SQL schema
crates/lib/src/db/tests.rs        # Unit tests
```

**Data structures**:
```rust
// models/session.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub tmux_session: String,      // tmux session name
    pub tmux_window: String,       // tmux window index/name
    pub repo_path: PathBuf,        // working directory
    pub state: SessionState,
    pub last_output: Option<String>, // last captured output snippet
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SessionState {
    Idle,
    Working,
    NeedsInput,
    Done,
}

// models/event.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub session_id: Uuid,
    pub event_type: EventType,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    SessionCreated,
    SessionDiscovered,
    StateChanged { from: SessionState, to: SessionState },
    PromptSubmit,
    AssistantResponse,
    ToolCall { tool: String },
    OutputCaptured,
}
```

**SQL Schema**:
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    tmux_session TEXT NOT NULL,
    tmux_window TEXT NOT NULL,
    repo_path TEXT NOT NULL,
    state TEXT NOT NULL,
    last_output TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    timestamp TEXT NOT NULL
);

CREATE INDEX idx_events_session ON events(session_id);
CREATE INDEX idx_sessions_state ON sessions(state);
```

**Database API**:
```rust
impl Database {
    pub fn new(path: &Path) -> Result<Self>;
    pub fn create_session(&self, session: &Session) -> Result<()>;
    pub fn get_session(&self, id: Uuid) -> Result<Option<Session>>;
    pub fn update_session(&self, session: &Session) -> Result<()>;
    pub fn list_sessions(&self) -> Result<Vec<Session>>;
    pub fn list_sessions_by_state(&self, state: SessionState) -> Result<Vec<Session>>;
    pub fn delete_session(&self, id: Uuid) -> Result<()>;
    pub fn log_event(&self, event: &Event) -> Result<()>;
    pub fn get_events(&self, session_id: Uuid, limit: usize) -> Result<Vec<Event>>;
}
```

**Exit Criteria**:
- [ ] All model structs compile with Serialize/Deserialize
- [ ] `Database::new()` creates DB file with schema
- [ ] Unit tests for all CRUD operations pass
- [ ] Unit tests for event logging pass

**Tests**:
```rust
// crates/lib/src/db/tests.rs
#[test] fn test_create_and_get_session()
#[test] fn test_update_session_state()
#[test] fn test_list_sessions()
#[test] fn test_list_sessions_by_state()
#[test] fn test_delete_session()
#[test] fn test_log_and_get_events()
#[test] fn test_session_state_serialization()
#[test] fn test_event_type_serialization()
```

**Validation**:
```bash
cargo test -p lib db::tests
```

---

## Step 1.3: Tmux Wrapper

**Goal**: Rust interface to tmux commands

**Tasks**:
1. Define `Tmux` trait for mockability
2. Implement `RealTmux` using Command
3. Implement `MockTmux` for testing
4. Parse tmux output formats

**Files to create/modify**:
```
crates/lib/src/tmux/mod.rs      # Trait + re-exports
crates/lib/src/tmux/real.rs     # RealTmux implementation
crates/lib/src/tmux/mock.rs     # MockTmux for testing
crates/lib/src/tmux/models.rs   # TmuxSession, TmuxWindow
crates/lib/src/tmux/tests.rs    # Unit tests
tests/tmux_integration.rs                     # Integration tests
```

**Trait definition**:
```rust
// tmux/mod.rs
#[async_trait]
pub trait Tmux: Send + Sync {
    /// List all tmux sessions
    async fn list_sessions(&self) -> Result<Vec<TmuxSession>>;

    /// List windows in a session
    async fn list_windows(&self, session: &str) -> Result<Vec<TmuxWindow>>;

    /// Capture pane content
    async fn capture_pane(&self, session: &str, window: &str) -> Result<String>;

    /// Get pane PID (to check if claude is running)
    async fn get_pane_pid(&self, session: &str, window: &str) -> Result<Option<u32>>;

    /// Check if process is claude
    async fn is_claude_process(&self, pid: u32) -> Result<bool>;
}

// tmux/models.rs
#[derive(Debug, Clone)]
pub struct TmuxSession {
    pub name: String,
    pub created: DateTime<Utc>,
    pub attached: bool,
}

#[derive(Debug, Clone)]
pub struct TmuxWindow {
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub pane_pid: Option<u32>,
}
```

**Tmux commands used**:
```bash
# List sessions
tmux list-sessions -F "#{session_name}|#{session_created}|#{session_attached}"

# List windows
tmux list-windows -t {session} -F "#{window_index}|#{window_name}|#{window_active}|#{pane_pid}"

# Capture pane (last 100 lines)
tmux capture-pane -t {session}:{window} -p -S -100

# Get pane PID
tmux display-message -t {session}:{window} -p "#{pane_pid}"
```

**Exit Criteria**:
- [ ] `RealTmux` compiles and runs tmux commands
- [ ] `MockTmux` returns configurable responses
- [ ] Unit tests pass with MockTmux
- [ ] Integration test creates tmux session, lists it, captures output

**Tests**:
```rust
// Unit tests (with MockTmux)
#[tokio::test] async fn test_list_sessions_parses_output()
#[tokio::test] async fn test_list_windows_parses_output()
#[tokio::test] async fn test_capture_pane_returns_content()
#[tokio::test] async fn test_handles_empty_session_list()
#[tokio::test] async fn test_handles_tmux_not_running()

// Integration tests (real tmux) - tests/tmux_integration.rs
#[tokio::test] async fn test_real_list_sessions()
#[tokio::test] async fn test_real_capture_pane()
#[tokio::test] async fn test_real_detect_process()
```

**Validation**:
```bash
cargo test -p lib tmux::tests
cargo test --test tmux_integration
```

---

## Step 1.4: IPC Protocol

**Goal**: Unix socket communication between daemon and TUI

**Tasks**:
1. Define Request/Response message types
2. Implement JSON serialization over newline-delimited stream
3. Create `IpcServer` for daemon
4. Create `IpcClient` for TUI
5. Handle connection lifecycle

**Files to create/modify**:
```
crates/lib/src/ipc/mod.rs        # Re-exports
crates/lib/src/ipc/messages.rs   # Request, Response enums
crates/lib/src/ipc/server.rs     # IpcServer
crates/lib/src/ipc/client.rs     # IpcClient
crates/lib/src/ipc/tests.rs      # Unit tests
tests/ipc_integration.rs                       # Integration tests
```

**Message types**:
```rust
// ipc/messages.rs
#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Ping,
    ListSessions,
    GetSession { id: Uuid },
    GetSessionByTmux { session: String, window: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Pong,
    Sessions(Vec<Session>),
    Session(Option<Session>),
    Error { message: String },
}
```

**Server API**:
```rust
// ipc/server.rs
pub struct IpcServer {
    socket_path: PathBuf,
    // ...
}

impl IpcServer {
    pub async fn bind(socket_path: &Path) -> Result<Self>;
    pub async fn accept(&self) -> Result<IpcConnection>;
    pub fn cleanup(&self) -> Result<()>;  // Remove stale socket
}

pub struct IpcConnection {
    // ...
}

impl IpcConnection {
    pub async fn recv(&mut self) -> Result<Request>;
    pub async fn send(&mut self, response: Response) -> Result<()>;
}
```

**Client API**:
```rust
// ipc/client.rs
pub struct IpcClient {
    // ...
}

impl IpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self>;
    pub async fn request(&mut self, req: Request) -> Result<Response>;
    pub async fn list_sessions(&mut self) -> Result<Vec<Session>>;
    pub async fn get_session(&mut self, id: Uuid) -> Result<Option<Session>>;
}
```

**Exit Criteria**:
- [ ] Messages serialize/deserialize correctly
- [ ] Server binds to socket, accepts connections
- [ ] Client connects, sends request, receives response
- [ ] Stale socket cleanup works

**Tests**:
```rust
// Unit tests
#[test] fn test_request_serialization()
#[test] fn test_response_serialization()
#[test] fn test_error_response()

// Integration tests
#[tokio::test] async fn test_server_client_ping()
#[tokio::test] async fn test_server_client_list_sessions()
#[tokio::test] async fn test_client_reconnect_on_disconnect()
#[tokio::test] async fn test_stale_socket_cleanup()
```

**Validation**:
```bash
cargo test -p lib ipc::tests
cargo test --test ipc_integration
```

---

## Step 1.4: IPC Protocol

**Goal**: Unix socket communication between daemon and TUI

**Tasks**:
1. Define `Request` enum (ListSessions, GetSession, AttachSession)
2. Define `Response` enum (SessionList, Session, Error)
3. Implement JSON serialization
4. Create `IpcServer` (daemon side) - accepts connections, handles requests
5. Create `IpcClient` (TUI side) - connects, sends requests, receives responses

**Exit Criteria**:
- [ ] Unit tests for message serialization
- [ ] Integration test: server + client in same process, request/response works
- [ ] Daemon log shows: "IPC server listening on ~/.claude-admin/daemon.sock"

**Tests**:
```rust
// Unit tests
#[test] fn test_request_serialization()
#[test] fn test_response_serialization()

// Integration tests
#[tokio::test] async fn test_ipc_list_sessions()
#[tokio::test] async fn test_ipc_get_session()
```

---

# Phase 2: Daemon Core

## Step 2.1: Basic Daemon Lifecycle

**Goal**: Daemon starts, creates socket, handles shutdown gracefully

**Tasks**:
1. Parse CLI args
2. Initialize logging
3. Create IPC server
4. Handle signals for graceful shutdown
5. Write PID file for single-instance check

**Files to create/modify**:
```
crates/daemon/src/main.rs       # Entry point, CLI parsing
crates/daemon/src/config.rs     # Configuration handling
crates/daemon/src/logging.rs    # Log setup (human + JSON)
crates/daemon/src/server.rs     # Main server loop
```

**CLI arguments**:
```rust
// main.rs
#[derive(Parser)]
struct Args {
    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Log file path (human-readable)
    #[arg(long, default_value = "~/.claude-admin/daemon.log")]
    log_file: PathBuf,

    /// JSON log file path (structured logs)
    #[arg(long, default_value = "~/.claude-admin/daemon.json.log")]
    json_log_file: PathBuf,

    /// Unix socket path
    #[arg(long, default_value = "~/.claude-admin/daemon.sock")]
    socket_path: PathBuf,

    /// Database path
    #[arg(long, default_value = "~/.claude-admin/sessions.db")]
    db_path: PathBuf,
}
```

**Directory structure** (created at startup):
```
~/.claude-admin/
├── daemon.sock          # Unix socket
├── daemon.pid           # PID file
├── daemon.log           # Human-readable logs
├── daemon.json.log      # JSON structured logs
└── sessions.db          # SQLite database
```

**Exit Criteria**:
- [ ] `daemon` starts and logs "Daemon started"
- [ ] Log entry: `{"level":"INFO","message":"Daemon started","timestamp":"..."}`
- [ ] Second instance fails with "Daemon already running"
- [ ] SIGTERM triggers "Daemon shutting down" log
- [ ] PID file removed on clean shutdown

**Tests**:
```rust
// Unit tests
#[test] fn test_config_defaults()
#[test] fn test_config_from_args()

// Integration tests
#[tokio::test] async fn test_daemon_starts_and_logs()
#[tokio::test] async fn test_daemon_single_instance_check()
#[tokio::test] async fn test_daemon_graceful_shutdown()
#[test] fn test_log_output_format_human()
#[test] fn test_log_output_format_json()
```

**Validation**:
```bash
# Start daemon
cargo run -p daemon

# Check logs
cat ~/.claude-admin/daemon.log
cat ~/.claude-admin/daemon.json.log | jq .

# Check PID file
cat ~/.claude-admin/daemon.pid

# Test single instance
cargo run -p daemon  # Should fail

# Test shutdown
kill $(cat ~/.claude-admin/daemon.pid)
# Verify "Daemon shutting down" in logs
```

---

## Step 2.2: Session Discovery

**Goal**: Daemon discovers existing Claude sessions on startup

**Tasks**:
1. On startup, scan tmux sessions/windows
2. For each window, check if running `claude` process
3. Detect working directory from tmux
4. Create Session records in DB
5. Set initial state (heuristic from output)

**Files to create/modify**:
```
crates/daemon/src/discovery.rs  # Discovery logic
crates/daemon/src/server.rs     # Call discovery on startup
```

**Discovery logic**:
```rust
// discovery.rs
pub struct SessionDiscovery<T: Tmux> {
    tmux: T,
    db: Database,
}

impl<T: Tmux> SessionDiscovery<T> {
    /// Discover all Claude sessions in tmux
    pub async fn discover(&self) -> Result<Vec<Session>> {
        let mut discovered = Vec::new();

        for tmux_session in self.tmux.list_sessions().await? {
            for window in self.tmux.list_windows(&tmux_session.name).await? {
                if let Some(pid) = window.pane_pid {
                    if self.tmux.is_claude_process(pid).await? {
                        let session = self.create_session(&tmux_session, &window).await?;
                        discovered.push(session);
                    }
                }
            }
        }

        Ok(discovered)
    }

    /// Detect initial state from pane output
    async fn detect_initial_state(&self, session: &str, window: &str) -> SessionState {
        let output = self.tmux.capture_pane(session, window).await
            .unwrap_or_default();

        if output.contains("╭") || output.contains("Tool:") {
            SessionState::Working
        } else if output.trim().ends_with(">") || output.contains("?") {
            SessionState::NeedsInput
        } else {
            SessionState::Idle
        }
    }
}
```

**Exit Criteria**:
- [ ] Start daemon with Claude running in tmux → session in DB
- [ ] Log: "Discovered session: main:0 (/path/to/repo) state=Working"
- [ ] IPC `ListSessions` returns discovered sessions
- [ ] Non-Claude windows ignored

**Tests**:
```rust
// Unit tests (with MockTmux)
#[tokio::test] async fn test_discovers_claude_process()
#[tokio::test] async fn test_ignores_non_claude_process()
#[tokio::test] async fn test_detects_working_state()
#[tokio::test] async fn test_detects_needs_input_state()

// Integration tests
#[tokio::test] async fn test_real_discovery_with_claude()
```

**Validation**:
```bash
# Start a Claude session in tmux
tmux new-session -d -s test
tmux send-keys -t test "claude" Enter

# Start daemon
cargo run -p daemon

# Check logs for discovery
grep "Discovered session" ~/.claude-admin/daemon.log

# Query via simple client (or nc)
echo '{"ListSessions":{}}' | nc -U ~/.claude-admin/daemon.sock
```

---

## Step 2.3: Session Monitoring Loop

**Goal**: Continuously monitor sessions for state changes

**Tasks**:
1. Background task polls sessions every N seconds
2. Capture pane output, analyze state
3. Update DB on state change
4. Log state transitions
5. Remove sessions when tmux window closes

**Files to create/modify**:
```
crates/daemon/src/monitor.rs    # Monitoring loop
crates/daemon/src/state.rs      # State detection logic
crates/daemon/src/server.rs     # Spawn monitor task
```

**State detection patterns**:
```rust
// state.rs
pub struct StateDetector {
    /// Patterns indicating WORKING state
    working_patterns: Vec<Regex>,
    /// Patterns indicating NEEDS_INPUT state
    input_patterns: Vec<Regex>,
    /// Patterns indicating DONE state
    done_patterns: Vec<Regex>,
}

impl StateDetector {
    pub fn new() -> Self {
        Self {
            working_patterns: vec![
                Regex::new(r"Tool:").unwrap(),          // Tool call
                Regex::new(r"Reading").unwrap(),        // Reading file
                Regex::new(r"Writing").unwrap(),        // Writing file
                Regex::new(r"╭─").unwrap(),             // Claude UI box
            ],
            input_patterns: vec![
                Regex::new(r">\s*$").unwrap(),          // Prompt
                Regex::new(r"\?\s*$").unwrap(),         // Question
                Regex::new(r"Approve\?").unwrap(),      // Permission prompt
            ],
            done_patterns: vec![
                Regex::new(r"Session ended").unwrap(),
                Regex::new(r"Goodbye").unwrap(),
            ],
        }
    }

    pub fn detect(&self, output: &str, last_activity: Duration) -> SessionState {
        // Check most recent lines
        let recent = output.lines().rev().take(20).collect::<Vec<_>>().join("\n");

        if self.done_patterns.iter().any(|p| p.is_match(&recent)) {
            return SessionState::Done;
        }

        if last_activity > Duration::from_secs(5) {
            if self.input_patterns.iter().any(|p| p.is_match(&recent)) {
                return SessionState::NeedsInput;
            }
        }

        if self.working_patterns.iter().any(|p| p.is_match(&recent)) {
            return SessionState::Working;
        }

        SessionState::Idle
    }
}
```

**Monitor loop**:
```rust
// monitor.rs
pub async fn run_monitor(
    db: Database,
    tmux: impl Tmux,
    interval: Duration,
    mut shutdown: broadcast::Receiver<()>,
) {
    let detector = StateDetector::new();
    let mut interval = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = check_sessions(&db, &tmux, &detector).await {
                    tracing::error!("Monitor error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                tracing::info!("Monitor shutting down");
                break;
            }
        }
    }
}
```

**Exit Criteria**:
- [ ] Monitor runs every 2 seconds (configurable)
- [ ] State changes detected and logged
- [ ] DB updated with new state
- [ ] Log: "Session abc123 state changed: Working → NeedsInput"
- [ ] Session removed when tmux window closes

**Tests**:
```rust
// Unit tests
#[test] fn test_detect_working_from_tool_call()
#[test] fn test_detect_input_from_prompt()
#[test] fn test_detect_done_from_exit()
#[test] fn test_idle_when_no_patterns_match()

// Integration tests
#[tokio::test] async fn test_monitor_detects_state_change()
#[tokio::test] async fn test_monitor_removes_closed_session()
```

**Validation**:
```bash
# Start daemon and watch logs
cargo run -p daemon &
tail -f ~/.claude-admin/daemon.log

# In Claude session, trigger different states
# Watch for state change logs
```

---

## Step 2.4: Claude Hooks Integration

**Goal**: Receive structured events from Claude via hooks

**Tasks**:
1. Create hook script
2. Add HTTP endpoint in daemon for hook events
3. Handle prompt events
4. Associate events with sessions
5. Create install script

**Files to create/modify**:
```
crates/daemon/src/hooks.rs           # Hook event handling
crates/daemon/src/http.rs            # HTTP server for hooks
scripts/claude-admin-hook.sh                # Hook script
scripts/install-hooks.sh                    # Install to ~/.claude/
crates/lib/src/hooks/mod.rs    # Hook config types
```

**Hook script**:
```bash
#!/bin/bash
# scripts/claude-admin-hook.sh
# Called by Claude on prompt events

SOCKET="$HOME/.claude-admin/daemon.sock"
EVENT_TYPE="$1"
shift

# Build JSON payload
PAYLOAD=$(cat <<EOF
{
    "HookEvent": {
        "event_type": "$EVENT_TYPE",
        "session_id": "$CLAUDE_SESSION_ID",
        "cwd": "$(pwd)",
        "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
        "data": $@
    }
}
EOF
)

# Send to daemon
echo "$PAYLOAD" | nc -U "$SOCKET" 2>/dev/null || true
```

**Hooks config** (installed to ~/.claude/settings.json):
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": ["command:~/.claude-admin/hooks/claude-admin-hook.sh PreToolUse"]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "*",
        "hooks": ["command:~/.claude-admin/hooks/claude-admin-hook.sh PostToolUse"]
      }
    ],
    "Notification": [
      {
        "matcher": "*",
        "hooks": ["command:~/.claude-admin/hooks/claude-admin-hook.sh Notification"]
      }
    ]
  }
}
```

**IPC extension for hooks**:
```rust
// ipc/messages.rs - add to Request enum
pub enum Request {
    // ... existing
    HookEvent {
        event_type: String,
        session_id: Option<String>,
        cwd: PathBuf,
        timestamp: DateTime<Utc>,
        data: serde_json::Value,
    },
}
```

**Exit Criteria**:
- [ ] Hook script installed at `~/.claude-admin/hooks/`
- [ ] Hooks registered in `~/.claude/settings.json`
- [ ] Claude tool use sends event to daemon
- [ ] Event logged and stored in DB
- [ ] Session state updated from hook events

**Tests**:
```rust
// Unit tests
#[test] fn test_parse_hook_event()
#[test] fn test_match_session_from_cwd()

// Integration tests
#[tokio::test] async fn test_hook_event_updates_state()
#[tokio::test] async fn test_hook_event_stored_in_db()
```

**Validation**:
```bash
# Install hooks
./scripts/install-hooks.sh

# Verify installation
cat ~/.claude/settings.json | jq '.hooks'
ls -la ~/.claude-admin/hooks/

# Start daemon
cargo run -p daemon &

# Start Claude and use a tool
claude
# > Read some file

# Check daemon logs for hook events
grep "HookEvent" ~/.claude-admin/daemon.log
```

---

# Phase 3: TUI

## Step 3.1: Basic TUI Framework

**Goal**: TUI renders, handles input, connects to daemon

**Tasks**:
1. Setup ratatui with crossterm backend
2. Create `App` struct with state
3. Implement main loop (events + render)
4. Connect to daemon on startup
5. Handle connection failure gracefully

**Files to create/modify**:
```
crates/tui/src/main.rs        # Entry point
crates/tui/src/app.rs         # App state machine
crates/tui/src/ui.rs          # Render functions
crates/tui/src/client.rs      # IPC client wrapper
crates/tui/src/events.rs      # Input event handling
crates/tui/src/tui.rs         # Terminal setup/teardown
```

**App state**:
```rust
// app.rs
pub struct App {
    pub state: AppState,
    pub sessions: Vec<Session>,
    pub selected: Option<usize>,
    pub client: Option<IpcClient>,
    pub error: Option<String>,
}

pub enum AppState {
    Connecting,
    Connected,
    Disconnected,
    Error(String),
}

impl App {
    pub fn new() -> Self { /* ... */ }

    pub async fn connect(&mut self, socket_path: &Path) -> Result<()> {
        self.state = AppState::Connecting;
        match IpcClient::connect(socket_path).await {
            Ok(client) => {
                self.client = Some(client);
                self.state = AppState::Connected;
                self.refresh_sessions().await?;
                Ok(())
            }
            Err(e) => {
                self.state = AppState::Error(e.to_string());
                Err(e)
            }
        }
    }

    pub async fn refresh_sessions(&mut self) -> Result<()> {
        if let Some(client) = &mut self.client {
            self.sessions = client.list_sessions().await?;
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Action {
        match key {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                Action::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                Action::None
            }
            KeyCode::Char('a') | KeyCode::Enter => {
                if let Some(session) = self.selected_session() {
                    Action::Attach(session.clone())
                } else {
                    Action::None
                }
            }
            KeyCode::Char('?') => Action::ToggleHelp,
            _ => Action::None,
        }
    }
}

pub enum Action {
    None,
    Quit,
    Attach(Session),
    ToggleHelp,
    Refresh,
}
```

**Main loop**:
```rust
// main.rs
#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = tui::init()?;
    let mut app = App::new();

    // Try to connect
    let socket_path = PathBuf::from(
        shellexpand::tilde("~/.claude-admin/daemon.sock").to_string()
    );
    let _ = app.connect(&socket_path).await;

    // Event loop
    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = crossterm::event::read()? {
                match app.handle_key(key.code) {
                    Action::Quit => break,
                    Action::Attach(session) => {
                        tui::restore()?;
                        attach_to_session(&session)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        // Periodic refresh
        app.refresh_sessions().await.ok();
    }

    tui::restore()?;
    Ok(())
}
```

**Exit Criteria**:
- [ ] TUI starts and shows "Connecting to daemon..."
- [ ] If daemon not running, shows "Could not connect to daemon"
- [ ] `q` quits the TUI cleanly
- [ ] Terminal restored properly on exit

**Tests**:
```rust
// Snapshot tests (crates/tui/src/ui/tests.rs)
#[test] fn test_render_connecting_state() {
    let app = App { state: AppState::Connecting, ..Default::default() };
    let snapshot = render_to_string(&app);
    insta::assert_snapshot!(snapshot);
}

#[test] fn test_render_connection_error() {
    let app = App {
        state: AppState::Error("Connection refused".into()),
        ..Default::default()
    };
    let snapshot = render_to_string(&app);
    insta::assert_snapshot!(snapshot);
}
```

**Validation**:
```bash
# Without daemon
cargo run -p tui
# Should show error state

# With daemon
cargo run -p daemon &
cargo run -p tui
# Should show connected state
```

---

## Step 3.2: Session List View

**Goal**: Display sessions from daemon

**Tasks**:
1. Fetch sessions from daemon via IPC
2. Render grouped session list
3. Keyboard navigation
4. Selection highlight

**Files to modify**:
```
crates/tui/src/ui.rs          # Add session list rendering
crates/tui/src/app.rs         # Add grouping logic
```

**Session grouping**:
```rust
// app.rs
impl App {
    /// Group sessions by repo path
    pub fn grouped_sessions(&self) -> Vec<(String, Vec<&Session>)> {
        let mut groups: BTreeMap<String, Vec<&Session>> = BTreeMap::new();

        for session in &self.sessions {
            let repo_name = session.repo_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            groups.entry(repo_name).or_default().push(session);
        }

        groups.into_iter().collect()
    }
}
```

**Render function**:
```rust
// ui.rs
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),    // Header
        Constraint::Min(0),       // Session list
        Constraint::Length(1),    // Status bar
    ]).split(f.area());

    render_header(f, chunks[0]);
    render_sessions(f, chunks[1], app);
    render_status_bar(f, chunks[2], app);
}

fn render_sessions(f: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    let mut flat_index = 0;

    for (repo_name, sessions) in app.grouped_sessions() {
        // Repo header
        lines.push(Line::from(format!("  {}/", repo_name)).bold());

        for (i, session) in sessions.iter().enumerate() {
            let is_selected = Some(flat_index) == app.selected;
            let prefix = if i == sessions.len() - 1 { "└─" } else { "├─" };
            let cursor = if is_selected { "▶" } else { " " };

            let (icon, style) = match session.state {
                SessionState::Working => ("●", Style::default().fg(Color::Blue)),
                SessionState::NeedsInput => ("!", Style::default().fg(Color::Yellow)),
                SessionState::Done => ("✓", Style::default().fg(Color::Green)),
                SessionState::Idle => (" ", Style::default().fg(Color::DarkGray)),
            };

            let line = Line::from(vec![
                Span::raw(format!("  {}{}", prefix, cursor)),
                Span::styled(format!("[{}]", icon), style),
                Span::raw(format!(" {}    ", session.tmux_window)),
                Span::styled(format!("{:?}", session.state), style),
            ]);

            lines.push(if is_selected {
                line.style(Style::default().bg(Color::DarkGray))
            } else {
                line
            });

            flat_index += 1;
        }
    }

    let list = Paragraph::new(lines);
    f.render_widget(list, area);
}
```

**Exit Criteria**:
- [ ] Sessions displayed grouped by repo
- [ ] Status icons: ● (Working/blue), ! (NeedsInput/yellow), ✓ (Done/green)
- [ ] j/k and arrow keys navigate
- [ ] Selected session highlighted

**Tests**:
```rust
// Snapshot tests
#[test] fn test_render_session_list_empty() {
    let app = App { sessions: vec![], ..Default::default() };
    insta::assert_snapshot!(render_to_string(&app));
}

#[test] fn test_render_session_list_with_sessions() {
    let app = App {
        sessions: vec![
            Session { state: SessionState::Working, tmux_window: "feature/auth".into(), .. },
            Session { state: SessionState::NeedsInput, tmux_window: "feature/api".into(), .. },
        ],
        selected: Some(0),
        ..Default::default()
    };
    insta::assert_snapshot!(render_to_string(&app));
}

#[test] fn test_render_grouped_by_repo() {
    let app = App {
        sessions: vec![
            Session { repo_path: "/code/project-a".into(), .. },
            Session { repo_path: "/code/project-b".into(), .. },
            Session { repo_path: "/code/project-a".into(), .. },
        ],
        ..Default::default()
    };
    insta::assert_snapshot!(render_to_string(&app));
}
```

---

## Step 3.3: Attach to Session

**Goal**: Jump to selected tmux session

**Tasks**:
1. On Enter/`a`, get tmux coordinates
2. Exit TUI cleanly
3. Exec into tmux

**Files to modify**:
```
crates/tui/src/attach.rs      # Attach logic
crates/tui/src/main.rs        # Handle attach action
```

**Attach logic**:
```rust
// attach.rs
use std::os::unix::process::CommandExt;

pub fn attach_to_session(session: &Session) -> Result<()> {
    let target = format!("{}:{}", session.tmux_session, session.tmux_window);

    // Check if we're already in tmux
    if std::env::var("TMUX").is_ok() {
        // Use switch-client
        let err = std::process::Command::new("tmux")
            .args(["switch-client", "-t", &target])
            .exec();
        Err(err.into())
    } else {
        // Use attach-session
        let err = std::process::Command::new("tmux")
            .args(["attach-session", "-t", &target])
            .exec();
        Err(err.into())
    }
}
```

**Exit Criteria**:
- [ ] Pressing `a` attaches to correct tmux window
- [ ] TUI exits before attaching
- [ ] Works from outside tmux (attach-session)
- [ ] Works from inside tmux (switch-client)

**Tests**:
```rust
// Unit tests
#[test] fn test_attach_command_outside_tmux() {
    std::env::remove_var("TMUX");
    let cmd = build_attach_command(&session);
    assert_eq!(cmd.get_program(), "tmux");
    assert_eq!(cmd.get_args().collect::<Vec<_>>(), vec!["attach-session", "-t", "main:0"]);
}

#[test] fn test_attach_command_inside_tmux() {
    std::env::set_var("TMUX", "/tmp/tmux-1000/default,12345,0");
    let cmd = build_attach_command(&session);
    assert_eq!(cmd.get_args().collect::<Vec<_>>(), vec!["switch-client", "-t", "main:0"]);
}
```

---

## Step 3.4: Status Bar & Help

**Goal**: Show daemon connection status and keys

**Tasks**:
1. Bottom status bar
2. Help overlay on `?`

**Files to modify**:
```
crates/tui/src/ui.rs          # Add status bar and help
crates/tui/src/app.rs         # Add show_help state
```

**Status bar**:
```rust
// ui.rs
fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let (status_icon, status_style) = match &app.state {
        AppState::Connected => ("●", Style::default().fg(Color::Green)),
        AppState::Connecting => ("○", Style::default().fg(Color::Yellow)),
        _ => ("○", Style::default().fg(Color::Red)),
    };

    let status = Line::from(vec![
        Span::raw("  [a]ttach  [k]ill  [?]help  [q]uit"),
        Span::raw("         "),
        Span::styled(format!("daemon: {} ", status_icon), status_style),
        Span::raw(format!("{} sessions", app.sessions.len())),
    ]);

    f.render_widget(Paragraph::new(status), area);
}
```

**Help overlay**:
```rust
fn render_help_overlay(f: &mut Frame, area: Rect) {
    let help_text = vec![
        "  Keyboard Shortcuts",
        "  ──────────────────",
        "  j/↓     Move down",
        "  k/↑     Move up",
        "  a/Enter Attach to session",
        "  k       Kill session",
        "  r       Refresh",
        "  ?       Toggle help",
        "  q       Quit",
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL);

    let help = Paragraph::new(help_text.join("\n"))
        .block(block);

    // Center the help popup
    let popup_area = centered_rect(40, 12, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(help, popup_area);
}
```

**Exit Criteria**:
- [ ] Status bar shows "daemon: ● connected" (green) or "○ disconnected" (red)
- [ ] Key hints visible
- [ ] Session count shown
- [ ] `?` toggles help overlay

**Tests**:
```rust
// Snapshot tests
#[test] fn test_render_status_bar_connected()
#[test] fn test_render_status_bar_disconnected()
#[test] fn test_render_help_overlay()
```

---

# Phase 4: Polish & Integration

## Step 4.1: End-to-End Testing

**Goal**: Full workflow tests with real components

**Tasks**:
1. Test: Start daemon → Start Claude in tmux → Verify discovery → Check TUI shows session
2. Test: Claude requests input → State changes to NEEDS_INPUT → TUI updates
3. Test: Attach from TUI → Lands in correct tmux window
4. Set up CI pipeline

**Files to create**:
```
tests/e2e/mod.rs                       # E2E test harness
tests/e2e/discovery.rs                 # Discovery workflow tests
tests/e2e/state_changes.rs             # State monitoring tests
tests/e2e/attach.rs                    # Attach workflow tests
.github/workflows/ci.yml               # GitHub Actions CI
```

**E2E test harness**:
```rust
// tests/e2e/mod.rs
pub struct TestEnv {
    daemon_handle: JoinHandle<()>,
    tmux_session: String,
    socket_path: PathBuf,
    db_path: PathBuf,
}

impl TestEnv {
    pub async fn setup() -> Self {
        // Create temp directory
        let tmp = tempdir().unwrap();
        let socket_path = tmp.path().join("daemon.sock");
        let db_path = tmp.path().join("sessions.db");

        // Create unique tmux session for this test
        let tmux_session = format!("test-{}", Uuid::new_v4());
        Command::new("tmux")
            .args(["new-session", "-d", "-s", &tmux_session])
            .status()
            .unwrap();

        // Start daemon
        let daemon_handle = tokio::spawn(async move {
            run_daemon(&socket_path, &db_path).await.unwrap();
        });

        Self { daemon_handle, tmux_session, socket_path, db_path }
    }

    pub async fn start_claude_in_window(&self, window: &str) {
        Command::new("tmux")
            .args(["new-window", "-t", &self.tmux_session, "-n", window])
            .status()
            .unwrap();

        // Simulate claude by running a shell that looks like claude
        Command::new("tmux")
            .args(["send-keys", "-t", &format!("{}:{}", self.tmux_session, window),
                   "echo '> '", "Enter"])
            .status()
            .unwrap();
    }

    pub async fn teardown(self) {
        // Kill tmux session
        Command::new("tmux")
            .args(["kill-session", "-t", &self.tmux_session])
            .status()
            .ok();

        // Stop daemon
        self.daemon_handle.abort();
    }
}
```

**Exit Criteria**:
- [ ] E2E test discovers session in tmux
- [ ] E2E test detects state change
- [ ] E2E test verifies TUI shows correct data
- [ ] CI runs on every PR

**Tests**:
```rust
// tests/e2e/discovery.rs
#[tokio::test]
async fn test_full_workflow_discovery() {
    let env = TestEnv::setup().await;

    // Start "claude" in a window
    env.start_claude_in_window("feature-auth").await;

    // Give daemon time to discover
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Connect as client and verify
    let mut client = IpcClient::connect(&env.socket_path).await.unwrap();
    let sessions = client.list_sessions().await.unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].tmux_window, "feature-auth");

    env.teardown().await;
}

// tests/e2e/state_changes.rs
#[tokio::test]
async fn test_full_workflow_state_change() {
    let env = TestEnv::setup().await;
    env.start_claude_in_window("test").await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Simulate claude working (tool output)
    Command::new("tmux")
        .args(["send-keys", "-t", &format!("{}:test", env.tmux_session),
               "echo 'Tool: Reading file'", "Enter"])
        .status()
        .unwrap();

    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut client = IpcClient::connect(&env.socket_path).await.unwrap();
    let sessions = client.list_sessions().await.unwrap();
    assert_eq!(sessions[0].state, SessionState::Working);

    env.teardown().await;
}
```

**CI configuration**:
```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: macos-latest  # Need tmux
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Install tmux
        run: brew install tmux

      - name: Run unit tests
        run: cargo test --workspace --lib

      - name: Run integration tests
        run: cargo test --workspace --test '*'

      - name: Run E2E tests
        run: |
          tmux new-session -d -s ci
          cargo test --test e2e
```

**Validation**:
```bash
cargo test --test e2e
```

---

## Step 4.2: Error Handling & Edge Cases

**Goal**: Robust error handling throughout

**Tasks**:
1. Stale socket cleanup
2. Handle tmux session disappearing
3. Handle DB corruption
4. Graceful degradation when hooks fail
5. TUI error display

**Files to modify**:
```
crates/daemon/src/server.rs     # Socket cleanup
crates/daemon/src/monitor.rs    # Handle missing sessions
crates/lib/src/db/mod.rs  # DB recovery
crates/tui/src/app.rs         # Error state handling
```

**Stale socket cleanup**:
```rust
// server.rs
impl IpcServer {
    pub async fn bind(socket_path: &Path) -> Result<Self> {
        // Try to remove stale socket
        if socket_path.exists() {
            // Check if daemon is actually running
            match UnixStream::connect(socket_path).await {
                Ok(_) => {
                    return Err(Error::DaemonAlreadyRunning);
                }
                Err(_) => {
                    // Stale socket, remove it
                    tracing::warn!("Removing stale socket at {:?}", socket_path);
                    std::fs::remove_file(socket_path)?;
                }
            }
        }

        let listener = UnixListener::bind(socket_path)?;
        Ok(Self { listener, socket_path: socket_path.to_owned() })
    }
}
```

**Session disappearance handling**:
```rust
// monitor.rs
async fn check_sessions(db: &Database, tmux: &impl Tmux, detector: &StateDetector) -> Result<()> {
    for session in db.list_sessions()? {
        // Check if tmux window still exists
        let windows = tmux.list_windows(&session.tmux_session).await?;
        let exists = windows.iter().any(|w| w.name == session.tmux_window);

        if !exists {
            tracing::info!("Session {} tmux window no longer exists, removing", session.id);
            db.delete_session(session.id)?;
            continue;
        }

        // ... rest of monitoring logic
    }
    Ok(())
}
```

**Exit Criteria**:
- [ ] Daemon starts even if stale socket exists
- [ ] Session auto-removed when tmux window closes
- [ ] TUI shows "Session no longer exists" if selected session disappears
- [ ] DB recreated if corrupted (with warning)

**Tests**:
```rust
#[tokio::test] async fn test_stale_socket_cleanup()
#[tokio::test] async fn test_session_removed_when_window_closes()
#[tokio::test] async fn test_db_recovery_on_corruption()
#[test] fn test_tui_handles_session_disappearing()
```

---

## Step 4.3: Documentation & Install

**Goal**: Ready for others to use

**Tasks**:
1. README with installation
2. Install script
3. Configuration docs
4. Troubleshooting guide

**Files to create**:
```
README.md                              # Main documentation
docs/configuration.md                  # Config options
docs/troubleshooting.md               # Common issues
scripts/install.sh                     # Install script
scripts/uninstall.sh                   # Uninstall script
```

**README.md**:
```markdown
# Claude Admin

Terminal-based session manager for Claude Code.

## Features

- Track multiple Claude sessions across tmux
- Visual status indicators (Working, Needs Input, Done)
- Quick attach to any session
- State change detection via hooks

## Installation

```bash
# Clone and install
git clone https://github.com/you/claude-admin
cd claude-admin
./scripts/install.sh
```

## Usage

```bash
# Start the daemon
daemon

# Open the TUI
claude-admin
```

## Key Bindings

| Key | Action |
|-----|--------|
| j/↓ | Move down |
| k/↑ | Move up |
| a/Enter | Attach to session |
| q | Quit |
| ? | Help |

## Configuration

See [docs/configuration.md](docs/configuration.md)

## Troubleshooting

See [docs/troubleshooting.md](docs/troubleshooting.md)
```

**Install script**:
```bash
#!/bin/bash
# scripts/install.sh

set -e

echo "Building claude-admin..."
cargo build --release

echo "Installing binaries..."
mkdir -p ~/.local/bin
cp target/release/daemon ~/.local/bin/
cp target/release/claude-admin ~/.local/bin/

echo "Installing hooks..."
mkdir -p ~/.claude-admin/hooks
cp scripts/claude-admin-hook.sh ~/.claude-admin/hooks/
chmod +x ~/.claude-admin/hooks/claude-admin-hook.sh

echo "Configuring Claude hooks..."
# Merge hooks into ~/.claude/settings.json
./scripts/configure-hooks.sh

echo ""
echo "Installation complete!"
echo ""
echo "Add to your shell profile:"
echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
echo ""
echo "Then start the daemon:"
echo "  daemon &"
echo ""
echo "And open the TUI:"
echo "  claude-admin"
```

**Exit Criteria**:
- [ ] Fresh machine install works
- [ ] README explains all features
- [ ] Configuration documented
- [ ] Common issues have solutions

---

# TUI Mock

```
┌─────────────────────────────────────────────────────────────────┐
│  claude-admin                                      daemon: ●    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  claude_admin/                                                  │
│  ├─▶[●] feature/auth    WORKING    "Implementing OAuth..."     │
│  ├─ [!] feature/api     INPUT      "Waiting for input"         │
│  └─ [✓] feature/ui      DONE       "Completed 2m ago"          │
│                                                                 │
│  other-project/                                                 │
│  └─ [ ] main            IDLE                                   │
│                                                                 │
│                                                                 │
│                                                                 │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│  [a]ttach  [n]ew  [k]ill  [?]help  [q]uit         4 sessions   │
└─────────────────────────────────────────────────────────────────┘
```

Legend:
- `●` WORKING (blue)
- `!` NEEDS_INPUT (yellow)
- `✓` DONE (green)
- ` ` IDLE (gray)
- `▶` Selected

---

# Summary

| Phase | Steps | Focus |
|-------|-------|-------|
| 1. Foundation | 1.1-1.4 | Project setup, models, DB, tmux, IPC |
| 2. Daemon | 2.1-2.4 | Lifecycle, discovery, monitoring, hooks |
| 3. TUI | 3.1-3.4 | Framework, list, attach, polish |
| 4. Polish | 4.1-4.3 | E2E tests, error handling, docs |

Total: 15 steps across 4 phases
