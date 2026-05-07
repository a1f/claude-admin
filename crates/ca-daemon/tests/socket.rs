//! Integration tests for the ca-daemon socket lifecycle.
//!
//! These tests spawn the actual `ca-daemon` binary as a subprocess. Each
//! test uses a per-test tempdir for the socket so they run independently.

use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use std::time::Duration;

use tempfile::tempdir;
use tokio::process::Command;

mod common;
use common::{DAEMON_BIN, send_sigterm, spawn_daemon, wait_for_socket};

#[tokio::test]
async fn daemon_creates_socket_on_start() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");

    let mut child = spawn_daemon(&socket);
    assert!(
        wait_for_socket(&socket, Duration::from_secs(3)).await,
        "socket did not appear within 3s"
    );

    let meta = std::fs::metadata(&socket).expect("stat socket");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket should be 0600, got {mode:o}");

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn daemon_removes_socket_on_sigterm() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");

    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    send_sigterm(&child);
    let exit = tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .expect("daemon should exit within 3s")
        .expect("waitpid failed");
    assert!(exit.success(), "daemon should exit cleanly, got {exit}");
    assert!(
        !socket.exists(),
        "socket should be removed on shutdown, still at {}",
        socket.display()
    );
}

#[tokio::test]
async fn daemon_refuses_to_start_if_socket_exists() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    std::fs::write(&socket, b"").expect("seed socket file");

    let output = Command::new(DAEMON_BIN)
        .env("CA_SOCKET_PATH", &socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run ca-daemon");

    assert!(
        !output.status.success(),
        "daemon should refuse to start when socket already exists"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "stderr should mention 'already exists', got: {stderr}"
    );
}
