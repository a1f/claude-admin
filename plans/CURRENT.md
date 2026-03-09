# Claude Admin — Implementation Plan

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
 M0.3  | Polling loop                  | Done | C: crates/daemon/src/polling.rs              | cargo build -p daemon                         | Shutdown signal respected (no hangs)
       |                               |         | M: crates/daemon/src/main.rs (spawn task)    | Start daemon + open Claude in tmux            | 5s interval, no busy-wait
       |                               |         |                                             | Check daemon.log for discovery messages        | State changes logged as events
       |                               |         |                                             | Close Claude, check cleanup messages           |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.4  | Expand IPC protocol           | Done | C: crates/ca-lib/src/ipc.rs                  | cargo test -p ca-lib ipc::tests               | Request/Response serde round-trips
       |                               |         | M: crates/daemon/src/socket.rs (use ipc)     | cargo test -p daemon socket::tests            | IpcClient reconnect/error handling
       |                               |         | M: crates/daemon/src/main.rs (Arc<Database>) | >= 12 tests: serde round-trips for all        | No unwrap in handler dispatch
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod ipc)    |   Request/Response variants, handler          |
       |                               |         |                                             |   dispatch returns correct response types,    |
       |                               |         |                                             |   socket integration test (ping + list)       |
       |                               |         |                                             | Manual: echo JSON | nc -U sock                |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.5  | Hooks handler + shell script  | Done | C: crates/ca-lib/src/hooks.rs                | cargo test -p ca-lib hooks::tests             | infer_state_from_hook mapping complete
       |                               |         | C: scripts/claude-admin-hook.sh              | >= 10 tests: infer_state for all hook types,  | find_session_for_hook by working_dir
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod hooks)  |   HookEvent serde round-trips,               | Shell script: exit 0 even if daemon down
       |                               |         |                                             |   find_session_for_hook (match/no-match),     | Script is chmod +x
       |                               |         |                                             |   apply_hook_event updates DB state           |
       |                               |         |                                             | Manual: echo hook JSON | nc -U sock           |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.6  | CLI crate (basic commands)    | Done | C: crates/cli/{Cargo.toml,src/main.rs}       | cargo test -p cli                             | Graceful "daemon not running" message
       |                               |         | M: Cargo.toml (workspace members)            | >= 6 tests: clap arg parsing validates        | Consistent output formatting
       |                               |         |                                             |   subcommands, output formatting helpers,     | Clap subcommands: status,list,events,
       |                               |         |                                             |   IpcClient error handling (connection        |   ping,daemon {start,stop}
       |                               |         |                                             |   refused → graceful message)                 |
       |                               |         |                                             | Manual: cargo run -p cli -- ping/list/status  |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.7  | TUI scaffold (ratatui)        | Done | C: crates/tui/{Cargo.toml,src/main.rs}       | cargo test -p tui app::tests                  | Terminal restore on panic (crossterm)
       |                               |         | C: crates/tui/src/app.rs                     | >= 8 tests: App state transitions,            | Event loop: key events + tick timer
       |                               |         | C: crates/tui/src/ui.rs                      |   select_next/prev wraps correctly,           | App state: sessions, selected_index
       |                               |         | M: Cargo.toml (workspace members)            |   handle_key for j/k/q/Enter, empty list      | Clean separation: app.rs=state, ui.rs=render
       |                               |         |                                             |   edge case, session list update              |
       |                               |         |                                             | Manual: cargo run -p tui, j/k/Enter/q         |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.8  | TUI output preview pane       | Done | M: crates/tui/src/app.rs (preview state)     | cargo test -p tui                             | Preview refreshes on selection change
       |                               |         | M: crates/tui/src/ui.rs (right panel)        | >= 4 tests: preview state on selection         | No flicker on refresh
       |                               |         |                                             |   change, empty/no-session returns None,      | Handles empty/no-session gracefully
       |                               |         |                                             |   preview cleared on session removal          |
       |                               |         |                                             | Manual: select session, verify 60/40 split    |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.9  | TUI real-time IPC subscriptions| Done | M: crates/daemon/src/socket.rs (subscribers) | cargo test -p daemon socket::tests            | Subscriber cleanup on disconnect
       |                               |         | M: crates/tui/src/app.rs (Subscribe + push)  | >= 6 tests: subscriber add/remove,            | tokio::select! in TUI event loop
       |                               |         |                                             |   broadcast delivery to multiple subs,        | Broadcast doesn't block daemon
       |                               |         |                                             |   subscriber cleanup on disconnect,           |
       |                               |         |                                             |   broadcast with no subscribers is no-op      |
       |                               |         |                                             | Manual: TUI + Claude, verify live updates     |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M0.10 | Hook install CLI command      | Done | C: crates/ca-lib/src/hook_install.rs         | cargo test -p ca-lib hook_install::tests      | Reads existing settings.json safely
       |                               |         | M: crates/cli/src/main.rs (hooks subcommand) | >= 8 tests: install into empty dir,           | Merges hooks, preserves other settings
       |                               |         |                                             |   install merges with existing settings,      | Idempotent (re-install = no-op)
       |                               |         |                                             |   idempotent re-install, uninstall removes,   | Script copied + chmod +x
       |                               |         |                                             |   uninstall when not installed, status check, |
       |                               |         |                                             |   preserves non-hook settings in JSON         |
