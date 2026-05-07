//! RPC dispatch over the UDS.
//!
//! Newline-delimited JSON. Each line is an `RpcRequest`; the daemon writes one
//! `RpcResponse` line per request. Connections survive multiple round-trips
//! until the client closes them (EOF). Malformed JSON or unknown variants
//! produce an `RpcResponse::Error` and the connection stays open for the next
//! request — only socket-level errors close it.

use std::sync::Arc;
use std::time::Instant;

use ca_lib::{RpcRequest, RpcResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, warn};

/// Wire-protocol version. Increment when the request/response shape changes
/// in a breaking way.
pub const PROTOCOL_VERSION: u32 = 1;

/// Per-daemon state surfaced into RPC handlers.
#[derive(Clone)]
pub struct AppState {
    pub started_at: Instant,
}

/// Read newline-delimited requests from the stream and write one response
/// line per request. Returns when the client closes the connection.
pub async fn handle_connection(stream: UnixStream, state: Arc<AppState>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                debug!(error = %e, "rpc read error");
                break;
            }
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let response = match serde_json::from_str::<RpcRequest>(trimmed) {
            Ok(req) => dispatch(req, &state),
            Err(e) => RpcResponse::Error {
                message: format!("parse error: {e}"),
            },
        };

        let mut out = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "serializing response");
                break;
            }
        };
        out.push('\n');
        if write_half.write_all(out.as_bytes()).await.is_err() {
            break;
        }
        if write_half.flush().await.is_err() {
            break;
        }
    }

    let _ = write_half.shutdown().await;
}

/// Pure RPC dispatch — kept side-effect-free for unit testing.
fn dispatch(req: RpcRequest, state: &AppState) -> RpcResponse {
    match req {
        RpcRequest::Ping => RpcResponse::Pong {
            uptime_s: state.started_at.elapsed().as_secs(),
        },
        RpcRequest::Version => RpcResponse::Version {
            daemon: ca_lib::version().to_owned(),
            protocol: PROTOCOL_VERSION,
        },
        RpcRequest::ArchitectRegister { .. } => RpcResponse::Error {
            message: "ArchitectRegister not yet implemented (lands in M2)".to_owned(),
        },
        RpcRequest::TaskList { .. } => RpcResponse::Error {
            message: "TaskList not yet implemented (lands in M3)".to_owned(),
        },
        RpcRequest::TaskGet { .. } => RpcResponse::Error {
            message: "TaskGet not yet implemented (lands in M3)".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_state() -> AppState {
        AppState {
            started_at: Instant::now(),
        }
    }

    #[test]
    fn dispatch_ping_returns_pong() {
        let resp = dispatch(RpcRequest::Ping, &fresh_state());
        match resp {
            RpcResponse::Pong { uptime_s } => assert_eq!(uptime_s, 0),
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_version_returns_daemon_and_protocol() {
        let resp = dispatch(RpcRequest::Version, &fresh_state());
        match resp {
            RpcResponse::Version { daemon, protocol } => {
                assert!(!daemon.is_empty());
                assert_eq!(protocol, PROTOCOL_VERSION);
                assert_eq!(daemon, ca_lib::version());
            }
            other => panic!("expected Version, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_unimplemented_request_returns_error() {
        let resp = dispatch(
            RpcRequest::TaskList {
                architector_id: "x".into(),
            },
            &fresh_state(),
        );
        match resp {
            RpcResponse::Error { message } => {
                assert!(message.contains("not yet"), "message: {message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
