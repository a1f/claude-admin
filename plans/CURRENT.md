# Unified claude-admin: M0 + M1 Implementation Plan

## Context

Combine claude_admin (Rust daemon, 101 tests) and dacm (Tauri app, designs only) into a single terminal-first session manager. Each step below = 1 commit. M0 complete before M1 starts.

**Stack:** Rust, Tokio, rusqlite, ratatui, crossterm, Unix sockets
**Shared crate:** `ca-lib` (avoids Rust `core` conflict)

---

## M0: Core Session Management (Tmux Dashboard)

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.1  | Extract ca-lib crate          | Done    | C: crates/ca-lib/{Cargo.toml,src/lib.rs}    | cargo build --workspace                       | No logic changes, pure move
       |                               |         |    src/{models,events,db,tmux,state,config}  | cargo test --workspace (101 tests pass)       | All imports resolve
       |                               |         | M: Cargo.toml (workspace members)           |                                               | No duplicated deps between ca-lib/daemon
       |                               |         | M: crates/daemon/Cargo.toml (use ca-lib dep) |                                               |
       |                               |         | M: crates/daemon/src/main.rs (use ca_lib::)  |                                               |
       |                               |         | M: crates/daemon/src/bin/scan_panes.rs       |                                               |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.2  | Session discovery module      | Done | C: crates/ca-lib/src/discovery.rs            | cargo test -p ca-lib discovery::tests         | is_claude_process covers: "claude",
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        | >= 8 unit tests                               |   "node", "deno", version patterns
       |                               |         |                                             |                                               | discover creates DB records correctly
       |                               |         |                                             |                                               | cleanup removes stale sessions
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.3  | Polling loop                  | Pending | C: crates/daemon/src/polling.rs              | cargo build -p daemon                         | Shutdown signal respected (no hangs)
       |                               |         | M: crates/daemon/src/main.rs (spawn task)    | Start daemon + open Claude in tmux            | 5s interval, no busy-wait
       |                               |         |                                             | Check daemon.log for discovery messages        | State changes logged as events
       |                               |         |                                             | Close Claude, check cleanup messages           |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.4  | Expand IPC protocol           | Pending | C: crates/ca-lib/src/ipc.rs                  | cargo test --workspace                        | Request/Response serde round-trips
       |                               |         | M: crates/daemon/src/socket.rs (use ipc)     | echo '{"type":"list_sessions"}' | nc -U sock  | IpcClient reconnect/error handling
       |                               |         | M: crates/daemon/src/main.rs (Arc<Database>) | Verify JSON response with sessions            | No unwrap in handler dispatch
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod ipc)    |                                               |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.5  | Hooks handler + shell script  | Pending | C: crates/ca-lib/src/hooks.rs                | cargo test -p ca-lib hooks::tests             | infer_state_from_hook mapping complete
       |                               |         | C: scripts/claude-admin-hook.sh              | echo hook JSON | nc -U sock                   | find_session_for_hook by working_dir
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod hooks)  | Check state updated in DB                     | Shell script: exit 0 even if daemon down
       |                               |         |                                             |                                               | Script is chmod +x
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.6  | CLI crate (basic commands)    | Pending | C: crates/cli/{Cargo.toml,src/main.rs}       | cargo run -p cli -- ping                      | Graceful "daemon not running" message
       |                               |         | M: Cargo.toml (workspace members)            | cargo run -p cli -- list                      | Consistent output formatting
       |                               |         |                                             | cargo run -p cli -- status                    | Clap subcommands: status,list,events,
       |                               |         |                                             | cargo run -p cli -- events --limit 5          |   ping,daemon {start,stop}
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.7  | TUI scaffold (ratatui)        | Pending | C: crates/tui/{Cargo.toml,src/main.rs}       | cargo run -p tui                              | Terminal restore on panic (crossterm)
       |                               |         | C: crates/tui/src/app.rs                     | Sessions listed with colored states            | Event loop: key events + tick timer
       |                               |         | C: crates/tui/src/ui.rs                      | j/k navigation works                          | App state: sessions, selected_index
       |                               |         | M: Cargo.toml (workspace members)            | Enter/a attaches (tmux attach)                | Clean separation: app.rs=state, ui.rs=render
       |                               |         |                                             | q quits cleanly                               |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.8  | TUI output preview pane       | Pending | M: crates/tui/src/app.rs (preview state)     | Select session, preview shows pane content    | Preview refreshes on selection change
       |                               |         | M: crates/tui/src/ui.rs (right panel)        | Content updates every 2s                      | No flicker on refresh
       |                               |         |                                             | Layout: 60/40 split left/right                | Handles empty/no-session gracefully
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.9  | TUI real-time IPC subscriptions| Pending | M: crates/daemon/src/socket.rs (subscribers) | Open TUI, start Claude in tmux                | Subscriber cleanup on disconnect
       |                               |         | M: crates/tui/src/app.rs (Subscribe + push)  | State change appears in TUI within 1s         | tokio::select! in TUI event loop
       |                               |         |                                             | No manual refresh needed                      | Broadcast doesn't block daemon
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.10 | Hook install CLI command      | Pending | C: crates/ca-lib/src/hooks/install.rs        | ca hooks install -> modifies settings.json    | Reads existing settings.json safely
       |                               |         | M: crates/cli/src/main.rs (hooks subcommand) | ca hooks status -> shows "installed"          | Merges hooks, preserves other settings
       |                               |         |                                             | ca hooks uninstall -> removes entries         | Idempotent (re-install = no-op)
       |                               |         |                                             | Run again -> "already installed"              | Script copied + chmod +x
