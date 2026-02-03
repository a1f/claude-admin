# Step 2.2-2.3: Session Discovery & Monitoring

## Overview

Add session discovery and monitoring to the claude_admin daemon with:
- **Hooks primary**: Real-time state updates from Claude events
- **Polling backup**: 5-second interval for discovery and cleanup

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        DAEMON                                │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │  Discovery  │    │   Polling   │    │    Hooks    │     │
│  │  (startup)  │    │  (5s loop)  │    │  (realtime) │     │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘     │
│         │                  │                  │             │
│         └──────────────────┴──────────────────┘             │
│                            │                                 │
│                    ┌───────▼───────┐                        │
│                    │   Database    │                        │
│                    │  (sessions,   │                        │
│                    │   events)     │                        │
│                    └───────────────┘                        │
└─────────────────────────────────────────────────────────────┘
                             ▲
                             │ Hook Events
                             │ (PreToolUse, PostToolUse, etc.)
┌─────────────────────────────────────────────────────────────┐
│                    CLAUDE SESSIONS                           │
│   tmux:0 (claude)    tmux:1 (claude)    tmux:2 (claude)    │
└─────────────────────────────────────────────────────────────┘
```

---

## Chapter 1: Database Schema & Models

### Step 1.1: Define Models
**Files:** Create `crates/daemon/src/models.rs`

```rust
pub enum SessionState { Idle, Working, NeedsInput, Done }

pub struct Session {
    id, pane_id, session_name, window_index, pane_index,
    working_dir, state, detection_method, last_activity,
    created_at, updated_at
}
```

**Exit:** Models compile, serialize/deserialize correctly

### Step 1.2: Create Database Schema
**Files:** Modify `crates/daemon/src/db.rs`

Add tables: `sessions`, `events` with indexes

**Exit:** `sqlite3 ~/.claude-admin/sessions.db ".schema"` shows tables

### Step 1.3: Add Session CRUD
**Files:** Modify `crates/daemon/src/db.rs`

Add: `create_session`, `get_session`, `get_session_by_pane`, `update_session`, `update_session_state`, `list_sessions`, `delete_session`

**Exit:** All CRUD operations pass unit tests

### Step 1.4: Add Event Logging
**Files:** Create `crates/daemon/src/events.rs`, modify `db.rs`

```rust
pub enum EventType {
    SessionDiscovered, SessionRemoved,
    StateChanged { from, to }, HookReceived { hook_type }
}
```

Add: `log_event`, `get_events`, `get_recent_events`

**Exit:** Events logged and retrieved correctly

---

## Chapter 2: Session Discovery

### Step 2.1: Create Discovery Module
**Files:** Create `crates/daemon/src/discovery.rs`

```rust
impl SessionDiscovery {
    fn discover_sessions() -> Vec<Session>
    fn is_claude_process(name: &str) -> bool  // version pattern "2.1.20"
    fn create_session_from_pane(pane: &TmuxPane) -> Session
}
```

**Exit:** Discovers Claude sessions from tmux, ignores non-Claude panes

### Step 2.2: Integrate Discovery on Startup
**Files:** Modify `crates/daemon/src/main.rs`

Call discovery after DB init, log discovered sessions, create DB records

**Exit:** Daemon logs discovered sessions, DB populated on startup

### Step 2.3: Initial State Detection
**Files:** Modify `crates/daemon/src/discovery.rs`

Use `state::detect_state()` to set initial session state from pane content

**Exit:** Discovered sessions have appropriate initial state

---

## Chapter 3: State Detection Logic

### Step 3.1: Create State Module
**Files:** Create `crates/daemon/src/state.rs`

```rust
pub fn detect_state(content: &str) -> SessionState