```

**M0 Exit Criteria:** Start daemon, open Claude in tmux, run TUI. Sessions appear with live state updates. Enter attaches. Hooks forward events. CLI queries work.

---

## M1: Plan & Task System (Semi-Auto Orchestration)

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.1  | Workspaces table + CRUD       | Done    | C: crates/ca-lib/src/workspace.rs            | cargo test -p ca-lib workspace::tests         | Path uniqueness enforced
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 8 tests: create, get, get_by_path,         | Name auto-derived from path dirname
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   list, delete, duplicate path error          | Timestamps set correctly
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.2  | Projects table + CRUD         | Done    | C: crates/ca-lib/src/project.rs              | cargo test -p ca-lib project::tests           | FK to workspaces with CASCADE delete
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 10 tests: CRUD + cascade delete +          | Status enum: active,running,completed,
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   list_by_workspace + archive                 |   archived
       |                               |         |                                             |                                               | worktree_path/branch_name nullable
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.3  | Plans table + PlanContent JSON| Done    | C: crates/ca-lib/src/plan.rs                 | cargo test -p ca-lib plan::tests              | PlanContent JSON round-trip fidelity
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | >= 15 tests: CRUD + JSON round-trips +        | update_step_status: deserialize->find->
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   step status update + missing step error     |   update->serialize->save
       |                               |         |                                             |   + active plan query                         | StepStatus/PlanStatus enums with serde
       |                               |         |                                             |                                               | ExitCriteria: commands[] + description
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.4  | Schema migrations + session   | Done    | C: crates/ca-lib/src/migrations.rs           | cargo test --workspace (all existing pass)    | schema_version table for tracking
       | linking to projects           |         | M: crates/ca-lib/src/db.rs (call migrations) | New DB: has project_id/plan_step_id cols      | ALTER TABLE sessions ADD COLUMN safe
       |                               |         | M: crates/ca-lib/src/models.rs (add fields)  | Existing DB: migrates without data loss       | Optional fields (Option<i64>, Option<String>)
       |                               |         |                                             | Session CRUD handles new nullable fields      |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.5  | Workspace/project/plan CLI    | Done    | M: crates/cli/src/main.rs (add subcommands)  | ca workspace add ~/dev/myapp                  | Direct DB access (not IPC) for CLI
       |                               |         |                                             | ca workspace list                             | --db-path flag with default
       |                               |         |                                             | ca project create 1 "auth feature"            | Plan loaded from JSON file (--file)
       |                               |         |                                             | ca plan create 1 "Auth" --file plan.json      | Validates JSON before insert
       |                               |         |                                             | ca plan step 1 "0.1" completed                | Human-readable output tables
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.6  | Workspace/project/plan IPC    | Done    | M: crates/ca-lib/src/ipc.rs (new variants)   | cargo test --workspace                        | All new Request/Response variants
       |                               |         | M: crates/daemon/src/socket.rs (handlers)    | IPC round-trip for each new message type      |   serialize correctly
       |                               |         |                                             |                                               | Handler dispatch covers all variants
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.7  | TUI plan viewer               | Done    | C: crates/tui/src/plan_view.rs               | Open TUI, navigate to plan view               | Phase headers collapsible
       |                               |         | M: crates/tui/src/app.rs (ViewMode enum)     | Steps render with status indicators           | Step status indicators: o * v x -
       |                               |         | M: crates/tui/src/ui.rs (route views)        | s key cycles status, persists to DB           | Progress counter per phase
       |                               |         |                                             | b key returns to session list                 | b/Enter navigation between views
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.8  | Session spawn with plan       | Done    | C: crates/ca-lib/src/spawn.rs                | ca spawn 1 --step 0.1                         | generate_plan_context output format:
       | context injection             |         | M: crates/cli/src/main.rs (spawn subcommand) | New tmux window opens with Claude             |   goal, progress, current step,
       |                               |         |                                             | Claude receives plan context as prompt         |   completed/remaining, exit criteria
       |                               |         |                                             | Session registered with project_id +           | tmux new-window + send-keys
       |                               |         |                                             |   plan_step_id in DB                          | Temp file for long context
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.9  | Batch execution               | Done    | C: crates/ca-lib/src/orchestrator.rs         | ca batch 1 --steps 0.1,0.2,0.3 --max 2       | Max concurrency respected
       |                               |         | M: crates/cli/src/main.rs (batch subcommand) | 2 tmux windows open (not 3)                   | Each session gets unique step context
       |                               |         |                                             | Steps marked InProgress in DB                 | suggest_parallelizable_steps: no file
       |                               |         |                                             | ca batch 1 --auto suggests groups             |   overlap = parallelizable
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.10 | TUI orchestration view        | Done    | C: crates/tui/src/project_view.rs            | Open TUI, select project                      | Split view: steps left, sessions right
       |                               |         | M: crates/tui/src/app.rs (project view mode) | Plan steps + active sessions side by side     | s spawns session for selected step
       |                               |         | M: crates/tui/src/ui.rs (route to view)      | s spawns, a attaches, b batches               | Session-to-step linking visible
       |                               |         |                                             | State changes reflect in both panels          | Tab switches between panels
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.11 | Worktree management           | Done    | C: crates/ca-lib/src/git.rs                  | ca workspace add ~/dev/myapp                  | is_git_repo check before worktree ops
       |                               |         | M: crates/ca-lib/src/project.rs (integrate)  | ca project create 1 "auth"                    | Worktree path: {repo}-worktrees/{proj}/
       |                               |         | M: crates/ca-lib/src/spawn.rs (use worktree) | ls ~/dev/myapp-worktrees/auth/  (exists)      | spawn uses worktree_path when set
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod git)    | git -C ~/dev/myapp worktree list              | Archive/delete removes worktree
       |                               |         |                                             | ca project archive 1 -> worktree removed      |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M1.12 | Settings key-value store      | Done    | C: crates/ca-lib/src/settings.rs             | cargo test -p ca-lib settings::tests          | UPSERT via INSERT OR REPLACE
       |                               |         | M: crates/ca-lib/src/db.rs (schema)          | Defaults populated on first init              | ensure_defaults idempotent
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        | get/set/list all work                         | Settings: poll_interval, max_sessions,
       |                               |         |                                             |                                               |   worktree patterns, notifications
```

