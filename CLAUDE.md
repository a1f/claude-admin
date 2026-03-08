# Claude Project Settings

## Rust Completion Rules

Before claiming any Rust coding task is complete, run and pass all of the following:

1. `cargo fmt-check`
2. `cargo lint-strict`
3. `cargo test --workspace --all-targets`

If any command fails, do not claim completion. Fix the issues first or report that the task is still in progress.

Only skip checks when the user explicitly requests that skip.
