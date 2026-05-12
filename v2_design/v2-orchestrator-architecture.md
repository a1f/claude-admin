# v2-orchestrator — Architecture spec

> **Scope.** This document specifies the next major version of the claude_admin pipeline (`v2-orchestrator`). It replaces the current `claude -p`-based dispatch + watcher + pr-babysit toolchain with a hierarchical tmux + interactive-claude design backed by SQLite, driven (in MVP) by slash commands.
>
> **Conflict with existing v2_design HTML mocks.** The mocks in `v2_design/01-system-overview.html`–`v2_design/11-ui-session-live.html` describe a *self-hosted web server + multi-laptop daemon + browser SPA + Postgres + S3* topology (the "v2 web admin"). This document describes a *different* v2: a **local hierarchical tmux orchestrator + SQLite + slash-command MVP** — the user's revised direction. Where the two disagree, **this document wins for v2-orchestrator** and the HTML mocks should be treated as a parallel "future web shell" exploration, not the MVP target. Specifically:
> - **No web server, no browser SPA, no daemon-pairing protocol.** Everything runs on the developer's machine.
> - **SQLite at `~/.work/claude-admin.db`**, not Postgres on a VPS.
> - **No tunneled WS.** All IPC is local: tmux, sqlite, filesystem sentinels.
> - The HTML mocks' five-role agent vocabulary (architect / coder / reviewer / critic / validator) **carries over** — it's reused below with two role splits (per-task architect vs top-level architect; goal-critique vs breakdown-critique vs PR-critique).

---

## 1. Architecture overview

```
                      ┌──────────────────────────────────────────────────────────────┐
                      │  USER (you, in a terminal)                                   │
                      │   • slash commands: /goals /breakdown /dispatch /pr-babysit  │
                      │     /ship                                                    │
                      │   • optional: tmux attach -t claude-admin to watch any agent │
                      └──────────┬───────────────────────────────────────────────────┘
                                 │   (slash → ca CLI → daemon UDS)
                                 ▼
       ┌─────────────────────────────────────────────────────────────────────────────┐
       │  ca-daemon  (single foreground process, owns the orchestration)             │
       │   • UDS RPC: ~/.work/ca.sock                                                │
       │   • SQLite: ~/.work/claude-admin.db   (single source of truth)              │
       │   • tmux ops: create/destroy sessions + windows, capture-pane, send-keys   │
       │   • spawns + reaps agent processes; reconciles after crash                  │
       │   • watches sentinel files (~/.work/agents/<sid>/done) for agent exit      │
       └──────────┬──────────────────────────────────────────────────────────────────┘
                  │  (spawns + observes)
                  ▼
   ╔═════════════════════════════════════════════════════════════════════════════════╗
   ║   tmux topology (hierarchical)                                                  ║
   ║                                                                                 ║
   ║   tmux session: claude-admin              ← root, top-level orchestration       ║
   ║   ├── window: daemon                       (tail of ca-daemon log)              ║
   ║   ├── window: arch-top                     (top-level architect, on demand)     ║
   ║   ├── window: goal-crit-<mid>-<r>          (goal critique round r, on demand)   ║
   ║   ├── window: bd-crit-<mid>-<r>            (breakdown critique round r)         ║
   ║   └── window: review-summary-<pr>          (PR review summary author)           ║
   ║                                                                                 ║
   ║   tmux session: ca-<plan>-<task>          ← one per dispatch (e.g.              ║
   ║   ├── window: arch                         (per-task architect, the loop)       ║
   ║   ├── window: coder                        (interactive coder)                  ║
   ║   ├── window: reviewer-<n>                 (one per reviewer kind: bugs/quality)║
   ║   ├── window: critic-<n>                   (one per per-task critic)            ║
   ║   └── window: shell                        (escape hatch for the user)          ║
   ╚═════════════════════════════════════════════════════════════════════════════════╝
                  │
                  ▼
       ┌──────────────────────────────────────────────────────────────────────────────┐
       │  SQLite tables (single file)                                                 │
       │  goals (immutable) · validation_goals (immutable) · critiques (append-only)  │
       │  tasks (mutable, architect-owned) · dispatches · agents · reviews            │
       │  decisions · events (append-only audit log) · meta (schema_version)          │
       └──────────────────────────────────────────────────────────────────────────────┘
                  │
                  ▼
       ┌──────────────────────────────────────────────────────────────────────────────┐
       │  Filesystem (per-agent scratch, never source of truth)                       │
       │  ~/.work/agents/<session_id>/                                                │
       │     prompt.md       — initial system + user prompt                           │
       │     transcript.log  — tmux pipe-pane capture                                 │
       │     done            — sentinel file the agent touches when finished          │
       │     output.json     — final structured output (critique JSON, etc.)          │
       │  ~/dev/claude-admin-worktrees/<task-id>/  — git worktree (unchanged)         │
       └──────────────────────────────────────────────────────────────────────────────┘
```

**End-to-end flow** (one milestone, abridged):

```
Phase 1: GOALS
  user: /goals <plan> <milestone>
   → daemon spawns arch-top (tmux window) → drafts goals + validation goals → output.json
   → daemon spawns goal-crit (3 rounds max): score, surface critique, arch-top revises
   → loop terminates on score≥THRESHOLD or round=3
   → daemon FREEZES rows (sets ratified_at=NULL still; writes still possible)
   → user runs /ratify-goals → ratified_at=NOW; rows are now immutable

Phase 2: BREAKDOWN
  user: /breakdown <plan> <milestone>
   → daemon spawns arch-top → drafts tasks (mutable rows)
   → daemon spawns bd-crit (3 rounds max): does the task set cover all goals + validation goals?
   → user reviews, accepts → tasks remain mutable but versioned

Phase 3: DISPATCH (deterministic)
  user: /dispatch <plan> <task-id>     (or /dispatch <plan> --all-ready)
   → daemon: create worktree + branch
   → daemon: tmux new-session ca-<plan>-<task>
   → daemon: spawn per-task architect (window: arch) → it owns the inner loop
   → per-task arch spawns coder, awaits commit; on commit spawns reviewer + critic
   → per-task arch decides fix/drop/approve; on approve, marks task ready_for_pr

Phase 4: PR + MULTI-AGENT REVIEW
   → daemon (or per-task arch) opens PR with `gh pr create --draft`
   → daemon spawns 3 PR-critics (focus: did this PR match the task?)
   → daemon spawns 2 quality reviewers (bugs + quality — security DEFERRED, see §11)
   → all five report scored JSON via output.json
   → daemon spawns top-level architect (window: arch-top) → reads aggregate, decides
       approve | iterate | drop, with override flag
   → on approve, daemon spawns review-summary (write human-readable summary)
   → user receives a notification (terminal write or pushover) pointing at summary

Phase 5: SHIP + FINAL CRITIQUE
  user: /ship <plan> <task-id>     (or daemon's pr-babysit polls CI)
   → CI green → user merges → daemon detects merge → marks task=shipped
   → if it was the LAST task of a milestone:
       daemon spawns 3 final-goal-critics (independent, no shared output)
       average score; if low → surface to user with “consider new tasks”
```

---

## 2. Agent catalog

Every agent is **interactive `claude` running in a tmux window**, driven non-interactively by the daemon. All agents observe the same lifecycle: **spawn → claude session resumes / starts → final output written to `output.json` → `done` sentinel touched → daemon reaps → window closed (configurable retention)**.