**M1 Exit Criteria:** Create workspace + project + plan via CLI. View plan in TUI. Batch spawn 3 steps -> 3 tmux windows with Claude, each with plan context. Sessions complete -> steps update. Restart daemon -> everything persists.

---

## M2: TUI Interactivity & CRUD  ✓

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.1  | InputMode enum + TextInput    | Done | C: crates/tui/src/input.rs                  | cargo test -p tui input::tests                | q/Esc only fires in Normal mode
       | widget                        |         | M: crates/tui/src/app.rs (InputMode enum)   | >= 10 tests: insert_char, delete_char,        | Esc in non-Normal → back to Normal
       |                               |         | M: crates/tui/src/main.rs (gate keys)       |   backspace, cursor bounds, move_home/end,    | cursor_pos never exceeds value.len()
       |                               |         |                                             |   clear, InputMode gating (q in Normal        |
       |                               |         |                                             |   quits, q in Command doesn't, Esc in         |
       |                               |         |                                             |   Command → Normal)                           |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.2  | Command palette shell         | Done | C: crates/tui/src/command_palette.rs         | cargo test -p tui command_palette::tests      | : opens palette, Esc closes
       |                               |         | M: crates/tui/src/app.rs (palette state)     | >= 6 tests: : opens, typing accumulates,      | Keys route to palette when active
       |                               |         | M: crates/tui/src/ui.rs (render bar)         |   Enter → ExecuteCommand, Esc → Normal,       | Bottom bar Constraint::Length(1)
       |                               |         | M: crates/tui/src/main.rs (route keys)       |   backspace deletes, empty Enter is no-op     | ExecuteCommand echoes as message
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.3  | Form overlay framework        | Done | C: crates/tui/src/form.rs                    | cargo test -p tui form::tests                 | Tab/Shift+Tab cycle fields
       |                               |         | M: crates/tui/src/app.rs (form_overlay)      | >= 8 tests: correct fields per FormKind,      | Submit with empty required → error
       |                               |         | M: crates/tui/src/ui.rs (render overlay)     |   Tab cycles focus, Shift+Tab reverse,        | Centered overlay 60%×50%
       |                               |         | M: crates/tui/src/main.rs (route keys)       |   submit empty required → error,              | Focused field highlighted
       |                               |         |                                             |   submit valid → SubmitForm action,           |
       |                               |         |                                             |   Esc cancels + returns Normal                |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.4  | Wire forms to DB (CRUD)       | Done | M: crates/tui/src/app.rs (new actions)       | cargo test -p tui                             | SubmitForm extracts form data
       |                               |         | M: crates/tui/src/main.rs (handle actions)   | >= 6 tests: SubmitForm → Create action,       | Delete calls db.delete_*() + refresh
       |                               |         | M: crates/tui/src/form.rs (extract data)     |   OpenForm sets input_mode + overlay,         | Create calls db.create_*() + refresh
       |                               |         |                                             |   delete workspace/project/plan actions       | Existing db methods reused
       |                               |         |                                             | Manual: TUI form → DB row created             |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.5  | Command parser                | Done | C: crates/tui/src/commands.rs                | cargo test -p tui commands::tests             | parse_command returns Result
       |                               |         | M: crates/tui/src/main.rs (wire Execute)     | >= 12 tests: "ws add /path name" → Create,    | Error messages shown in palette
       |                               |         | M: crates/tui/src/app.rs (new actions)       |   "ws list" → LoadWorkspaces,                 | ws add/del, proj new/del,
       |                               |         |                                             |   "ws del 1" → Delete, "proj new" → Form,    |   plan del, help
       |                               |         |                                             |   "help" → Help, unknown → Err,              | Simple cmds inline, complex open forms
       |                               |         |                                             |   missing args → Err, extra args → Ok         |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.6  | Command palette autocomplete  | Done | M: crates/tui/src/command_palette.rs         | cargo test -p tui command_palette::tests      | Prefix matching on static list
       |                               |         | M: crates/tui/src/ui.rs (suggestion popup)   | >= 6 tests: prefix match, Tab accepts top,    | Tab accepts top suggestion
       |                               |         |                                             |   Up/Down cycle, empty shows all,             | Up/Down cycle suggestions
       |                               |         |                                             |   no match → empty list,                      | Popup above command bar
       |                               |         |                                             |   selection wraps around                      |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.7  | Help overlay                  | Done | C: crates/tui/src/help.rs                    | cargo test -p tui help::tests                 | ? toggles help overlay
       |                               |         | M: crates/tui/src/app.rs (help state)        | >= 5 tests: help_content non-empty for all    | 3-col table: Key, Action, CLI Cmd
       |                               |         | M: crates/tui/src/ui.rs (render overlay)     |   ViewModes, ? toggles InputMode::Help,       | Centered 70%×80% overlay
       |                               |         | M: crates/tui/src/main.rs (route keys)       |   Esc returns Normal, ? in Help → Normal      | Esc or ? closes
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.8  | Inline CRUD keybindings       | Done | M: crates/tui/src/app.rs (per-view keys)     | cargo test -p tui                             | n → OpenForm per view context
       |                               |         | M: crates/tui/src/plan_view.rs               | >= 8 tests: n → OpenForm(correct kind),       | d → delete with y/n confirmation
       |                               |         | M: crates/tui/src/ui.rs (key hints in title) |   d → Delete for selected, d on empty → noop,| N → OpenForm(CreateWorkspace)
       |                               |         |                                             |   N → CreateWorkspace from any view,          | Key hints in view titles
       |                               |         |                                             |   delete confirm y executes, n cancels        | Keys ignored on empty lists
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.9  | Session import / assign       | Done | M: crates/tui/src/app.rs (untracked + picker)| cargo test -p tui                             | i toggles untracked filter
       |                               |         | M: crates/tui/src/ui.rs (picker popup)       | >= 6 tests: toggle filter, assign action,     | p opens project picker popup
       |                               |         | M: crates/tui/src/main.rs (handle assign)    |   picker navigation, assign sets project_id,  | Enter assigns selected project
       |                               |         |                                             |   filtered list correct, Esc closes picker    | db.update_session() called
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M2.10 | Status bar and feedback       | Done | M: crates/tui/src/app.rs (status_message)    | cargo test -p tui                             | Status clears after 5s
       |                               |         | M: crates/tui/src/ui.rs (bottom bar)         | >= 4 tests: set_status stores, renders,       | Left=key hints, right=status msg
       |                               |         | M: crates/tui/src/plan_view.rs               |   clears after timeout, command palette       | Command palette replaces bar
       |                               |         | M: crates/tui/src/project_view.rs            |   replaces bar when active                    | All CRUD ops call set_status
       |                               |         | M: crates/tui/src/main.rs (set_status calls) |                                               |
