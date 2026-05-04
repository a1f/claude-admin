//! RPC envelope — typed messages between `ca` (CLI) and `ca-daemon`.
//!
//! Messages are tagged via `"type"` so unknown variants on either side
//! produce a clear deserialization error rather than silently dropping
//! to a default.

use serde::{Deserialize, Serialize};

use crate::Task;

/// All requests the CLI can send to the daemon. Tagged in JSON via `"type"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcRequest {
    /// Liveness probe.
    Ping,
    /// Daemon version + protocol version handshake.
    Version,
    /// Register a new architector for a (repo, milestone).
    ArchitectRegister {
        repo: String,
        milestone_id: String,
        issue_url: String,
    },
    /// List the tasks attached to an architector.
    TaskList { architector_id: String },
    /// Fetch one task by id.
    TaskGet { task_id: String },
}

/// All responses the daemon can return. Tagged in JSON via `"type"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResponse {
    /// Pong + server uptime in seconds.
    Pong { uptime_s: u64 },
    /// Daemon version + protocol version.
    Version { daemon: String, protocol: u32 },
    /// Architector created and persisted.
    ArchitectorRegistered { architector_id: String },
    /// Task list for an architector.
    TaskList { tasks: Vec<Task> },
    /// One task.
    Task(Box<Task>),
    /// Generic error response.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStatus;

    #[test]
    fn rpc_request_ping_roundtrip() {
        let r = RpcRequest::Ping;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(r, parsed);
    }

    #[test]
    fn rpc_request_with_fields_roundtrip() {
        let r = RpcRequest::ArchitectRegister {
            repo: "a1f/claude-admin".to_owned(),
            milestone_id: "M1".to_owned(),
            issue_url: "https://github.com/a1f/claude-admin/issues/2".to_owned(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(r, parsed);
    }

    #[test]
    fn rpc_response_pong_roundtrip() {
        let r = RpcResponse::Pong { uptime_s: 3600 };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: RpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(r, parsed);
    }

    #[test]
    fn rpc_response_task_roundtrip() {
        let task = Task {
            id: "M0-T3".to_owned(),
            title: "ca-lib core types".to_owned(),
            deliverable: "Architector + Task + Commit + Review + Critique + RPC types".to_owned(),
            blockers: vec!["M0-T2 merged".to_owned()],
            status: TaskStatus::Coding,
            estimated_loc: Some(150),
        };
        let r = RpcResponse::Task(Box::new(task.clone()));
        let json = serde_json::to_string(&r).unwrap();
        let parsed: RpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(r, parsed);
    }

    #[test]
    fn rpc_unknown_variant_errors() {
        // Required by M0-T3 spec: unknown discriminator produces an Err, not Ok.
        let bad = r#"{"type":"non_existent_request"}"#;
        let result: Result<RpcRequest, _> = serde_json::from_str(bad);
        assert!(
            result.is_err(),
            "unknown variant must error, got {result:?}"
        );
    }

    #[test]
    fn rpc_missing_tag_errors() {
        // Defensive: no "type" field → Err.
        let bad = r#"{"foo":"bar"}"#;
        let result: Result<RpcRequest, _> = serde_json::from_str(bad);
        assert!(result.is_err());
    }
}