| # | Role | Where (tmux) | Mutability of output | "I'm done" signal | Read-only? |
|---|---|---|---|---|---|
| 1 | **arch-top** | `claude-admin:arch-top` (root session) | Drafts immutable goals (until ratified); drafts + edits mutable tasks; writes `decisions` rows | `output.json` written + `done` sentinel | No (writes goals/tasks/decisions) |
| 2 | **goal-critique** | `claude-admin:goal-crit-<mid>-<round>` | Append-only `critiques` rows | `output.json` (score+rationale) + `done` | **Yes** — `--allowedTools` excludes Edit/Write/Bash |
| 3 | **breakdown-critique** | `claude-admin:bd-crit-<mid>-<round>` | Append-only `critiques` rows | same | **Yes** |
| 4 | **arch-task** (per-task architect) | `ca-<plan>-<task>:arch` | Owns the inner loop; updates `tasks.md` in worktree; writes `decisions` rows | `output.json` (terminal verdict: ready_for_pr / dropped) + `done` | No (writes within worktree + decisions) |
| 5 | **coder** | `ca-<plan>-<task>:coder` | Commits to branch | `done` sentinel touched **after** the agent runs `git push`; `output.json` lists commits + open items | No (Edit/Write/Bash within worktree) |
| 6 | **task-reviewer** | `ca-<plan>-<task>:reviewer-<kind>` | Append-only `reviews` rows | `output.json` (review JSON) + `done` | **Yes** (Read/Glob/Grep + Bash(git diff/show/log only)) |
| 7 | **task-critic** | `ca-<plan>-<task>:critic-<n>` | Append-only `critiques` rows (target_kind=task) | `output.json` (critique JSON) + `done` | **Yes** |
| 8 | **pr-critic** | `claude-admin:pr-crit-<pr>-<n>` | Append-only `critiques` rows (target_kind=pr) | `output.json` + `done` | **Yes** |
| 9 | **pr-quality-reviewer** | `claude-admin:pr-rev-<pr>-<kind>` | Append-only `reviews` rows | `output.json` + `done` | **Yes** |
| 10 | **review-summary** | `claude-admin:review-summary-<pr>` | Writes one row to `pr_summaries` (markdown) | `output.json` (the summary md) + `done` | **Yes** (read-only on repo + db reads) |
| 11 | **final-critic** | `claude-admin:final-crit-<mid>-<n>` | Append-only `critiques` rows (target_kind=final) | `output.json` + `done` | **Yes** |

### 2.1 Read-only enforcement (critique-class agents)

Two-layer:

1. **`--allowedTools` whitelist.** Critique agents get only `Read, Glob, Grep, Bash(git diff *), Bash(git show *), Bash(git log *), Bash(gh issue view *), Bash(gh pr view *), Bash(gh pr diff *)`. Anything else → permission denied → daemon detects and aborts the round.
2. **System prompt.** Append a hard-rule block: *"You are READ-ONLY. You MUST NOT ask the user any question. You MUST NOT call any Write/Edit/Bash mutating tool. If you genuinely need information you cannot reach, write `BLOCKED: <reason>` to your output.json and exit."*

The daemon also enforces a hard timeout (default 5 min for critique-class) and kills the window if exceeded — surfacing as `errored`.

### 2.2 Prompt sources

| Agent | Prompt source |
|---|---|
| coder | `~/.claude/skills/coder/SKILL.md` (existing — keep as-is) |
| task-reviewer | `~/.claude/skills/reviewer/SKILL.md` (existing — keep) |
| task-critic | `~/.claude/skills/critic/SKILL.md` (existing — keep) |
| pr-critic | New: `skills/pr-critic/SKILL.md`. Reuses critic prompt with PR-level framing ("score does this PR match the task" — not "the milestone goal") |
| pr-quality-reviewer | Reuses `skills/reviewer/SKILL.md` with `kind ∈ {bugs, quality}` |
| arch-task | New: `skills/arch-task/SKILL.md`. Defines the inner loop, decision rubric, when to spawn coder/reviewer, how to update tasks.md |
| arch-top | New: `skills/arch-top/SKILL.md`. Defines goal drafting, breakdown drafting, top-level PR decision, override semantics |
| goal-critique | New: `skills/goal-critique/SKILL.md`. Adversarial review of goals: are they observable? testable? scoped? |
| breakdown-critique | New: `skills/breakdown-critique/SKILL.md`. Coverage check: do tasks collectively achieve goals + validation goals? |
| review-summary | New: `skills/review-summary/SKILL.md`. Output: short markdown for the user; "no review needed" allowed |
| final-critic | New: `skills/final-critic/SKILL.md`. Reads merged code + tests + plan; scores GOAL achievement (not quality) |

**Each "new" skill should be ≤120 lines.** They inherit the structure of the existing `coder/critic/reviewer` skills (frontmatter + role + inputs + output schema + hard rules).

---

## 3. tmux topology

### 3.1 Naming convention

| Object | Name template | Example |
|---|---|---|
| Root session | `claude-admin` | `claude-admin` |
| Per-dispatch session | `ca-<plan>-<task_id>` (lowercased; `_` → `-`) | `ca-v2_design-m0a-t3` |
| Window inside root | `<role>` or `<role>-<scope>-<round>` | `arch-top`, `goal-crit-m0a-2` |
| Window inside per-dispatch | `<role>` or `<role>-<n>` | `arch`, `coder`, `reviewer-bugs`, `critic-3` |

**Constraint**: tmux names cap at ~50 chars. Plans/tasks longer than that are hashed (last 8 chars of sha1) and stored in `dispatches.tmux_session_name` so the daemon can map back.

### 3.2 Lifecycle: who creates, who closes

| Object | Creator | Closer |
|---|---|---|
| Root session `claude-admin` | `ca-daemon` on first boot (idempotent: `tmux has-session` first) | Never. Persists across daemon restarts. |
| Per-dispatch session | Daemon on `dispatch` RPC | Daemon on `task=shipped` OR explicit `cleanup` RPC; default retain 24h post-terminal |
| Per-agent window | Daemon (via `tmux new-window -d -t <session>`) | Daemon on agent reap (after `done` sentinel processed); default retain window 30 min for human inspection |
| Daemon log window | Daemon on boot | Daemon on shutdown |

**Retention rationale**: humans want to see what failed. Default-retain dead windows; provide `/cleanup` to nuke.

### 3.3 Recovery flow after a crash

The daemon's startup procedure (idempotent):

1. Open SQLite at `~/.work/claude-admin.db`. Run pending migrations.
2. Query `agents WHERE ended_at IS NULL` → list of "supposedly running" agents.
3. For each, `tmux has-session -t <tmux_session>` and `tmux list-windows -t <tmux_session>` → window exists?
4. For each agent:
   - Window exists + `done` sentinel exists → mark `ended_at=now`, parse `output.json`, advance state machine.
   - Window exists + no `done` sentinel + claude process alive → reattach (no-op, just trust).
   - Window exists + no `done` sentinel + claude process dead → mark `status=crashed`, surface to user.
   - Window missing → mark `status=lost`, surface.
5. Query `dispatches WHERE phase NOT IN terminal_phases` → for each, run a state-machine reconciler that looks at all its agents and decides what to do next (e.g. coder is shipped but no reviewer spawned → spawn reviewers).
6. Begin accepting RPC.

This means a `kill -9` of the daemon followed by restart loses **zero work** as long as the agents themselves are still running in tmux. Recovery == "look at the world, then look at the db, reconcile."

### 3.4 Attach/detach semantics

- The daemon **never attaches** to tmux interactively. It only uses headless commands (`tmux new-session -d`, `new-window -d`, `send-keys`, `capture-pane`, `pipe-pane`).
- The user can `tmux attach -t claude-admin` or `tmux attach -t ca-<plan>-<task>` at any time to spectate. Detach with `Ctrl-b d`. The daemon doesn't notice.
- The user **must not type** in agent windows — there's a hard-rule note in each agent's SKILL.md prompt and the daemon's tmux setup script writes a `tmux set-option -t <session> -w status-right` banner saying "READ-ONLY — do not type".
- A dedicated `shell` window in each per-dispatch session gives the user a place to poke at the worktree without contaminating the agent's pane.

---

## 4. Claude session management

### 4.1 The core problem

`claude` (interactive) is a TTY app. It expects a human keyboard. We need to:
- Start it inside a tmux window with a specific initial prompt.
- Capture all of its output for the daemon (without the daemon attaching).
- Reuse the same claude session for follow-up turns ("the reviewer flagged X, please fix") — this saves context build-up cost.
- Detect when it's "done" so the daemon can reap.

### 4.2 Spawning an agent (initial turn)

The daemon's "spawn agent" routine, per agent:

1. **Allocate a session_id** (uuidv7) and a directory `~/.work/agents/<session_id>/`.
2. **Materialize the prompt** — write `~/.work/agents/<session_id>/prompt.md`. This is the **user prompt** that will be sent. The system prompt comes from the role's SKILL.md (passed via `--append-system-prompt`).
3. **Construct the tmux window**. The window's command line is something like:
   ```
   claude \
     --append-system-prompt "$(cat ~/.work/agents/<sid>/system.md)" \
     --allowedTools "<role-allowlist>" \
     --permission-mode acceptEdits \
     --add-dir <worktree> \
     --session-id <sid>
   ```
   We pass `--session-id` so we know the UUID up front (claude assigns one if you don't; but for resume to work reliably we name it ourselves).
4. **Pipe the pane** to disk: `tmux pipe-pane -o -t <session>:<window> 'cat >> ~/.work/agents/<sid>/transcript.log'`. ANSI-preserved, append-only.
5. **Send the initial prompt** as keystrokes:
   ```
   tmux send-keys -t <session>:<window> -l "$(cat ~/.work/agents/<sid>/prompt.md)"
   tmux send-keys -t <session>:<window> Enter
   ```
   `-l` (literal) prevents tmux from interpreting prompt content as control sequences.
6. **Insert agent row** in db: `agents(session_id=<sid>, tmux_session, tmux_window, role, claude_session_uuid=<sid>, status='running', started_at=now)`.
7. **Return** to caller — fire-and-forget. The completion path is sentinel-driven (§4.4), not RPC-blocking.

### 4.3 Follow-up turns ("send another message to the same agent")

Two design options. **Recommendation: ALWAYS RESUME for fix-loops with the same agent role. ALWAYS FRESH for critique-class.**

**Option A — reuse the same claude session (RECOMMENDED for coder fix-on-feedback)**:
- Send keys to the existing pane:
  ```
  tmux send-keys -t <session>:<window> -l "$(cat new-prompt.md)"
  tmux send-keys -t <session>:<window> Enter
  ```
- Bonus: claude already has the prior turn in context — no re-feed of code/diff. Cheaper, faster.
- Risk: the pane might be in a weird state (mid-tool-use, mid-permission-prompt). The daemon must verify the pane is at the input prompt before sending. Heuristic: capture-pane, look for the input prompt regex; if mid-something, wait or signal.

**Option B — spawn a fresh `claude --resume <uuid>` in a new window**:
- Easier to reason about. Each turn = a clean tmux window.
- Costs another claude startup + session-restore latency.
- Use this for **critique re-rounds** (round 1 → round 2): spawn a brand-new agent (new session_id) so the round's output is unambiguously its own, never tangled with a prior turn's tail.

**Concrete decision matrix:**

| Scenario | Strategy |
|---|---|
| Coder needs to fix reviewer findings | **Reuse** (same window, send-keys with feedback bundle) |
| Coder gets stuck and user wants to nudge | **Reuse** |
| Per-task arch decides "iterate" → coder goes again | **Reuse** |
| Goal-critique round 2 (after arch revised goals) | **Fresh** (new sid, no prior context) |
| pr-critic instances (3 of them) | **Fresh × 3**, parallel, no shared context |
| arch-top decides on PR aggregate | **Fresh** every PR (one-shot decision) |

### 4.4 The "I'm done" signal

**Recommendation: filesystem sentinel + structured output file**, not exit detection.

Each agent's SKILL.md ends with a hard rule:

> When you finish, write your final structured output to `${OUTPUT_PATH}` (provided in the user prompt as an env-style variable, e.g. `~/.work/agents/<sid>/output.json`). Then run: `touch ~/.work/agents/<sid>/done`. Then exit (`/exit` or just stop).

The daemon runs an inotify-style polling loop (1s interval; macOS doesn't have inotify, fall back to mtime polling) on every active agent's `done` path. When `done` appears:

1. Daemon reads `output.json` → parses according to the role's output schema (db migration enforces shape per role).
2. Daemon updates `agents.ended_at`, `agents.status='done'`.
3. Daemon writes role-specific row(s): a critique → `critiques`, a review → `reviews`, an architect decision → `decisions`.
4. Daemon advances the state machine for the parent dispatch (or milestone for top-level agents).
5. Daemon closes the tmux window (after retention delay).

**Why not just watch the claude process exit?** Two reasons:
- The claude process can exit on `/exit` without having produced its structured output (e.g. it crashed or wandered off-script). The sentinel forces the agent to confirm it's done its actual job, not just that it exited.
- Some flows want the agent to stay around for a follow-up (coder waiting for feedback). Process-exit doesn't capture "done with this turn but available for next."

For **error detection**: if the claude process has been dead for >30s and no `done` sentinel exists, daemon marks `status=crashed`.

### 4.5 Output capture

`tmux pipe-pane -o ...` writes everything the user would see (with ANSI) to `transcript.log`. The daemon:
- Tails it for liveness checks (no new output for 5 min during expected work → mark `stuck`).
- Greps it for `permission_denied` / `tool_use_error` patterns → mark `permission_blocked`.
- Greps it for the literal `BLOCKED:` prefix the SKILL.md instructs critique agents to write → mark `agent_blocked`.

Structured output is **never** parsed from the transcript (fragile against ANSI / stream-mode quirks). The `output.json` file is the contract.

---

## 5. SQLite schema

**File**: `~/.work/claude-admin.db`. **Library**: `rusqlite` (Rust) for the daemon, or `sqlite3` Python module if MVP daemon is Python (see §7).

**WAL mode** + `PRAGMA synchronous=NORMAL` + `PRAGMA foreign_keys=ON`.

**Migrations**: hand-written, numbered (`001_initial.sql`, `002_add_X.sql`), tracked in a `meta(schema_version INTEGER)` row. The daemon runs unapplied migrations on startup. No ORM, no external migration tool — keep it dirt simple.

### 5.1 DDL (initial migration `001_initial.sql`)

```sql
PRAGMA foreign_keys = ON;

-- ─────────────────────────────────────────────────────────────────
-- META
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT INTO meta(key, value) VALUES ('schema_version', '1');

-- ─────────────────────────────────────────────────────────────────
-- PLAN / MILESTONE references (lightweight; the canonical plan
-- definition still lives in the registry.json + per-plan
-- milestones.json files for v1 compatibility)
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE plans (
  codename     TEXT PRIMARY KEY,                   -- 'v2_design'
  title        TEXT NOT NULL,
  registry_path TEXT NOT NULL,                     -- ~/.claude/plans/registry.json (for crosscheck)
  created_at   TEXT NOT NULL                        -- ISO-8601
);

CREATE TABLE milestones (
  id           TEXT PRIMARY KEY,                   -- 'v2_design:M0a' (composite to avoid collision across plans)
  plan_codename TEXT NOT NULL REFERENCES plans(codename),
  short_id     TEXT NOT NULL,                      -- 'M0a'
  title        TEXT NOT NULL,
  goal_summary TEXT,
  status       TEXT NOT NULL DEFAULT 'planned',    -- planned|goals_drafted|goals_ratified|broken_down|in_flight|all_merged|critiquing|shipped
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  UNIQUE(plan_codename, short_id)
);

-- ─────────────────────────────────────────────────────────────────
-- GOALS — IMMUTABLE after ratification.
-- Enforced by triggers + by daemon refusing UPDATE/DELETE when
-- ratified_at IS NOT NULL.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE goals (
  id            TEXT PRIMARY KEY,                  -- uuid7
  milestone_id  TEXT NOT NULL REFERENCES milestones(id),
  ord           INTEGER NOT NULL,                  -- display order, 1-based
  text          TEXT NOT NULL,                     -- 1-3 sentences, observable
  drafted_at    TEXT NOT NULL,
  ratified_at   TEXT,                              -- NULL until user ratifies; then frozen
  ratified_by   TEXT,                              -- 'user' or future: an agent's session_id
  UNIQUE(milestone_id, ord)
);

CREATE TABLE validation_goals (
  id            TEXT PRIMARY KEY,                  -- uuid7
  milestone_id  TEXT NOT NULL REFERENCES milestones(id),
  ord           INTEGER NOT NULL,
  text          TEXT NOT NULL,                     -- the validation scenario
  kind          TEXT NOT NULL,                     -- 'unit'|'integration'|'e2e'|'fe'|'be'
  drafted_at    TEXT NOT NULL,
  ratified_at   TEXT,
  ratified_by   TEXT,
  UNIQUE(milestone_id, ord)
);

-- Triggers to ENFORCE immutability after ratification
CREATE TRIGGER goals_no_update_after_ratify
BEFORE UPDATE ON goals
FOR EACH ROW
WHEN OLD.ratified_at IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'goals: row is ratified and immutable');
END;

CREATE TRIGGER goals_no_delete_after_ratify
BEFORE DELETE ON goals
FOR EACH ROW
WHEN OLD.ratified_at IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'goals: row is ratified and immutable');
END;

-- (identical triggers for validation_goals — omitted for brevity in this spec;
--  generate from a sql template at migration time)

-- ─────────────────────────────────────────────────────────────────
-- TASKS — MUTABLE, architect-owned.
-- Updates are tracked via the `events` audit log, not row-level
-- versioning, to keep the schema simple.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE tasks (
  id            TEXT PRIMARY KEY,                  -- 'v2_design:M0a-T1' (composite)
  milestone_id  TEXT NOT NULL REFERENCES milestones(id),
  short_id      TEXT NOT NULL,                     -- 'M0a-T1'
  title         TEXT NOT NULL,
  body_md       TEXT NOT NULL,                     -- the task spec (deliverable, scope, validation, ...)
  status        TEXT NOT NULL DEFAULT 'planned',   -- planned|dispatched|coding|reviewing|ready_for_pr|pr_open|pr_reviewing|approved|merged|dropped|blocked
  est_loc       INTEGER,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE(milestone_id, short_id)
);

CREATE TABLE task_dependencies (
  task_id       TEXT NOT NULL REFERENCES tasks(id),
  depends_on    TEXT NOT NULL REFERENCES tasks(id),
  required_state TEXT NOT NULL,                    -- 'merged'|'drafted'|'ready'
  PRIMARY KEY (task_id, depends_on)
);

CREATE INDEX tasks_by_milestone   ON tasks(milestone_id, status);
CREATE INDEX taskdeps_by_blocker  ON task_dependencies(depends_on);

-- ─────────────────────────────────────────────────────────────────
-- DISPATCHES — one row per /dispatch invocation. Maps to one
-- per-task tmux session.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE dispatches (
  id              TEXT PRIMARY KEY,                -- uuid7
  task_id         TEXT NOT NULL REFERENCES tasks(id),
  worktree_path   TEXT NOT NULL,
  branch          TEXT NOT NULL,
  tmux_session    TEXT NOT NULL,                   -- 'ca-v2-design-m0a-t1' (post-sanitize)
  phase           TEXT NOT NULL,                   -- spawning|coding|reviewing|ready_for_pr|pr_open|pr_reviewing|approved|merged|dropped|aborted|errored
  pr_number       INTEGER,                          -- once a PR is opened
  pr_url          TEXT,
  started_at      TEXT NOT NULL,
  ended_at        TEXT,
  error_summary   TEXT
);

CREATE INDEX dispatches_active ON dispatches(phase) WHERE ended_at IS NULL;

-- ─────────────────────────────────────────────────────────────────
-- AGENTS — every claude invocation. session_id == claude --session-id
-- (so resume by uuid works).
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE agents (
  session_id     TEXT PRIMARY KEY,                 -- uuid v7; also passed to claude --session-id
  role           TEXT NOT NULL,                    -- 'arch-top'|'arch-task'|'coder'|'task-reviewer'|...
  role_kind      TEXT,                              -- for reviewer/critic: 'bugs'|'quality'|null
  parent_kind    TEXT NOT NULL,                    -- 'dispatch'|'milestone'|'pr'|'goal-round'|'breakdown-round'
  parent_id      TEXT NOT NULL,                    -- the dispatch.id / milestone.id / pr_number / round_id
  tmux_session   TEXT NOT NULL,
  tmux_window    TEXT NOT NULL,
  pid            INTEGER,                           -- the claude process pid (best-effort)
  scratch_dir    TEXT NOT NULL,                    -- ~/.work/agents/<sid>/
  status         TEXT NOT NULL DEFAULT 'spawning', -- spawning|running|done|crashed|killed|stuck|permission_blocked|agent_blocked
  started_at     TEXT NOT NULL,
  ended_at       TEXT,
  output_summary TEXT                               -- short string, e.g. "score=87 verdict=acceptable"
);

CREATE INDEX agents_active        ON agents(status)  WHERE ended_at IS NULL;
CREATE INDEX agents_by_parent     ON agents(parent_kind, parent_id);

-- ─────────────────────────────────────────────────────────────────
-- CRITIQUES — append-only.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE critiques (
  id             TEXT PRIMARY KEY,                 -- uuid7
  target_kind    TEXT NOT NULL,                    -- 'goals'|'breakdown'|'task'|'pr'|'final'
  target_id      TEXT NOT NULL,                    -- milestone.id / task.id / pr_number / dispatch.id
  round          INTEGER NOT NULL DEFAULT 1,       -- for goal/breakdown auto-revise loop
  agent_session  TEXT NOT NULL REFERENCES agents(session_id),
  score          INTEGER,                           -- 0-100
  verdict        TEXT NOT NULL,                    -- 'strong'|'acceptable'|'weak'|'reject'|'pass'|'fail'
  rationale_md   TEXT NOT NULL,
  axes_json      TEXT,                              -- per-axis subscores (for task/pr critics)
  concerns_json  TEXT,                              -- list of concerns
  created_at     TEXT NOT NULL
);

CREATE INDEX critiques_by_target ON critiques(target_kind, target_id, round);

-- Append-only enforcement (no UPDATE, no DELETE)
CREATE TRIGGER critiques_no_update BEFORE UPDATE ON critiques
BEGIN SELECT RAISE(ABORT, 'critiques: append-only'); END;
CREATE TRIGGER critiques_no_delete BEFORE DELETE ON critiques
BEGIN SELECT RAISE(ABORT, 'critiques: append-only'); END;

-- ─────────────────────────────────────────────────────────────────
-- REVIEWS — append-only. Quality reviewer findings.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE reviews (
  id              TEXT PRIMARY KEY,                -- uuid7
  target_kind     TEXT NOT NULL,                   -- 'task'|'pr'
  target_id       TEXT NOT NULL,
  agent_session   TEXT NOT NULL REFERENCES agents(session_id),
  kind            TEXT NOT NULL,                   -- 'security'|'bugs'|'quality'
  summary_md      TEXT,
  blocker_count   INTEGER NOT NULL DEFAULT 0,
  major_count     INTEGER NOT NULL DEFAULT 0,
  minor_count     INTEGER NOT NULL DEFAULT 0,
  nit_count       INTEGER NOT NULL DEFAULT 0,
  findings_json   TEXT NOT NULL,                   -- raw findings array
  created_at      TEXT NOT NULL
);

CREATE INDEX reviews_by_target ON reviews(target_kind, target_id);

CREATE TRIGGER reviews_no_update BEFORE UPDATE ON reviews
BEGIN SELECT RAISE(ABORT, 'reviews: append-only'); END;
CREATE TRIGGER reviews_no_delete BEFORE DELETE ON reviews
BEGIN SELECT RAISE(ABORT, 'reviews: append-only'); END;

-- ─────────────────────────────────────────────────────────────────
-- DECISIONS — every architect verdict (per-task or top-level).
-- Append-only; supersession is captured by latest-row-wins per
-- (target_kind, target_id).
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE decisions (
  id                  TEXT PRIMARY KEY,            -- uuid7
  target_kind         TEXT NOT NULL,               -- 'task'|'pr'|'milestone'
  target_id           TEXT NOT NULL,
  architect_session   TEXT NOT NULL REFERENCES agents(session_id),
  architect_role      TEXT NOT NULL,               -- 'arch-task'|'arch-top'
  decision            TEXT NOT NULL,               -- 'approve'|'iterate'|'drop'|'ratify'
  override            INTEGER NOT NULL DEFAULT 0,  -- 1 if architect overrode aggregate score
  rationale_md        TEXT NOT NULL,
  follow_up_task_ids  TEXT,                        -- JSON array of task ids the architect added
  created_at          TEXT NOT NULL
);

CREATE INDEX decisions_by_target ON decisions(target_kind, target_id, created_at);

-- ─────────────────────────────────────────────────────────────────
-- PR SUMMARIES — output of the review-summary agent. One per PR.
-- Latest-wins (we may regenerate after iterations).
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE pr_summaries (
  id              TEXT PRIMARY KEY,                -- uuid7
  pr_number       INTEGER NOT NULL,
  dispatch_id     TEXT NOT NULL REFERENCES dispatches(id),
  agent_session   TEXT NOT NULL REFERENCES agents(session_id),
  recommendation  TEXT NOT NULL,                   -- 'ship'|'inspect'|'do_not_ship'
  body_md         TEXT NOT NULL,
  trivial         INTEGER NOT NULL DEFAULT 0,      -- 1 if "no review needed"
  created_at      TEXT NOT NULL
);

CREATE INDEX pr_summaries_by_pr ON pr_summaries(pr_number, created_at DESC);

-- ─────────────────────────────────────────────────────────────────
-- EVENTS — append-only audit log. Everything the daemon does.
-- This is what /history queries.
-- ─────────────────────────────────────────────────────────────────
CREATE TABLE events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ts          TEXT NOT NULL,
  kind        TEXT NOT NULL,                       -- 'spawn_agent'|'agent_done'|'phase_change'|'decision'|'ratify'|'crash'|'override'|...
  subject     TEXT NOT NULL,                       -- free-form, e.g. 'task:M0a-T1' or 'agent:<sid>'
  payload_json TEXT
);
CREATE INDEX events_by_subject ON events(subject, ts);

CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'events: append-only'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'events: append-only'); END;
```

### 5.2 Schema rationale (table by table)

- **plans / milestones**: lightweight mirror of the registry/milestones-source files, so SQL queries can join across them cleanly. The canonical source for v1 compatibility stays in JSON; the daemon keeps the db row in sync on `goals/breakdown/dispatch` ops.
- **goals + validation_goals**: split because the audiences differ. Goals are read by humans; validation goals are read by automated runners (they're the test scenarios). Both immutable post-ratify, enforced by triggers.
- **tasks**: deliberately mutable. `body_md` is the full markdown spec; revisions overwrite. The audit trail lives in `events` (kind=`task_updated` with payload showing diff). No row versioning — keeps schema simple.
- **task_dependencies**: separate table because tasks can have N blockers and we want to query "what's unblocked?" cheaply (the `/suggest` use case). `required_state` lets us model "T2 needs T1 merged" vs "T2 needs T1 just drafted".
- **dispatches** vs **tasks**: a task may be re-dispatched (e.g. drop + retry). Each dispatch is a fresh row. `tasks.status` reflects the *current* dispatch's state; the history of dispatches is preserved.
- **agents**: every claude invocation. Indexed by status for the daemon's reconcile-on-boot query. `session_id` is the claude session UUID, so `--resume <session_id>` works directly.
- **critiques** + **reviews** + **decisions**: append-only by trigger. Latest-wins via `created_at DESC` queries when the daemon needs the "current" verdict.
- **pr_summaries**: separate from `decisions` because the summary is the *human-readable narrative*, not the verdict. Architect's `decisions` row is the machine truth; `pr_summaries.body_md` is what the user actually reads.
- **events**: cheap insert-only firehose. `payload_json` is opaque to the schema but conventional per `kind`. Used by `/history`, `/ca status`, debugging.

### 5.3 Indexes (rationale)

- `dispatches_active` (partial WHERE ended_at IS NULL) — boot reconcile lists only active dispatches.
- `agents_active` (partial) — same.
- `agents_by_parent` — given a dispatch.id, get all its agents fast.
- `critiques_by_target` (..., round) — fetch round 2 of goal critique cheaply.
- `tasks_by_milestone` (..., status) — `/suggest` and `/status` queries.
- `taskdeps_by_blocker` — "now that T1 merged, what unblocks?" query.
- `pr_summaries_by_pr` (..., created_at DESC) — latest summary per PR.
- `events_by_subject` — `/history task:M0a-T1` or `/history agent:<sid>`.

### 5.4 Migration story

- Single `migrations/` directory with `NNN_*.sql` files.
- Daemon on boot reads `meta.schema_version`, applies any with `NNN > version`, bumps `meta.schema_version` in the same transaction as each migration.
- No ORM, no third-party migration tool. Migrations are hand-reviewed plain SQL.

---

## 6. State machine

### 6.1 Milestone-level (drives goals/breakdown phases)

```
       planned
          │   /goals
          ▼
    drafting_goals  ──goal-critique cycle──┐
          │           (≤3 rounds)          │
          ▼                                 │
    awaiting_ratify  ◄──────revise──────────┘
          │   /ratify-goals
          ▼
    goals_ratified
          │   /breakdown
          ▼
    drafting_breakdown ──breakdown-critique cycle──┐
          │              (≤3 rounds)               │
          ▼                                         │
    awaiting_breakdown_ratify  ◄────revise─────────┘
          │   /ratify-breakdown   (or implicit accept)
          ▼
    broken_down
          │   /dispatch (first task)
          ▼
    in_flight
          │   (last task merged)
          ▼
    final_critiquing  ──3 final-critics in parallel──→ avg score
          │
          ├──score ≥ THRESHOLD──► shipped  ◀── terminal
          └──score <  THRESHOLD──► gaps_found
                                      │   user adds new tasks
                                      ▼
                                   in_flight (loop)
```

**Triggering events**:
- `/goals` invocation → `planned` → `drafting_goals`.
- Daemon detects all goal-critique agents `done` → either revise (back to `drafting_goals` with round+1) or `awaiting_ratify`.
- `/ratify-goals` user command → set `goals.ratified_at` for all rows + `validation_goals.ratified_at` → `goals_ratified`.
- Same loop for breakdown.
- Each task moves through its own state machine (§6.2); when ALL tasks in a milestone reach `merged`, the milestone moves to `final_critiquing`.

### 6.2 Per-task state machine (drives the inner loop)

```
       planned
         │   /dispatch
         ▼
       spawning  →  coding  →  reviewing  →  ready_for_pr  →  pr_open
                       │           │              │               │
                       │           ▼              │               ▼
                       │      (per-task           │           pr_reviewing
                       │       arch decides)      │                │
                       │           │              │           ┌────┴───────┐
                       │      iterate?            │           │   approved │
                       │           │              │           │     │      │
                       │           └──► coding    │           │     ▼      │
                       │                          │           │  awaiting_ │
                       │      drop?               │           │   ci_green │
                       │           │              │           │     │      │
                       │           └──► dropped   │           │     ▼      │
                       │                          │           │   merged   │
                       └──stuck/perm_blocked──► errored       │     │      │
                                                              │     ▼      │
                                                              │  shipped   │
                                                              │            │
                                                              └─drop───► dropped
```

**Triggering events** (per-task arch is the conductor):

| Event | Trigger | New state |
|---|---|---|
| `coder.done` | `done` sentinel + `output.json` shows commits | `reviewing` |
| `reviewer.done && critic.done` for all spawned | Daemon counts `agents.status='done' WHERE parent_id=<dispatch>` | per-task arch is woken (see below) |
| per-task arch decides `iterate` | arch's `output.json` decision=iterate + feedback bundle | `coding` (reuse coder window with new prompt) |
| per-task arch decides `drop` | decision=drop | `dropped`; daemon closes session |
| per-task arch decides `approve` | decision=approve | `ready_for_pr`; daemon runs `gh pr create --draft` |
| pr created | `gh pr create` returns | `pr_open`; daemon spawns 3 pr-critics + 2 pr-reviewers |
| all pr-reviewers + pr-critics done | count check | `pr_reviewing`; spawn arch-top with aggregate |
| arch-top decides `approve` | decision=approve | `approved`; spawn review-summary, notify user |
| user runs `/ship` | (interactive) | merges via `gh pr merge`, → `merged` → `shipped` |

**Waking the per-task arch.** When all reviewers+critics for a task finish, the daemon needs to either send a follow-up turn to the existing arch-task agent (reuse — preferred to keep context) or spawn a fresh one. **Recommendation: reuse.** The arch-task agent's first prompt should explicitly tell it "you will receive aggregated review JSON later via stdin" — daemon then `tmux send-keys` the aggregate as a new turn.

### 6.3 Goal/breakdown auto-revise loop (persistence)

Each round of critique gets:
- A **round_id** (uuid7) stored in `events` with `kind='critique_round_started'`, payload includes `target_kind`, `target_id`, `round_number`, `parent_arch_session`.
- The critique agent's row in `agents` has `parent_kind='goal-round'`, `parent_id=<round_id>`.
- The critique's row in `critiques` has `round=<round_number>`.

**On daemon restart mid-round**:
1. Reconcile finds critique agents with `status='running'` for an open round.
2. Reads their `done` sentinels.
3. If all done → tally scores → either advance (max-rounds-reached → `awaiting_ratify`) or send a "revise" turn to the arch-top (Option A: reuse).
4. If some not done → wait (no action; they're still running in tmux).
5. If some crashed → respawn just the crashed ones (assign new session_ids, same round_id).

Worst case: round 2 is interrupted by daemon kill; on restart, daemon sees "round 2 is live, 2 of 3 critiques done"; waits for the 3rd. The 3rd is still in tmux, claude session intact. Zero work lost.

---

## 7. Daemon design

### 7.1 What it is

`ca-daemon` — a long-running foreground process. **Recommendation: write in Python** for the MVP (faster to iterate, sqlite3 + subprocess + os.kill + libtmux are all stdlib-ish), with the door open to porting hot paths to Rust later. The current `dispatch.py` + `watcher.py` stack is already Python; the daemon is essentially "watcher.py promoted to a long-running orchestrator with an RPC surface."

(If the user has a strong Rust preference, switch to Rust + rusqlite + tokio + libc::kill + a tmux subprocess wrapper. Same architecture.)

### 7.2 What it listens to

| Channel | Mechanism | Used for |
|---|---|---|
| **UDS RPC** at `~/.work/ca.sock` | Unix domain socket, JSON-RPC framed by length-prefix | Slash commands (`ca dispatch`, `ca goals`, `ca status`, ...) sent by a thin `ca` CLI |
| **Sentinel polling** | `os.scandir(~/.work/agents/)` every 1s, look for `done` files | Agent completion detection |
| **Process polling** | `os.kill(pid, 0)` per active agent every 5s | Crash detection |
| **Transcript tailing** | Per-agent `tail -F transcript.log` (or scan offset) every 5s | Stuck/permission-blocked detection |
| **PR poll** | `gh pr view <num> --json state,mergeable,statusCheckRollup` every 30s while `pr_open` or `awaiting_ci_green` | CI-green detection (replaces pr-babysit poll loop) |
| **SIGHUP** | reload SKILL.md prompts, no other state | Hot-reload during dev |
| **SIGTERM** | graceful shutdown: stop spawning, persist state, do NOT kill agents | Clean restart preserving in-flight work |

### 7.3 Boot reconciliation algorithm

```
on_boot():
  open_db_with_migrations()
  ensure_root_tmux_session("claude-admin")
  ensure_daemon_log_window()

  # 1. Reconcile agents
  active_agents = db.query("SELECT * FROM agents WHERE ended_at IS NULL")
  for a in active_agents:
    if not tmux_window_exists(a.tmux_session, a.tmux_window):
      mark_lost(a)            # status='lost'; emit event; surface to user
      continue
    if Path(f"{a.scratch_dir}/done").exists():
      consume_done(a)         # parse output.json; insert critique/review/decision; close window
      continue
    if not pid_alive(a.pid):
      mark_crashed(a)
      continue
    # else: still running, do nothing — will be picked up by sentinel poller

  # 2. Reconcile dispatches
  active_dispatches = db.query("SELECT * FROM dispatches WHERE ended_at IS NULL")
  for d in active_dispatches:
    advance_state_machine(d)  # given current agents' states + db rows, what's next?

  # 3. Reconcile rounds
  open_rounds = db.query("""SELECT DISTINCT round_id FROM events
                             WHERE kind='critique_round_started'
                             AND round_id NOT IN (SELECT round_id FROM events WHERE kind='critique_round_finished')""")
  for r in open_rounds:
    advance_round(r)

  start_pollers()             # sentinel, process, transcript, gh-pr
  start_rpc_server()
```

### 7.4 Spawning vs reaping (single source of truth)

**Spawning** is always done by the daemon. Slash commands → RPC → daemon decides whether to spawn. Critic loops, fix-on-feedback, all driven by daemon state-machine code.

**Reaping** is sentinel-driven (§4.4). The daemon never blocks on a child's exit. The sentinel poller is the universal "agent finished" signal.

### 7.5 Where the existing dispatch.py logic moves

| Old location | New location |
|---|---|
| `dispatch.py:main` (whole flow) | `daemon/dispatch.py:handle_dispatch_rpc` (called from RPC) |
| `dispatch.py:check_blocker` (gh pr list etc.) | `daemon/blockers.py` (kept; called pre-spawn) |
| `dispatch.py:force_cleanup` | `daemon/cleanup.py:cleanup_dispatch(dispatch_id)` (also exposed as RPC) |
| `watcher.py:wait` (state-machine loop) | Absorbed into the daemon's main event loop |
| `watcher.py:status` | Replaced by SQL query; CLI `ca status` |
| `watcher.py: review fan-out` | `daemon/orchestrator.py:on_coder_done` spawns reviewers/critics via §4.2 |
| `pr_babysit.py:show` | `ca pr-babysit show` → SQL query against `pr_summaries` + `decisions` + `reviews` |
| `pr_babysit.py:merge/ready/drop` | `ca pr-babysit <action>` → daemon RPC → daemon runs gh + updates db |

---

## 8. Skill / command surface for MVP

A minimal, opinionated set. Each is a thin slash-command shim around `ca` CLI which talks to the daemon. The user can drive the entire pipeline through these commands; the daemon handles all orchestration.

| Slash command | Purpose | Daemon RPC | Needs human input? |
|---|---|---|---|
| `/goals <plan> <milestone>` | Kick off goal drafting + critique loop | `goals.draft` | No (auto-revises up to 3 rounds, then surfaces) |
| `/ratify-goals <plan> <milestone>` | Freeze goals + validation goals | `goals.ratify` | Yes — user reviews the latest draft + critique trail in chat |
| `/breakdown <plan> <milestone>` | Draft tasks + breakdown critique loop | `breakdown.draft` | No (same auto-revise) |
| `/ratify-breakdown <plan> <milestone>` | Mark tasks as the working set | `breakdown.ratify` | Yes |
| `/dispatch <plan> <task-id> [--force]` | Spawn the per-task tmux session + arch-task | `dispatch.start` | No |
| `/dispatch <plan> --all-ready` | Suggest + dispatch every unblocked task | same, looped | No |
| `/status [<plan>] [<task-id>]` | Show live state from sqlite (active dispatches, latest critiques, etc.) | `status.snapshot` | No |
| `/pr-babysit <plan> <task-id> [show\|ready\|merge\|drop\|iterate]` | The user's PR decision UI; reads pr_summaries + decisions | `pr.action` | Yes |
| `/ship <plan> <task-id>` | Convenience: poll until CI green, then merge (replaces pr-babysit's two-step ready+merge for the happy path) | `pr.ship` | No (notifies on done) |
| `/revisit <plan>` | After a merge, propose plan amendments | `revisit.run` | No (writes a recommendation file) |
| `/cleanup <plan> [<task-id>]` | Tear down tmux sessions + worktrees post-shipping | `cleanup.run` | No |
| `/history <subject>` | Tail the events log for a task / agent | `events.tail` | No |
| `/agents [<plan>] [<task-id>]` | List active agents + their tmux locations | `agents.list` | No |
| `/attach <plan> <task-id> [<role>]` | Print `tmux attach -t ca-<plan>-<task>:<role>` so user can spectate | (none, local cmd) | Yes (it's the user that attaches) |

**Notes**:
- All slash commands are thin SKILL.md files that shell out to a single `ca` CLI binary. The CLI is itself thin — it just dials the daemon's UDS socket and round-trips JSON.
- Existing skills `/suggest`, `/breakdown`, `/dispatch`, `/pr-babysit` are **upgraded in place** to invoke the new daemon (rather than running their own `dispatch.py` / `watcher.py`). This keeps the user's muscle memory.
- New skills: `/goals`, `/ratify-goals`, `/ratify-breakdown`, `/ship`, `/cleanup`, `/history`, `/agents`, `/attach`. Eight new slash commands.

---

## 9. MVP cut

### IN — must work to call MVP done

| Capability | Slash | Notes |
|---|---|---|
| Goals draft + bounded critique loop + ratify | `/goals` `/ratify-goals` | All 3 rounds, persisted, restart-safe |
| Breakdown draft + critique + ratify | `/breakdown` `/ratify-breakdown` | Tasks land in db, mutable |
| Dispatch one task | `/dispatch <plan> <task>` | Creates worktree + per-task tmux session + spawns arch-task → coder → reviewer + critic → arch-task decides → PR open |
| Multi-agent PR review | (auto on PR open) | 3 pr-critics + 2 pr-reviewers (bugs + quality) → arch-top decision → review-summary |
| User ship loop | `/pr-babysit` / `/ship` | Mostly preserves existing UX |
| Final goal critique on milestone close | (auto on last task merged) | 3 final-critics, average score, surface |
| Restart-safe daemon | (no slash) | Crash + restart preserves all in-flight work |
| Read-only enforcement on critique agents | (built into spawn) | Allowedtools whitelist + system prompt rule |
| SQLite as the single source of truth | (built in) | Migration applied on boot |
| Hierarchical tmux (root + per-dispatch) | (built into spawn) | Naming convention as §3.1 |
| `/status` `/agents` `/attach` `/history` for visibility | various | The user can always answer "what's happening?" |

### DEFERRED — explicitly NOT in MVP, with rationale

| Capability | Why deferred |
|---|---|
| **TUI / browser UI** | The user explicitly said: don't start with TUI; manual slash commands are acceptable for MVP. The HTML mocks in v2_design/ inform the future TUI but are not built now. |
| **Security reviewer** | Recommendation: drop from MVP fan-out. Bugs + quality cover most defects; security is high-noise, low-signal for the kinds of plans this orchestrator typically runs (orchestration code, not auth code). Re-add as an opt-in axis when the project flag says `security_sensitive: true`. |
| **Coder fix-on-feedback automation (full loop)** | MVP runs *one* coder pass per dispatch. If per-task arch says iterate, it sends ONE follow-up turn to the same coder. Multi-round iteration with arbitrary depth is deferred — caps at 2 rounds per dispatch in MVP, then escalates to user. |
| **Goal-critique threshold tuning per project** | MVP uses a global threshold (`score ≥ 80` to advance). Per-project config deferred. |
| **Multi-laptop / daemon pairing / WSS tunnel** | The HTML-mock vision; out of scope for this orchestrator. If the user later wants a remote shell, build a separate "ca-web" project that talks to the same SQLite. |
| **Validator agents (playwright, e2e runner)** | The HTML mocks list these. MVP relies on the validation_goals being checked manually or by CI. Automating playwright validation is a v2.1. |
| **Self-improvement / retrospective generation** | `improver` skill stays as today (post-merge). MVP doesn't auto-apply improvements to prompts. |
| **Auto-merge after CI green** | MVP requires the user to run `/ship` (or `/pr-babysit merge`). Auto-merge can be opt-in later. |
| **Resource cleanup automation** | MVP retains tmux windows + worktrees forever unless `/cleanup` is run. A reaper that nukes >7-day-old shipped artifacts is deferred. |
| **Agent cost tracking** | The token-budget table in HTML mock 04 is deferred. MVP relies on subscription quota. |
| **Concurrency limits** | MVP allows arbitrarily many parallel dispatches. Quota enforcement (e.g. max 5 concurrent coders) is deferred. |
| **`/status` rich rendering** | MVP `/status` is a plain text dump from SQL queries. The fancy box-drawing UI from `/show-plan` is a post-MVP polish. |

### Migration from current pipeline

The existing `dispatch.py` + `watcher.py` + `pr_babysit.py` are **kept running as-is** during MVP build-out. The new daemon is built alongside; the old slash commands can be ported one at a time. Suggested order:

1. Build the daemon skeleton + sqlite schema + UDS RPC + boot reconciliation. *No agent logic yet.*
2. Port `/dispatch` to the daemon, keeping the existing `coder` SKILL.md prompt untouched. The old `watcher.py` is **disabled** at this point; the daemon's sentinel poller takes over. Per-task tmux session is added.
3. Add `arch-task` agent + reviewer/critic spawn + per-task arch decision flow. PR creation moves into the daemon.
4. Port `/pr-babysit` to read from sqlite via the daemon. Add pr-critic + pr-reviewer + arch-top + review-summary.
5. Add `/goals` + `/breakdown` + critique loops + ratify commands. Tasks now flow into the daemon db.
6. Add final-critic + milestone close logic.
7. Migrate the existing `~/.work/dispatches/<plan>/<task>/state.json` files into the new sqlite tables (one-shot script). Old files retained for inspection only.

---

## 10. Open questions for the user

These need a human call before code starts.

1. **Daemon language**: Python (faster to ship, reuses existing dispatch.py logic, no new build step) vs Rust (consistent with v1 ca-daemon plan in `v1_orchestrator/milestones.json`, harder runtime cost up front, no easy `subprocess.Popen`-style spawn). Recommendation: **Python for MVP**, port to Rust opportunistically. **Need confirmation.**
2. **Where does the `ca` CLI live**: `~/.claude/scripts/ca/` (claude-admin tooling) or as a proper installed binary in `~/bin/ca`? This affects install/update story. Recommendation: install script drops a shim in `~/bin/ca` pointing at the python module. **Need confirmation.**
3. **Goal-critique scoring**: numeric (0-100, average across 3 critics) or rubric (5 axes × pass/fail, advance only if all pass)? Recommendation: **numeric average with verdict bucket** (matches existing critic skill). **Confirm.**
4. **Goal threshold**: `≥80` to advance from `awaiting_ratify` to ready-for-user-ratify? Or always surface to user regardless of score? Recommendation: always surface — the loop's purpose is to **improve** the draft, not to gate. **Confirm.**
5. **Per-task arch agent**: ONE per dispatch (the same agent across rounds via send-keys reuse) or fresh per round? Recommendation: **one per dispatch, reused across rounds** — keeps context, saves tokens. **Confirm.**
6. **Who opens the PR** — coder (via `gh pr create` as today) or per-task arch (after approve)? Recommendation: **arch-task opens it** post-approve; coder only pushes commits. This keeps the coder simpler and the arch's "approve" verdict the actual PR-creation trigger. **Confirm; this changes coder/SKILL.md.**
7. **Final critique threshold** for "milestone shipped" vs "gaps_found": 70? 80? Recommendation: **75** (avg of 3 critics). **Confirm.**
8. **Notification channel** when a PR is ready for the user to ship: terminal write (`wall` / `osascript display notification` on macOS) or just leave it for the user to `/status`? Recommendation: macOS `osascript` notification + a row in `events`. **Confirm.**
9. **Worktree retention** post-ship: keep forever (good for forensics) or delete on `/cleanup`? Recommendation: **delete on `/cleanup` but never auto-delete**. **Confirm.**
10. **Conflict with HTML mocks** in `v2_design/`: declare them as "the future web UI exploration, deferred indefinitely" and freeze, or actively reconcile? Recommendation: **archive them** under `v2_design/web_ui_future/` to prevent confusion with this MVP. **Confirm before I move them.**
11. **Plan/milestone canonical store**: keep the JSON files (`registry.json`, `milestones.json`) as the canonical source and mirror to sqlite, OR migrate fully to sqlite and have the JSON files be exports? Recommendation: **mirror for MVP** (no breaking change to existing v1 plans); migrate to sqlite-only in v2.1. **Confirm.**
12. **`--allowedTools` for arch-task agent** — does it need `Bash(gh pr create *)` to open the PR? If yes (per Q6), the per-task arch is no longer "lightweight". Recommendation: yes, allow it; it's a single explicit verb. **Confirm.**
13. **What happens if a critique agent writes `BLOCKED: ...`** — surface to user immediately (interrupt the loop) or treat the round as failed and continue? Recommendation: **surface immediately**; don't tally a blocked round into the score. **Confirm.**
14. **Agent retention** — keep tmux windows for dead agents 30 min? 24h? Forever until `/cleanup`? Recommendation: 24h for windows, forever for `~/.work/agents/<sid>/` scratch dirs (cheap on disk). **Confirm.**
15. **Where does `ca-daemon` start from**: launchd plist on macOS (auto-start at login)? systemd-user on Linux? Or just "user runs `ca-daemon &` once"? Recommendation: **user runs once for MVP**; provide a launchd plist as opt-in. **Confirm.**

---

## Appendix A — Renames vs deletions in the existing skill set

| Existing skill | Action | Notes |
|---|---|---|
| `dispatch` (SKILL.md + scripts/) | **Replaced**. SKILL.md rewritten to call daemon; scripts/ deleted (logic moves into daemon module) |
| `coder` SKILL.md | **Kept**. One delta: per Q6, coder no longer opens the PR — it just pushes commits and signals done. Update workflow section. |
| `critic` SKILL.md | **Kept**. Reused verbatim for both task-critic and pr-critic (the framing is in the user prompt, not the system prompt). |
| `reviewer` SKILL.md | **Kept**. Same — reused for task-reviewer and pr-quality-reviewer. |
| `pr-babysit` SKILL.md + scripts/ | **Replaced**. SKILL.md becomes a thin caller of `ca pr-babysit`; scripts/ deleted. |
| `breakdown` SKILL.md + scripts/ | **Rewritten**. Now invokes daemon's breakdown.draft RPC, which spawns arch-top + breakdown-critique loop. Existing template is kept as the seed prompt for arch-top. |
| `suggest` SKILL.md | **Kept** but rewired. Reads sqlite (via `ca suggest`) instead of the breakdown markdown. |
| `improver` `revisit` `test-builder` | **Kept as-is** (post-merge, single-shot). |
| `goals` (NEW) | New skill. Thin shim → `ca goals.draft`. |
| `ratify-goals` `ratify-breakdown` (NEW) | New skills. |
| `ship` (NEW) | New convenience skill (poll-CI + merge). |
| `cleanup` `history` `agents` `attach` (NEW) | New skills, all thin CLI shims. |

---

## Appendix B — Why these specific tradeoffs

**Why filesystem sentinels over RPC callbacks?** The agent is an LLM. Asking it to make a precise RPC call to localhost is fragile. Asking it to `touch /path/to/done` is one of the most reliable things an LLM-with-Bash can do. The output goes to a known file path it was told about in its prompt. Robust against partial output, claude-process crashes, and surprise resumes.

**Why reuse the same agent across rounds (send-keys) for some flows and fresh for others?** Context is expensive. The coder's working memory of "what I just wrote and why" is worth keeping. The critique agent's independence is worth more than its context — three critics with the same context-prefix would converge on the same opinion. So: reuse coder, fresh critic.

**Why no Postgres / no daemon-pairing?** The user's current flow is local. Adding a server tier triples the moving parts (server, daemon, browser, network) for a single-user single-laptop workflow. SQLite + tmux + UDS gives the same observability with one binary.

**Why immutable goals and append-only critiques?** The whole point of the goals-and-critique phase is that it produces a *contract* the architect can be held to. If goals can be edited mid-flight, the contract evaporates. Immutability is the cheapest way to get auditability — no ORM machinery, just triggers + a `ratified_at` flag. Critiques are append-only for the same reason: the round-2 critique should be readable as "the round-2 critique," not "the round-1 critique mutated."

**Why two separate architect roles (arch-task vs arch-top)?** Different scopes, different prompts, different responsibilities. arch-task lives entirely inside one dispatch — it has the worktree open, the diff in its head, and decides drop/iterate/approve at the *implementation* level. arch-top lives at the *plan* level — it sees aggregate review scores, can override, can update tasks, can bundle drops into "approve + follow-up tasks". Keeping them separate keeps prompts focused; one big "be the architect for everything" prompt would dilute both.

---

## Appendix C — User decisions (locked in)

The 15 open questions from §10 have been resolved. Architect recommendations marked ✓; overrides marked ⚠.

| Q  | Topic                       | Decision                                          | Note |
|----|-----------------------------|---------------------------------------------------|------|
| 1  | Daemon language             | **Rust**                                          | ⚠ overrides Python. Adds cargo workspace + tokio + sqlx/rusqlite + tmux interop to MVP scope. |
| 2  | `ca` CLI location           | Shim in `~/bin/ca` → release binary                | ✓ |
| 3  | Goal-critique scoring       | Numeric 0-100 + verdict + axes                    | ✓ Reuses existing critic skill schema. |
| 4  | Goal-critique threshold     | Always surface to user                            | ✓ Loop improves drafts; user always ratifies. |
| 5  | Per-task arch agent         | One per dispatch, reused                          | ✓ send-keys + `claude --resume <uuid>` for follow-ups. |
| 6  | Who opens the PR            | Per-task architect after approve                  | ✓ Requires `coder/SKILL.md` delta (drop PR-creation step from coder workflow). |
| 7  | Final critique threshold    | **80** average                                    | ⚠ overrides 75. Stricter; more `gaps_found` outcomes. |
| 8  | Notifications               | macOS `osascript` banners + `events` row           | ✓ Linux variant deferred. |
| 9  | Worktree retention          | **Auto-delete on merge, keep on drop**            | ⚠ overrides delete-only-on-`/cleanup`. Adds hook to ship flow. |
| 10 | HTML mocks                  | Archive under `v2_design/web_ui_future/`          | ✓ Done. |
| 11 | Plan canonical store        | JSON mirrored to sqlite                           | ✓ No breaking change to v1. |
| 12 | arch-task allowedTools      | Full git+gh, no editing                           | ✓ Code edits flow through coder via send-keys. |
| 13 | BLOCKED critique            | Surface immediately, halt loop                    | ✓ Don't tally a broken agent's score. |
| 14 | Agent retention             | **Close windows immediately, keep scratch**       | ⚠ overrides 24h-window. Transcript piped to `~/.work/agents/<sid>/transcript.log` before close. |
| 15 | Daemon startup              | User runs manually                                | ✓ launchd plist deferred to opt-in. |

### Downstream implications of the 4 overrides

**Override 1 — Rust daemon (Q1).** Architect's MVP estimate assumed Python (~few hundred LOC reusing existing `dispatch.py`/`watcher.py`/`pr_babysit.py`). Rust requires:
- Cargo workspace under `ca-daemon/` (likely reuse v1's planned `ca-daemon` crate)
- `tokio` for async + signal handling
- `rusqlite` (sync, simpler) or `sqlx` (async, schema-aware) for the DB layer
- Shell-out to `tmux` for window management OR a thin tmux interop crate
- Path-watching crate (`notify`) for sentinel polling, or just timed `std::fs::metadata` polls
- `serde_json` for transcript event parsing
- Subprocess management via `tokio::process::Command` (start_new_session via `nix` for detach)

Scope moves from days to ~1–2 weeks of focused work. The existing `v1_orchestrator` plan already had Rust `ca-daemon` as M1 — that work absorbs ≥half of this.

**Override 2 — Final critique threshold 80 (Q7).** Stricter than recommended 75. Empirically expect more milestones to land in `gaps_found` first time. Mitigation: when `gaps_found`, the arch-top agent must read the missing-validation list and propose concrete follow-up tasks; otherwise users will fight the threshold. Make this explicit in the final-critique skill prompt.

**Override 3 — Auto-delete worktree on merge (Q9).** Add a hook in `/ship` (and the `pr-babysit merge` action): after `gh pr merge` returns success, run `git worktree remove --force <worktree>` then `git branch -D <branch>`. SKIP on `drop` (worktree survives). `state.json` lives at `~/.work/dispatches/<plan>/<task>/`, a separate path — survives worktree removal. Test scenario must verify: merge → worktree gone → state.json still readable.

**Override 4 — Close windows immediately (Q14).** Loses tmux-scrollback as a debug surface. Mitigation: before tmux `kill-window`, the daemon must:
1. Verify the agent wrote its `done.sentinel`
2. `tmux pipe-pane -t <window>` flushes any buffered output to `~/.work/agents/<sid>/transcript.log` (already piped-pane'd at spawn)
3. THEN `tmux kill-window`

User inspection paths post-close: `less ~/.work/agents/<sid>/transcript.log`, `ca history <sid>`, `ca attach <sid>` (errors with "agent closed; see transcript at ..."). The scratch dir survives forever (cheap on disk; `/cleanup` removes).

These are the only deltas from the architect's recommended design. Everything else in this spec stands.

**Why hierarchical tmux instead of one flat list of windows?** With 5+ active dispatches × 4-5 windows each, a flat layout becomes unreadable. Per-dispatch session means `tmux attach -t ca-v2_design-m0a-t3` shows JUST that task's agents. The root `claude-admin` session shows just the cross-cutting work (critique loops, top-level architect). It also means cleanup is one `tmux kill-session` per dispatch, not 5 `kill-window` calls.