```

**M2 Exit Criteria:** TUI is self-sufficient for CRUD. `:ws add ~/dev test` creates workspace. `n` creates project/plan via form. `?` shows help. `i` filters untracked sessions, `p` assigns to project. `d` deletes with confirmation. Tab completion works in command palette. Status bar shows feedback after every operation. All checks pass: `cargo fmt-check && cargo lint-strict && cargo test --workspace --all-targets`.

---

## M3: Notifications & Quick Switch  ✓

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.1  | Notification module           | Done | C: crates/ca-lib/src/notify.rs               | cargo test -p ca-lib notify::tests            | osascript -e 'display notification'
       | (osascript, zero deps)        |         | M: crates/ca-lib/src/lib.rs (pub mod)        | >= 6 tests: send_notification formats          | Graceful no-op on non-macOS
       |                               |         |                                             |   correctly, special chars escaped,            | Title/subtitle/body parameters
       |                               |         |                                             |   non-macOS returns Ok (no-op),               | std::process::Command (no deps)
       |                               |         |                                             |   empty body handled                          |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.2  | Notification trigger config   | Done | M: crates/ca-lib/src/notify.rs               | cargo test -p ca-lib notify::tests            | NotificationRule struct
       |                               |         | M: crates/ca-lib/src/settings.rs (defaults)  | >= 6 tests: should_notify for each state       | Default: notify on NeedsInput only
       |                               |         |                                             |   transition, disabled rule skips,             | Configurable via settings table
       |                               |         |                                             |   custom rules from settings parsed,          | from/to state pairs as triggers
       |                               |         |                                             |   invalid config → fallback to defaults       |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.3  | Daemon notification dispatch  | Done | C: crates/daemon/src/notifier.rs             | cargo test -p daemon notifier::tests          | Fires after state change in polling loop
       |                               |         | M: crates/daemon/src/polling.rs (integrate)  | >= 4 tests: state change triggers notify,      | Dedup: don't re-notify same state
       |                               |         | M: crates/daemon/src/main.rs (init notifier) |   same state no re-notify, disabled →         | Rate limiting (1 per session per 30s)
       |                               |         |                                             |   silent, rate limit respected                |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.4  | TUI quick-switch              | Done | M: crates/tui/src/app.rs (keybindings)       | cargo test -p tui app::tests                  | 1-9 select by position index
       | (1-9, Tab/n for needs-input)  |         | M: crates/tui/src/ui.rs (position numbers)   | >= 8 tests: 1-9 select correct index,          | Tab/n cycles needs-input sessions
       |                               |         |                                             |   out-of-range ignored, Tab cycles             | Wraps around on last
       |                               |         |                                             |   needs-input only, n = next needs-input,     | No needs-input → Tab is no-op
       |                               |         |                                             |   no needs-input → Tab no-op, wrap around     |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.5  | Enhanced status bar with      | Done | M: crates/tui/src/app.rs (attention counts)  | cargo test -p tui                             | Count sessions per state
       | attention counts              |         | M: crates/tui/src/ui.rs (status bar)         | >= 4 tests: counts correct for mixed           | Show: "3 working | 1 needs input | 2 done"
       |                               |         |                                             |   states, zero states omitted, updates         | Yellow highlight for needs-input count
       |                               |         |                                             |   on session change                            | Integrates with existing status bar
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M3.6  | Visual indicators for         | Done | M: crates/tui/src/ui.rs (session list)       | cargo test -p tui                             | NeedsInput: yellow bg or bold
       | attention-needed sessions     |         | M: crates/tui/src/app.rs (tick counter)      | >= 4 tests: needs-input style differs,         | Blinking via tick-based toggle
       |                               |         |                                             |   blink toggles on tick, other states          | Working: green indicator pulses
       |                               |         |                                             |   unaffected, blink rate reasonable            | Done/Idle: subdued styling
```

