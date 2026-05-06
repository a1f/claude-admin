//! Integration tests for the ca-daemon JSON-RPC protocol.
//!
//! Spawns the daemon as a subprocess, connects via `UnixStream`, exercises
//! the wire protocol end-to-end. Each line is one request or one response.

use std::time::Duration;

use ca_lib::{RpcRequest, RpcResponse};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

mod common;
use common::{spawn_daemon, wait_for_socket};

/// Send a single RPC request line, read one response line, return parsed.
async fn round_trip(stream: &mut UnixStream, req: &RpcRequest) -> RpcResponse {
    let json = serde_json::to_string(req).expect("serialize request");
    let line = format!("{json}\n");
    stream
        .write_all(line.as_bytes())
        .await
        .expect("write request");
    stream.flush().await.expect("flush");

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut buf))
        .await
        .expect("response timed out")
        .expect("read response");
    serde_json::from_str(buf.trim_end()).unwrap_or_else(|e| panic!("parse {buf:?}: {e}"))
}

/// Like `round_trip` but the input is a raw byte slice (for malformed JSON).
async fn round_trip_raw(stream: &mut UnixStream, raw: &[u8]) -> String {
    stream.write_all(raw).await.expect("write raw");
    stream.flush().await.expect("flush");

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut buf))
        .await
        .expect("response timed out")
        .expect("read response");
    buf
}

#[tokio::test]
async fn version_handshake_returns_daemon_and_protocol() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    let mut stream = UnixStream::connect(&socket).await.expect("connect");
    let resp = round_trip(&mut stream, &RpcRequest::Version).await;
    match resp {
        RpcResponse::Version { daemon, protocol } => {
            assert_eq!(daemon, ca_lib::version());
            assert_eq!(protocol, 1);
        }
        other => panic!("expected Version, got {other:?}"),
    }

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn ping_returns_pong_with_uptime() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    let mut stream = UnixStream::connect(&socket).await.expect("connect");
    let resp = round_trip(&mut stream, &RpcRequest::Ping).await;
    match resp {
        RpcResponse::Pong { uptime_s } => {
            // u64; just assert it's a valid count (small number on a fresh daemon)
            assert!(uptime_s < 60, "uptime should be small, got {uptime_s}");
        }
        other => panic!("expected Pong, got {other:?}"),
    }

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn unknown_request_returns_error() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    let mut stream = UnixStream::connect(&socket).await.expect("connect");
    let raw = round_trip_raw(&mut stream, b"{\"type\":\"non_existent_request\"}\n").await;
    let parsed: RpcResponse = serde_json::from_str(raw.trim_end()).expect("parse");
    match parsed {
        RpcResponse::Error { message } => {
            assert!(
                message.to_lowercase().contains("parse")
                    || message.to_lowercase().contains("unknown"),
                "expected parse-error message, got {message:?}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn malformed_json_returns_error_not_crash() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    let mut stream = UnixStream::connect(&socket).await.expect("connect");

    // First request is broken JSON.
    let raw = round_trip_raw(&mut stream, b"not json\n").await;
    let bad: RpcResponse = serde_json::from_str(raw.trim_end()).expect("parse");
    assert!(matches!(bad, RpcResponse::Error { .. }));

    // Daemon must still serve a follow-up request on the same connection.
    let resp = round_trip(&mut stream, &RpcRequest::Ping).await;
    assert!(
        matches!(resp, RpcResponse::Pong { .. }),
        "daemon should still respond to ping after malformed input, got {resp:?}"
    );

    child.kill().await.ok();
    let _ = child.wait().await;
}

#[tokio::test]
async fn concurrent_clients_each_get_their_own_response() {
    let dir = tempdir().unwrap();
    let socket = dir.path().join("ca.sock");
    let mut child = spawn_daemon(&socket);
    assert!(wait_for_socket(&socket, Duration::from_secs(3)).await);

    // Three concurrent ping/version round-trips.
    let socket = socket.clone();
    let mut handles = Vec::new();
    for i in 0..3 {
        let s = socket.clone();
        handles.push(tokio::spawn(async move {
            let mut stream = UnixStream::connect(&s).await.expect("connect");
            let req = if i % 2 == 0 {
                RpcRequest::Ping
            } else {
                RpcRequest::Version
            };
            round_trip(&mut stream, &req).await
        }));
    }

    let mut got_pong = 0;
    let mut got_version = 0;
    for h in handles {
        match h.await.expect("join") {
            RpcResponse::Pong { .. } => got_pong += 1,
            RpcResponse::Version { .. } => got_version += 1,
            other => panic!("unexpected response: {other:?}"),
        }
    }
    assert_eq!(got_pong, 2, "expected 2 pongs");
    assert_eq!(got_version, 1, "expected 1 version");

    child.kill().await.ok();
    let _ = child.wait().await;
}
