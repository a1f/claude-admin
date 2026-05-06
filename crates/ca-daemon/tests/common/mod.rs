//! Shared helpers for integration tests.
//!
//! Cargo treats every direct `.rs` file in `tests/` as its own test binary,
//! but subdirectories (and their `mod.rs`) are shared modules — usable from
//! any sibling test file via `mod common;`.
//!
//! Each test binary may use only a subset of the helpers. Mark the module
//! `dead_code`-tolerant so that's not a lint error.

#![allow(dead_code)]

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};

/// Path to the `ca-daemon` binary, set by Cargo at build time.
pub const DAEMON_BIN: &str = env!("CARGO_BIN_EXE_ca-daemon");

/// Spawn the daemon as a child process pointed at the given socket path.
/// Stdout / stderr are silenced unless the caller swaps them in.
pub fn spawn_daemon(socket: &Path) -> Child {
    Command::new(DAEMON_BIN)
        .env("CA_SOCKET_PATH", socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ca-daemon")
}

/// Poll for the socket file to appear; returns whether it showed up
/// within the timeout.
pub async fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Send SIGTERM via libc (`tokio::process::Child::kill` is SIGKILL).
pub fn send_sigterm(child: &Child) {
    let pid = child.id().expect("child should have a pid");
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    assert_eq!(rc, 0, "libc::kill returned non-zero");
}
