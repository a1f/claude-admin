# Claude Project Settings

## Rust Completion Rules

Before claiming any Rust coding task is complete, run and pass all of the following:

1. `cargo fmt-check`
2. `cargo lint-strict`
3. `cargo test --workspace --all-targets`

If any command fails, do not claim completion. Fix the issues first or report that the task is still in progress.

Only skip checks when the user explicitly requests that skip.

## Agent skills

This repo uses the mattpocock skill catalog (vendored under `skills/`). Per-repo config:

- **Issue tracker** — see [`docs/agents/issue-tracker.md`](docs/agents/issue-tracker.md). GitHub Issues on `a1f/claude-admin`.
- **Triage labels** — see [`docs/agents/triage-labels.md`](docs/agents/triage-labels.md).
- **Domain docs** — see [`docs/agents/domain.md`](docs/agents/domain.md). Single-context: `CONTEXT.md` + `docs/adr/` at root.

M1 pipeline: `/roadmap-plan` → `/milestone` → `/to-issues` → `/architector` → `/coder` → `/cc-review` + `/critic` → `/pr-babysit` → `/distill-lessons`. See M1 PRD: [#16](https://github.com/a1f/claude-admin/issues/16). Breakdown: [#17](https://github.com/a1f/claude-admin/issues/17).

`/pr-babysit` is the AFK polling loop (S10): watches CI, routes bot comments to tier-1 (inline fix) / tier-2 (printed `/coder` tmux command) / tier-3 (escalate to slice issue via `architect-attention` label). On CI red it invokes `/diagnose` as an analysis-only subagent. After it exits:

- `[READY TO MERGE]` → `gh pr merge <N> --squash --delete-branch`
- `[ESCALATED]` → read the slice issue comment; decide whether to redispatch `/coder`, drop the PR (`gh pr close <N> --comment "<reason>"`), or keep iterating
- `[MAX ROUNDS EXHAUSTED]` → same as ESCALATED

(The earlier `/pr-decide` shell-wrapper skill was dropped in S10 — its subcommands were all replaceable by one-line `gh` invocations, made redundant by /coder shipping NON-DRAFT PRs and /cc-review + /critic posting summaries directly to PRs.)

`/milestone` uses `/grill-me` directly (the `/grill-with-docs` variant was dropped 2026-05-17). Programmatic tmux runtime + daemon + sqlite are M2 work; M1 is human-as-orchestrator with manual tmux + thin shell wrappers.