```

**M0 Exit Criteria:** Start daemon, open Claude in tmux, run TUI. Sessions appear with live state updates. Enter attaches. Hooks forward events. CLI queries work.

---

## M1: Plan & Task System (Semi-Auto Orchestration)

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.1  | Workspaces table + CRUD       | Pending | C: crates/ca-lib/src/workspace.rs            | cargo test -p ca-lib workspace::tests         | Path uniqueness enforced
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 8 tests: create, get, get_by_path,         | Name auto-derived from path dirname
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   list, delete, duplicate path error          | Timestamps set correctly
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.2  | Projects table + CRUD         | Pending | C: crates/ca-lib/src/project.rs              | cargo test -p ca-lib project::tests           | FK to workspaces with CASCADE delete
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 10 tests: CRUD + cascade delete +          | Status enum: active,running,completed,
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   list_by_workspace + archive                 |   archived
       |                               |         |                                             |                                               | worktree_path/branch_name nullable
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.3  | Plans table + PlanContent JSON| Pending | C: crates/ca-lib/src/plan.rs                 | cargo test -p ca-lib plan::tests              | PlanContent JSON round-trip fidelity
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 15 tests: CRUD + JSON round-trips +        | update_step_status: deserialize->find->
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   step status update + missing step error     |   update->serialize->save
       |                               |         |                                             |   + active plan query                         | StepStatus/PlanStatus enums with serde
       |                               |         |                                             |                                               | ExitCriteria: commands[] + description
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.4  | Schema migrations + session   | Pending | C: crates/ca-lib/src/migrations.rs           | cargo test --workspace (all existing pass)    | schema_version table for tracking
       | linking to projects           |         | M: crates/ca-lib/src/db.rs (call migrations) | New DB: has project_id/plan_step_id cols      | ALTER TABLE sessions ADD COLUMN safe
       |                               |         | M: crates/ca-lib/src/models.rs (add fields)  | Existing DB: migrates without data loss       | Optional fields (Option<i64>, Option<String>)
       |                               |         |                                             | Session CRUD handles new nullable fields      |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.5  | Workspace/project/plan CLI    | Pending | M: crates/cli/src/main.rs (add subcommands)  | ca workspace add ~/dev/myapp                  | Direct DB access (not IPC) for CLI
       |                               |         |                                             | ca workspace list                             | --db-path flag with default
       |                               |         |                                             | ca project create 1 "auth feature"            | Plan loaded from JSON file (--file)
       |                               |         |                                             | ca plan create 1 "Auth" --file plan.json      | Validates JSON before insert
       |                               |         |                                             | ca plan step 1 "0.1" completed                | Human-readable output tables
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.6  | Workspace/project/plan IPC    | Pending | M: crates/ca-lib/src/ipc.rs (new variants)   | cargo test --workspace                        | All new Request/Response variants
       |                               |         | M: crates/daemon/src/socket.rs (handlers)    | IPC round-trip for each new message type      |   serialize correctly
       |                               |         |                                             |                                               | Handler dispatch covers all variants
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.7  | TUI plan viewer               | Pending | C: crates/tui/src/plan_view.rs               | Open TUI, navigate to plan view               | Phase headers collapsible
       |                               |         | M: crates/tui/src/app.rs (ViewMode enum)     | Steps render with status indicators           | Step status indicators: o * v x -
       |                               |         | M: crates/tui/src/ui.rs (route views)        | s key cycles status, persists to DB           | Progress counter per phase
       |                               |         |                                             | b key returns to session list                 | b/Enter navigation between views
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.8  | Session spawn with plan       | Pending | C: crates/ca-lib/src/spawn.rs                | ca spawn 1 --step 0.1                         | generate_plan_context output format:
       | context injection             |         | M: crates/cli/src/main.rs (spawn subcommand) | New tmux window opens with Claude             |   goal, progress, current step,
       |                               |         |                                             | Claude receives plan context as prompt         |   completed/remaining, exit criteria
       |                               |         |                                             | Session registered with project_id +           | tmux new-window + send-keys
       |                               |         |                                             |   plan_step_id in DB                          | Temp file for long context
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.9  | Batch execution               | Pending | C: crates/ca-lib/src/orchestrator.rs         | ca batch 1 --steps 0.1,0.2,0.3 --max 2       | Max concurrency respected
       |                               |         | M: crates/cli/src/main.rs (batch subcommand) | 2 tmux windows open (not 3)                   | Each session gets unique step context
       |                               |         |                                             | Steps marked InProgress in DB                 | suggest_parallelizable_steps: no file
       |                               |         |                                             | ca batch 1 --auto suggests groups             |   overlap = parallelizable
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.10 | TUI orchestration view        | Pending | C: crates/tui/src/project_view.rs            | Open TUI, select project                      | Split view: steps left, sessions right
       |                               |         | M: crates/tui/src/app.rs (project view mode) | Plan steps + active sessions side by side     | s spawns session for selected step
       |                               |         | M: crates/tui/src/ui.rs (route to view)      | s spawns, a attaches, b batches               | Session-to-step linking visible
       |                               |         |                                             | State changes reflect in both panels          | Tab switches between panels
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.11 | Worktree management           | Pending | C: crates/ca-lib/src/git.rs                  | ca workspace add ~/dev/myapp                  | is_git_repo check before worktree ops
       |                               |         | M: crates/ca-lib/src/project.rs (integrate)  | ca project create 1 "auth"                    | Worktree path: {repo}-worktrees/{proj}/
       |                               |         | M: crates/ca-lib/src/spawn.rs (use worktree) | ls ~/dev/myapp-worktrees/auth/  (exists)      | spawn uses worktree_path when set
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod git)    | git -C ~/dev/myapp worktree list              | Archive/delete removes worktree
       |                               |         |                                             | ca project archive 1 -> worktree removed      |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.12 | Settings key-value store      | Pending | C: crates/ca-lib/src/settings.rs             | cargo test -p ca-lib settings::tests          | UPSERT via INSERT OR REPLACE
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | Defaults populated on first init              | ensure_defaults idempotent
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        | get/set/list all work                         | Settings: poll_interval, max_sessions,
       |                               |         |                                             |                                               |   worktree patterns, notifications
```

**M1 Exit Criteria:** Create workspace + project + plan via CLI. View plan in TUI. Batch spawn 3 steps -> 3 tmux windows with Claude, each with plan context. Sessions complete -> steps update. Restart daemon -> everything persists.

---

## Execution Order

```
M0.1 -> M0.2 -> M0.3 -> M0.4 -> M0.5 -> M0.6 -> M0.7 -> M0.8 -> M0.9 -> M0.10
                                                    (M0 complete, commit & verify)
M1.1 -> M1.2 -> M1.3 -> M1.4 -> M1.12 -> M1.5 -> M1.6 -> M1.7 -> M1.11 -> M1.8 -> M1.9 -> M1.10
                                                    (M1 complete, commit & verify)
```

22 steps. 22 commits.