fn is_working(content) -> bool   // "Tool:", "Reading", "Writing", "╭─"
fn is_needs_input(content) -> bool  // ends with ">", "?", "Approve?"
fn is_done(content) -> bool      // "Session ended", "Goodbye"
```

**Exit:** Correctly identifies all 4 states from pane content

### Step 3.2: Add State Detection Tests
**Files:** Modify `crates/daemon/src/state.rs`

Test cases for: tool calls, prompts, approval dialogs, welcome screen, exit

**Exit:** All state detection tests pass

---

## Chapter 4: Background Polling Loop

### Step 4.1: Create Polling Module Structure
**Files:** Create `crates/daemon/src/polling.rs`

```rust
impl PollingLoop {
    fn new(db, interval_secs: 5)
    async fn run(shutdown: Receiver<()>)
    async fn poll_once()
}
```

**Exit:** Polling loop compiles, graceful shutdown works

### Step 4.2: Implement Discovery Polling
**Files:** Modify `crates/daemon/src/polling.rs`

Add `discover_new_sessions()` - find new Claude panes, create DB records

**Exit:** New sessions detected within 5 seconds

### Step 4.3: Implement Dead Session Cleanup
**Files:** Modify `crates/daemon/src/polling.rs`

Add `cleanup_dead_sessions()` - remove sessions when tmux pane closes

**Exit:** Sessions removed when pane closes, logged to events

### Step 4.4: Implement State Update Polling
**Files:** Modify `crates/daemon/src/polling.rs`

Add `update_stale_states()` - update state for sessions without recent hook activity (>10s)

**Exit:** State changes detected via polling, logged

### Step 4.5: Integrate Polling into Main
**Files:** Modify `crates/daemon/src/main.rs`

Spawn polling task, wire up shutdown signal

**Exit:** Polling runs in background, stops on shutdown

---

## Chapter 5: Claude Hooks Integration

### Step 5.1: Extend IPC Protocol
**Files:** Modify `crates/daemon/src/socket.rs`

Add `HookEvent` and `HookAck` message types

**Exit:** HookEvent parses correctly

### Step 5.2: Create Hooks Handler Module
**Files:** Create `crates/daemon/src/hooks.rs`

```rust
impl HookHandler {
    fn handle_event(event: &HookEvent)
    fn infer_state_from_hook(event) -> SessionState
    fn find_session_by_cwd(cwd: &str) -> Option<Session>
}
```

**Exit:** Hook events update session state, logged to DB

### Step 5.3: Update Socket Handler
**Files:** Modify `crates/daemon/src/socket.rs`, `main.rs`

Route HookEvent messages to HookHandler

**Exit:** Hook events received via socket, ack sent back

### Step 5.4: Create Hook Script
**Files:** Create `scripts/claude-admin-hook.sh`

```bash
#!/bin/bash
# Send JSON to daemon socket
echo '{"type":"hook_event",...}' | nc -U ~/.claude-admin/daemon.sock
```

**Exit:** Script executable, sends to socket, exits 0 if daemon down

### Step 5.5: Create Hook Installation Script
**Files:** Create `scripts/install-hooks.sh`

Installs hook script, merges into `~/.claude/settings.json`

**Exit:** Hooks merged into settings, backup created

### Step 5.6: Auto-Install Hooks on First Run
**Files:** Create `crates/daemon/src/hooks/install.rs`, modify `main.rs`

Check if hooks installed, auto-install on first daemon run

**Exit:** First run installs hooks, subsequent runs skip

---

## Chapter 6: IPC Protocol Extension

### Step 6.1: Add Session Query Messages
**Files:** Modify `crates/daemon/src/socket.rs`

Add: `ListSessions`, `SessionList`, `GetSession`, `SessionResponse`, `GetRecentEvents`, `EventList`

**Exit:** Message types serialize correctly

### Step 6.2: Implement Session Query Handlers
**Files:** Modify `crates/daemon/src/socket.rs`

Handle ListSessions, GetSession, GetSessionByPane, GetRecentEvents

**Exit:** Queries return correct data

### Step 6.3: Create CLI Client for Testing
**Files:** Create `crates/daemon/src/bin/admin_client.rs`

Simple client: `admin_client list`, `admin_client ping`, `admin_client events`

**Exit:** Client can query daemon and display results

---

## Implementation Order

```
Chapter 1 (DB Schema) ──┬──> Chapter 2 (Discovery) ──┐
                        │                             │
                        └──> Chapter 3 (State Det.) ──┼──> Chapter 4 (Polling)
                        │                             │
                        └──> Chapter 5 (Hooks) ───────┘
                        │
                        └──> Chapter 6 (IPC Extension)
```

**Recommended sequence:**
1. Chapter 1 (foundation)
2. Chapter 3 (state detection - no deps)
3. Chapter 2 (discovery)
4. Chapter 4 (polling)
5. Chapter 5 (hooks) - can parallel with 4
6. Chapter 6 (IPC for TUI prep)

---

## Critical Files

| File | Action | Purpose |
|------|--------|---------|
| `crates/daemon/src/models.rs` | Create | Session, SessionState, Event types |
| `crates/daemon/src/db.rs` | Modify | Schema, CRUD, event logging |
| `crates/daemon/src/state.rs` | Create | State detection patterns |
| `crates/daemon/src/discovery.rs` | Create | Tmux scanning, session creation |
| `crates/daemon/src/polling.rs` | Create | 5-second background loop |
| `crates/daemon/src/hooks.rs` | Create | Hook event handling |
| `crates/daemon/src/socket.rs` | Modify | IPC protocol extension |
| `crates/daemon/src/main.rs` | Modify | Wire up all components |
| `scripts/claude-admin-hook.sh` | Create | Hook script for Claude |
| `scripts/install-hooks.sh` | Create | Hook installation |

---

## Verification

After each chapter:

```bash
# Build
cargo build -p daemon

# Run tests
cargo test -p daemon

# Run daemon
cargo run -p daemon

# Test discovery (Chapter 2)
cargo run --bin scan_panes

# Test IPC (Chapter 6)
cargo run --bin admin_client list
cargo run --bin admin_client events
```

End-to-end test:
1. Start daemon: `cargo run -p daemon`
2. Open Claude in tmux
3. Verify discovered: `cargo run --bin admin_client list`
4. Use a tool in Claude
5. Verify state change: `cargo run --bin admin_client events`
