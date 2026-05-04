# claude_admin

Local-machine orchestrator for Claude Code work — drives a feature from plan → breakdown → dispatch → review → merge with a daemon, an architector per milestone, a task-processor per dispatched task, and a TUI client.

> **Active plan:** [ca_v1](v1_orchestrator/00-final-plan.html) · issue [a1f/claude-admin#2](https://github.com/a1f/claude-admin/issues/2)
> **Future:** the [v2_design](v2_design/00-final-plan.html) web admin grows from the same daemon after ca_v1 ships.

## What this is

A Rust workspace + a set of orchestrator skills. The workspace builds the runtime: a daemon that owns Claude sessions, an architector (interactive Claude session in a tmux pane) that drives one milestone, task-processors that drive one task each, and a ratatui TUI to watch it. The skills (in `skills/`) are the system prompts and helpers that those agents invoke.

You run plans, the daemon runs the work.

## Architecture (one line each)

| Piece | What it does |
|---|---|
| `ca-daemon` | One per machine. UDS server. SQLite state. Owns claude sessions. |
| `architector` | One per `(repo, milestone)`. Tmux pane with claude + architector skill. Decides drop / accept / fix. |
| `ca-task-processor` | One per dispatched task. State machine (read → plan-actions → coder → review → decide). |
| `coder` | Tmux pane with claude + coder skill. Fresh worktree per task. |
| `reviewer` / `critic` | `claude -p` headless subprocesses. Reviewers find code defects; critics judge goal-fit (1-100). |
| `ca-tui` | ratatui client. Connects to daemon over UDS. Shows progress + alerts. |

Full architecture diagram in [`v1_orchestrator/00-final-plan.html`](v1_orchestrator/00-final-plan.html).

## Repo layout

| Path | What's there |
|---|---|
| `crates/ca-lib/` | Shared types: `Architector`, `Task`, `Commit`, `ReviewResult`, `CritiqueResult`, `RpcRequest`/`RpcResponse` |
| `crates/ca-daemon/` | Daemon binary (placeholder until M1) |
| `crates/ca/` | CLI entry for humans + architectors (placeholder until M1) |
| `crates/ca-tui/` | ratatui client (placeholder until M7) |
| `skills/` | Orchestrator skills: `breakdown`, `suggest`, `dispatch`, `coder`, `reviewer`, `critic`, `pr-babysit`, `improver`, `test-builder`, `revisit` |
| `v1_orchestrator/` | Active ca_v1 plan + milestones + breakdowns |
| `v2_design/` | Design brief + UI mocks for the eventual web admin |
| `install.sh` / `uninstall.sh` | Discovery-based skill installer (symlinks `<repo>/skills/*` into `~/.claude/skills/`) |
| `plans-registry.template.json` | Starting registry; copied to `~/.claude/plans/registry.json` on first install |

## Build

```sh
git clone git@github.com:a1f/claude-admin.git
cd claude-admin
cargo build --workspace
```

Per-binary placeholders (`ca-daemon`, `ca`, `ca-tui`) print `not yet implemented` and exit 0 until their respective milestones land. `ca-lib` is fully wired with serde-tested core types.

## Tests + gates

```sh
cargo fmt-check                       # formatting
cargo lint-strict                     # clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets  # all tests
```

CI runs the same three on every push and PR — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (lands in M0-T4).

## Install the skills

```sh
./install.sh
```

Creates symlinks `~/.claude/skills/<name>` → `<repo>/skills/<name>`. Re-run after pulling new skills. `./uninstall.sh` removes only the symlinks pointing back to this repo.

The orchestrator skills:

| Skill | One-liner |
|---|---|
| `/breakdown` | Milestone → PR-shaped tasks → child GitHub issue with checklist |
| `/suggest` | Scan the active breakdown, list dispatchable tasks (blockers met) |
| `/dispatch` | Spawn a headless coder in a fresh worktree for a chosen task |
| `coder` | _Internal._ System prompt for the coder agent |
| `reviewer` | _Internal._ System prompt for security / bugs / quality reviewer agents |
| `critic` | _Internal._ System prompt for goal-fit critics (1-100 scoring) |
| `/pr-babysit` | Post-review human decision: ready / merge / drop / iterate |
| `/improver` | Post-merge: refactor opportunities the coder couldn't take |
| `/test-builder` | Post-merge: under-tested paths + scenarios to add |
| `/revisit` | Post-merge: amendments to downstream plan |

The user-invocable skills (`/breakdown`, `/suggest`, `/dispatch`, `/pr-babysit`, `/improver`, `/test-builder`, `/revisit`) work today via Python scripts. Their logic moves into the Rust workspace over M3–M9 of ca_v1.

## Plans

| Codename | What | Status |
|---|---|---|
| [ca_v1](v1_orchestrator/00-final-plan.html) | Local Rust orchestrator (this work) | Active — M0 in flight |
| [v2_design](v2_design/00-final-plan.html) | Web admin built on ca_v1's daemon | Future — design brief only |

`~/.claude/plans/registry.json` is the per-machine registry of plans the skills know about. `install.sh` creates it from `plans-registry.template.json` on first run.

## Legacy code

The pre-v1 Rust crates (the original ca-lib + tui + cli + daemon — terminal UI orchestrator) are archived to the [`legacy/pre-v1`](https://github.com/a1f/claude-admin/tree/legacy/pre-v1) branch.

```sh
git fetch origin legacy/pre-v1:legacy/pre-v1
git diff main legacy/pre-v1 -- crates/   # see what changed
git checkout legacy/pre-v1                # browse the old code
```

The new ca_v1 workspace under `crates/` replaces the archived code. None of it carries over directly; concepts that were good (tmux session detection, gh wrappers, phase-based orchestration) get re-implemented under the new shape.

## Contributing

Work is organized as **plan → milestone → task → PR**:

1. Plans live in `v1_orchestrator/00-final-plan.html` and similar.
2. Each milestone gets a child GitHub issue with a task checklist (e.g. [#3](https://github.com/a1f/claude-admin/issues/3) for M0).
3. Each task is one PR-shaped unit (~150 LOC). PR title `[<task-id>] <title>`. Draft until reviewers + you sign off.
4. Until ca_v1 self-hosts (~M4), tasks are dispatched manually — the orchestrator builds itself.

See [`v1_orchestrator/breakdowns/M0.md`](v1_orchestrator/breakdowns/M0.md) for the current milestone's tasks.

## License

MIT. Personal project; not currently accepting outside contributions.
