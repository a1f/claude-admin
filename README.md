# claude_admin

Local-machine + (eventually) self-hosted orchestrator for Claude Code work — plan, break down, dispatch, review, merge.

## Active plan

[`v1_orchestrator/00-final-plan.html`](v1_orchestrator/00-final-plan.html) — **ca_v1**, the local Rust orchestrator we're building now.

Issue: [a1f/claude-admin#2](https://github.com/a1f/claude-admin/issues/2)

## Future direction

[`v2_design/00-final-plan.html`](v2_design/00-final-plan.html) — web admin design brief, post-v1.

## Repo layout

| Path | What's there |
|---|---|
| `skills/` | Orchestrator skills (breakdown, suggest, dispatch, coder, reviewer, critic, pr-babysit, improver, test-builder, revisit) |
| `v1_orchestrator/` | Active ca_v1 plan + breakdowns + (later) TUI mocks |
| `v2_design/` | Design brief + UI mocks for the eventual web admin |
| `install.sh` / `uninstall.sh` | Discovery-based skill installer (symlinks `<repo>/skills/*` into `~/.claude/skills/`) |
| `plans-registry.template.json` | Starting registry — install.sh copies to `~/.claude/plans/registry.json` on first run |
| `crates/` (post-M0-T2) | New Rust workspace: `ca-lib`, `ca-daemon`, `ca`, `ca-tui` |

## Build (post-M0-T2)

```sh
git clone git@github.com:a1f/claude-admin.git
cd claude-admin
./install.sh
cargo build --workspace
```

## Legacy code

The pre-v1 Rust crates (the old `ca-lib` + `tui` + `cli` + `daemon` — terminal UI orchestrator) have been archived to the [`legacy/pre-v1`](https://github.com/a1f/claude-admin/tree/legacy/pre-v1) branch for reference.

Compare patterns with:

```sh
git fetch origin legacy/pre-v1:legacy/pre-v1
git diff main legacy/pre-v1 -- crates/
```

The new ca_v1 workspace lands in `crates/` over M0-T2 and beyond, replacing the archived code.