**M3 Exit Criteria:** Claude session hits NeedsInput → macOS notification appears. Press `n` in TUI to jump to it. 1-9 selects by position. Status bar shows attention counts. Sessions needing input visually highlighted.

---

## M4: Code Review

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.1  | Git operations module         | Done | C: crates/ca-lib/src/git_ops.rs              | cargo test -p ca-lib git_ops::tests           | Parse unified diff format
       | (diff, log, show)             |         | M: crates/ca-lib/src/lib.rs (pub mod)        | >= 10 tests: parse_diff hunks correct,         | Structured DiffFile/DiffHunk/DiffLine
       |                               |         |                                             |   added/removed/context lines, binary file     | git diff --no-color for parsing
       |                               |         |                                             |   handling, empty diff, git log parsing,       | git log --format for structured output
       |                               |         |                                             |   commit_show, file rename detection          | Handle non-git dirs gracefully
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.2  | Reviews + review_comments     | Done | C: crates/ca-lib/src/review.rs               | cargo test -p ca-lib review::tests            | FK to sessions + projects
       | tables + CRUD                 |         | M: crates/ca-lib/src/db.rs (schema)          | >= 12 tests: create review, add comments,      | ReviewStatus: pending, in_progress,
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   list by project, list by session,            |   approved, changes_requested
       |                               |         |                                             |   resolve comment, delete review cascade,     | review_comments: file, line, body,
       |                               |         |                                             |   update status, get with comments             |   resolved bool
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.3  | TUI review mode               | Done | C: crates/tui/src/review_view.rs             | cargo test -p tui review_view::tests          | ViewMode::Review added
       | (commit list, diff viewer,    |         | M: crates/tui/src/app.rs (ViewMode, state)   | >= 8 tests: commit list renders,               | Scrollable diff with line numbers
       |  inline comments)             |         | M: crates/tui/src/ui.rs (route view)         |   diff scroll works, add comment on line,     | Left panel: commit list
       |                               |         | M: crates/tui/src/help.rs (review keys)      |   navigate hunks with n/p,                     | Right panel: diff viewer
       |                               |         |                                             |   j/k scroll, Enter on commit shows diff      | c adds comment at cursor line
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.4  | External diff integration     | Done | M: crates/tui/src/review_view.rs             | cargo test -p tui                             | v opens vimdiff for selected file
       | (vimdiff, delta)              |         | M: crates/tui/src/app.rs (actions)           | >= 4 tests: v returns VimdiffAction,           | d opens delta for selected file
       |                               |         | M: crates/tui/src/main.rs (handle action)    |   d returns DeltaAction, missing tool          | Restore terminal before exec
       |                               |         |                                             |   → error message, correct args passed        | Re-enter alternate screen after return
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.5  | Review submission             | Done | M: crates/ca-lib/src/review.rs (format)      | cargo test -p ca-lib review::tests            | Format comments as markdown text
       | (send feedback to Claude)     |         | M: crates/tui/src/review_view.rs (submit)    | >= 6 tests: format_review_feedback             | tmux send-keys to session pane
       |                               |         | M: crates/tui/src/main.rs (handle action)    |   produces correct markdown, empty review      | Group comments by file
       |                               |         |                                             |   → no-op, send-keys escapes special chars,   | Include file:line references
       |                               |         |                                             |   review status updated after submit          |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.6  | Review lifecycle              | Done | M: crates/daemon/src/polling.rs (detect)     | cargo test -p daemon                          | Session DONE → prompt for review
       | (ready → review → feedback    |         | M: crates/tui/src/app.rs (review prompts)    | >= 6 tests: session done triggers review       | Notification: "Session X ready for review"
       |  → iterate)                   |         | M: crates/ca-lib/src/notify.rs (review notif)|   prompt, review submitted → iterate,         | Review round counter
       |                               |         |                                             |   multiple rounds tracked, review status       | Status flow: pending→in_progress→
       |                               |         |                                             |   transitions valid                            |   approved/changes_requested
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M4.7  | ca review --html              | Done | C: crates/cli/src/review_html.rs             | cargo test -p cli review_html::tests          | Standalone HTML file (no server)
       | (standalone HTML diff export) |         | M: crates/cli/src/main.rs (review subcommand)| >= 4 tests: HTML output valid structure,       | Inline CSS for syntax highlighting
       |                               |         |                                             |   comments embedded, opens in browser,        | Include review comments inline
       |                               |         |                                             |   missing review → error                      | open command (macOS) to launch browser
