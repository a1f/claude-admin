# Codex Project Instructions

## Rust Done Gate

For any task that modifies Rust code (`*.rs`) or Cargo manifests (`Cargo.toml`, `Cargo.lock`), do not mark the task as done until all required checks have run and passed.

Required commands:

1. `cargo fmt-check`
2. `cargo lint-strict`
3. `cargo test --workspace --all-targets`

If a required check fails:

1. Keep working until fixed, or
2. Report the exact failure and explicitly state the task is not done.

Checks may be skipped only if the user explicitly asks to skip them.
