//! Integration tests for the ca-daemon socket lifecycle.
//!
//! These tests spawn the actual `ca-daemon` binary as a subprocess via the
//! Cargo-provided `CARGO_BIN_EXE_ca-daemon` env var. Each test uses a
//! per-test tempdir for the socket so tests run independently.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tempfile::tempdir;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::process::Command;

/// Path to the daemon binary, set by Cargo at build time.
const DAEMON_BIN: &str = env!("CARGO_BIN_EXE_ca-daemon");

/// Poll for the socket file to appear, returning whether it showed up
/// within `timeout`.
async fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn daemon_creates_socket_on_start() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");

    let mut child = Command::new(DAEMON_BIN)
        .env("CA_SOCKET_PATH", &socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ca-daemon");

    assert!(
        wait_for_socket(&socket, Duration::from_secs(3)).await,
        "socket did not appear within 3s"
    );

    let meta = std::fs::metadata(&socket).expect("stat socket");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket should be 0600, got {mode:o}");

    // Cleanup
    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn daemon_serves_banner_on_connection() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");

    let mut child = Command::new(DAEMON_BIN)
        .env("CA_SOCKET_PATH", &socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    let mut stream = UnixStream::connect(&socket).await.expect("connect");
    let mut buf = String::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_string(&mut buf))
        .await
        .expect("read timed out")
        .expect("read failed");

    assert!(
        buf.contains("ca-daemon"),
        "banner should mention ca-daemon, got: {buf:?}"
    );
    assert!(
        buf.contains("proto=1"),
        "banner should include proto, got: {buf:?}"
    );

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn daemon_removes_socket_on_sigterm() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");

    let mut child = Command::new(DAEMON_BIN)
        .env("CA_SOCKET_PATH", &socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    // Send SIGTERM via libc.
    let pid = child.id().expect("child should have a pid");
    let kill_rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    assert_eq!(kill_rc, 0, "libc::kill returned non-zero");

    // Wait for clean exit.
    let exit_status = tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .expect("daemon should exit within 3s")
        .expect("waitpid failed");
    assert!(
        exit_status.success(),
        "daemon should exit cleanly, got {exit_status}"
    );

    // Socket file should be removed.
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