```

**M4 Exit Criteria:** Claude makes 3 commits. TUI shows review mode with commit list. Navigate diffs, add per-line comments, submit. Claude receives formatted feedback via tmux send-keys. `ca review --html` generates browsable HTML diff. Review lifecycle tracks rounds.

---

## M5: Remote & Git Integration

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M5.1  | Remote host registration      | Pending | C: crates/ca-lib/src/remote.rs               | cargo test -p ca-lib remote::tests            | Workspace.host field (local/remote)
       | on workspaces                 |         | M: crates/ca-lib/src/db.rs (schema migration)| >= 8 tests: register host, list remotes,       | SSH connection test on register
       |                               |         | M: crates/ca-lib/src/workspace.rs (host)     |   connection test success/failure,             | Store: hostname, user, port, key_path
       |                               |         | M: crates/cli/src/main.rs (remote subcommand)|   duplicate host error, remove host,          | ca remote add user@host --key ~/.ssh/id
       |                               |         |                                             |   workspace linked to remote host             |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M5.2  | SSH tmux commands             | Pending | M: crates/ca-lib/src/remote.rs (ssh ops)     | cargo test -p ca-lib remote::tests            | ssh -t user@host tmux list-panes
       | (list, capture, send-keys)    |         | M: crates/ca-lib/src/tmux.rs (remote flag)   | >= 8 tests: list_remote_panes parses,          | Timeout on SSH commands (10s)
       |                               |         |                                             |   capture_remote_output works,                 | Error handling: unreachable host
       |                               |         |                                             |   send_keys_remote escapes correctly,         | Key-based auth only (no passwords)
       |                               |         |                                             |   connection failure → graceful error         | Reuse existing tmux.rs patterns
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M5.3  | Remote session discovery      | Pending | M: crates/ca-lib/src/discovery.rs (remote)   | cargo test -p ca-lib discovery::tests         | Extend polling to scan remote hosts
       |                               |         | M: crates/daemon/src/polling.rs (remote scan)| >= 6 tests: remote sessions discovered,        | Session.host = remote hostname
       |                               |         | M: crates/tui/src/ui.rs (host indicator)     |   host field set, TUI shows host badge,       | TUI: [remote] badge next to session
       |                               |         |                                             |   remote down → skip (no crash),              | Remote scan failure → log, continue
       |                               |         |                                             |   mixed local+remote list correct             |
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M5.4  | Commit stack navigation       | Pending | M: crates/ca-lib/src/git_ops.rs (stack)      | cargo test -p ca-lib git_ops::tests           | Branch-based commit stack
       | in TUI                        |         | C: crates/tui/src/git_view.rs                | >= 6 tests: stack from log correct,            | Show commits since branch point
       |                               |         | M: crates/tui/src/app.rs (ViewMode::Git)     |   navigate between commits, diff between      | j/k navigate, Enter shows diff
       |                               |         | M: crates/tui/src/ui.rs (route view)         |   adjacent commits, empty branch handled,     | Commit message + stats summary
       |                               |         | M: crates/tui/src/help.rs (git keys)         |   cherry-pick detection                       | Integration with review view
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M5.5  | GitHub PR creation            | Pending | C: crates/ca-lib/src/github.rs               | cargo test -p ca-lib github::tests            | Wraps `gh pr create`
       | (ca pr create wrapping gh)    |         | M: crates/cli/src/main.rs (pr subcommand)    | >= 6 tests: pr create args correct,            | Auto-fill title from first commit
       |                               |         |                                             |   body from commit messages, draft flag,       | Body from commit messages
       |                               |         |                                             |   gh not installed → error, pr list/view,     | --draft flag support
       |                               |         |                                             |   link PR to project in DB                    | ca pr create / ca pr list / ca pr view
```

