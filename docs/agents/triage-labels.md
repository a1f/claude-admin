# Triage label vocabulary

Canonical roles → GitHub label strings used in `a1f/claude-admin`:

- `needs-triage` — maintainer needs to evaluate
- `needs-info` — waiting on reporter
- `ready-for-agent` — fully specified, AFK-ready (agent can pick it up with no human context)
- `ready-for-human` — needs human implementation
- `wontfix` — will not be actioned

Project-specific:

- `milestone-ratified` — milestone PRD frozen; do not edit issue body without removing label
- `goals-ratified` — goals issue frozen; same rule
- `milestone:<id>` — tags slice issues with their parent milestone (e.g. `milestone:M1`)
- `breakdown` — issue is a milestone breakdown / PR-task list