**M5 Exit Criteria:** Register remote workspace. TUI shows remote sessions alongside local with host badge. Commit stack navigable in TUI. `ca pr create` generates PR via `gh` with auto-filled title/body. SSH operations timeout gracefully.

---

## M6: Resource Management & Document System

```
 #     | Step                          | Status  | Creates / Modifies                          | Validation                                    | Review Focus
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M6.1  | Parse Claude hook events      | Pending | M: crates/ca-lib/src/hooks.rs (token parse)  | cargo test -p ca-lib hooks::tests             | Extract input/output token counts
       | for token counts              |         | C: crates/ca-lib/src/resource.rs             | >= 8 tests: parse token counts from hook       | Parse cost data if available
       |                               |         | M: crates/ca-lib/src/lib.rs (pub mod)        |   payload, missing field → None,              | Fallback: scan ~/.claude/ logs
       |                               |         |                                             |   accumulate across events, model name        | ResourceMetric struct: session_id,
       |                               |         |                                             |   extracted, cost calculation                  |   metric_type, value, timestamp
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M6.2  | Resource tracking table       | Pending | M: crates/ca-lib/src/db.rs (schema)          | cargo test -p ca-lib resource::tests          | resource_usage table
       | + aggregation queries         |         | M: crates/ca-lib/src/resource.rs (CRUD)      | >= 10 tests: insert metric, query by           | Aggregation: sum by session, project
       |                               |         |                                             |   session, aggregate by project,              | Time-range queries (today, week, all)
       |                               |         |                                             |   time-range filter, cost_by_project,         | Cost estimation from token counts
       |                               |         |                                             |   total_tokens_by_session, empty → zero       | ca resources show --project <id>
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M6.3  | TUI resources panel           | Pending | C: crates/tui/src/resource_view.rs           | cargo test -p tui resource_view::tests        | ViewMode::Resources added
       |                               |         | M: crates/tui/src/app.rs (ViewMode, state)   | >= 6 tests: per-session breakdown renders,     | Per-session token breakdown
       |                               |         | M: crates/tui/src/ui.rs (route view)         |   project totals correct, empty → message,    | Project totals with cost estimates
       |                               |         | M: crates/tui/src/help.rs (resource keys)    |   sort by tokens/cost, time filter            | r key from sessions to open resources
       |                               |         |                                             |   toggles (today/week/all)                    | Table with sortable columns
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M6.4  | Document viewer with          | Pending | C: crates/tui/src/doc_view.rs                | cargo test -p tui doc_view::tests             | Read any text/markdown file
       | inline commenting             |         | M: crates/tui/src/app.rs (ViewMode::Doc)     | >= 8 tests: file loads, scroll works,          | Line-addressable comments
       |                               |         | M: crates/tui/src/ui.rs (route view)         |   add comment at line, list comments,         | Comment stored in review_comments
       |                               |         | M: crates/tui/src/help.rs (doc keys)         |   resolve comment, navigate between           | Similar UX to review view
       |                               |         |                                             |   comments, non-existent file → error         | Send doc feedback to Claude session
-------+-------------------------------+---------+---------------------------------------------+-----------------------------------------------+------------------------------------------
 M6.5  | Plan versioning               | Pending | M: crates/ca-lib/src/plan.rs (versioning)    | cargo test -p ca-lib plan::tests              | Auto-commit plan JSON on change
       | (git-backed plan history)     |         | C: crates/ca-lib/src/plan_version.rs         | >= 6 tests: save creates git commit,           | Diff between plan versions
       |                               |         | M: crates/tui/src/plan_view.rs (history)     |   list versions from git log,                 | TUI: h key shows plan history
       |                               |         |                                             |   diff between versions shows step changes,   | Restore previous version
       |                               |         |                                             |   restore old version works,                  | Plans stored as JSON files in
       |                               |         |                                             |   no git repo → graceful fallback             |   .claude-admin/plans/ dir
```

**M6 Exit Criteria:** Run sessions, check `ca resources show --project auth` for token/cost breakdown. TUI resources panel shows per-session and per-project totals. Open doc viewer, add comments, send to Claude. Plan changes auto-versioned, history viewable and restorable.

---

## Execution Order

```
M0.1 -> M0.2 -> M0.3 -> M0.4 -> M0.5 -> M0.6 -> M0.7 -> M0.8 -> M0.9 -> M0.10
                                                    (M0 complete ✓)
M1.1 -> M1.2 -> M1.3 -> M1.4 -> M1.12 -> M1.5 -> M1.6 -> M1.7 -> M1.11 -> M1.8 -> M1.9 -> M1.10
                                                    (M1 complete ✓)
M2.1 -> M2.2 -> M2.3 -> M2.4 -> M2.5 -> M2.6 -> M2.7 -> M2.8 -> M2.9 -> M2.10
                                                    (M2 complete ✓)
M3.1 -> M3.2 -> M3.3 -> M3.4 -> M3.5 -> M3.6
                                                    (M3 complete ✓)
M4.1 -> M4.2 -> M4.3 -> M4.4 -> M4.5 -> M4.6 -> M4.7  (M4 complete ✓)
M5.1 -> M5.2 -> M5.3 -> M5.4 -> M5.5
M6.1 -> M6.2 -> M6.3 -> M6.4 -> M6.5
```

## Dependency Graph

```
M0 (Dashboard) ──┬──> M1 (Plans) ──┬──> M3 (Notifs) ──> M5 (Remote & Git)
                  │                 │
                  │                 └──> M4 (Review) ───> M5 (Remote & Git)
                  │
                  └──> M2 (TUI CRUD)                  ──> M6 (Resources & Docs)
```

55 steps total. M0 (10 done) + M1 (12 done) + M2 (10 done) + M3 (6 done) + M4 (7 pending) + M5 (5 pending) + M6 (5 pending). 38 done, 17 pending.
