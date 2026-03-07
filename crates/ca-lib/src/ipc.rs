use crate::events::Event;
use crate::hooks::HookEvent;
use crate::models::Session;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("connection closed")]
    ConnectionClosed,

    #[error("daemon returned error: {0}")]
    DaemonError(String),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping,
    ListSessions,
    GetSession { id: String },
    GetSessionByPane { pane_id: String },
    GetEvents { session_id: String, limit: usize },
    GetRecentEvents { limit: usize },
    HookEvent { event: HookEvent },
    Subscribe,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong,
    SessionList { sessions: Vec<Session> },
    Session { session: Option<Session> },
    Events { events: Vec<Event> },
    HookAck { session_id: Option<String> },
    Error { message: String },
    Subscribed,
    SessionUpdate { sessions: Vec<Session> },
}

pub struct IpcClient {
    reader: tokio::io::BufReader<tokio::io::ReadHalf<tokio::net::UnixStream>>,
    writer: tokio::io::WriteHalf<tokio::net::UnixStream>,
}

impl IpcClient {
    pub async fn connect(path: &std::path::Path) -> Result<Self, IpcError> {
        let stream = tokio::net::UnixStream::connect(path).await?;
        let (read_half, writer) = tokio::io::split(stream);
        Ok(IpcClient {
            reader: tokio::io::BufReader::new(read_half),
            writer,
        })
    }

    pub async fn send(&mut self, request: &Request) -> Result<Response, IpcError> {
        use tokio::io::AsyncWriteExt;

        let json = serde_json::to_string(request)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;

        self.recv_response().await
    }

    /// Read a single response/push message from the connection.
    /// Used by subscribers to receive SessionUpdate pushes.
    pub async fn recv_response(&mut self) -> Result<Response, IpcError> {
        use tokio::io::AsyncBufReadExt;

        let mut line = String::new();
        let bytes = self.reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(IpcError::ConnectionClosed);
        }

        let response: Response = serde_json::from_str(line.trim())?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventType;
    use crate::models::{Session, SessionState};

    fn make_session(id: &str, pane_id: &str, state: SessionState) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
            project_id: None,
            plan_step_id: None,
        }
    }

    // -- Request round-trips --

    #[test]
    fn test_request_ping_roundtrip() {
        let req = Request::Ping;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_list_sessions_roundtrip() {
        let req = Request::ListSessions;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_get_session_roundtrip() {
        let req = Request::GetSession {
            id: "sess-abc".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_get_session_by_pane_roundtrip() {
        let req = Request::GetSessionByPane {
            pane_id: "%3".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_get_events_roundtrip() {
        let req = Request::GetEvents {
            session_id: "sess-1".to_string(),
            limit: 20,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_get_recent_events_roundtrip() {
        let req = Request::GetRecentEvents { limit: 50 };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    // -- Response round-trips --

    #[test]
    fn test_response_pong_roundtrip() {
        let resp = Response::Pong;
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_session_some_roundtrip() {
        let session = make_session("sess-1", "%0", SessionState::Working);
        let resp = Response::Session {
            session: Some(session),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_session_none_roundtrip() {
        let resp = Response::Session { session: None };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_session_list_roundtrip() {
        let sessions = vec![
            make_session("sess-1", "%0", SessionState::Idle),
            make_session("sess-2", "%1", SessionState::Working),
        ];
        let resp = Response::SessionList { sessions };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_events_roundtrip() {
        let events = vec![
            Event {
                id: 1,
                session_id: "sess-1".to_string(),
                event_type: EventType::SessionDiscovered,
                payload: None,
                timestamp: 1706500000,
            },
            Event {
                id: 2,
                session_id: "sess-1".to_string(),
                event_type: EventType::StateChanged {
                    from: SessionState::Idle,
                    to: SessionState::Working,
                },
                payload: None,
                timestamp: 1706500001,
            },
        ];
        let resp = Response::Events { events };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_error_roundtrip() {
        let resp = Response::Error {
            message: "session not found".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    // -- JSON shape assertions --

    #[test]
    fn test_request_ping_json_shape() {
        let json = serde_json::to_string(&Request::Ping).unwrap();
        assert!(json.contains("\"type\":\"ping\""));
    }

    #[test]
    fn test_response_pong_json_shape() {
        let json = serde_json::to_string(&Response::Pong).unwrap();
        assert!(json.contains("\"type\":\"pong\""));
    }

    #[test]
    fn test_response_error_json_shape() {
        let json = serde_json::to_string(&Response::Error {
            message: "oops".to_string(),
        })
        .unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"oops\""));
    }

    #[test]
    fn test_request_hook_event_roundtrip() {
        use crate::hooks::HookEvent;

        let req = Request::HookEvent {
            event: HookEvent {
                hook_type: "PostToolUse".to_string(),
                session_id: Some("sess-1".to_string()),
                working_dir: "/home/user/project".to_string(),
                timestamp: 1706500000,
                payload: Some(serde_json::json!({"tool": "Read", "path": "/foo"})),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_response_hook_ack_roundtrip() {
        let resp = Response::HookAck {
            session_id: Some("sess-1".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    // -- Subscribe/Subscribed/SessionUpdate round-trips --

    #[test]
    fn test_request_subscribe_roundtrip() {
        let req = Request::Subscribe;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_request_subscribe_json_shape() {
        let json = serde_json::to_string(&Request::Subscribe).unwrap();
        assert!(json.contains("\"type\":\"subscribe\""));
    }

    #[test]
    fn test_response_subscribed_roundtrip() {
        let resp = Response::Subscribed;
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_subscribed_json_shape() {
        let json = serde_json::to_string(&Response::Subscribed).unwrap();
        assert!(json.contains("\"type\":\"subscribed\""));
    }

    #[test]
    fn test_response_session_update_roundtrip() {
        let sessions = vec![
            make_session("sess-1", "%0", SessionState::Working),
            make_session("sess-2", "%1", SessionState::Idle),
        ];
        let resp = Response::SessionUpdate { sessions };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_session_update_empty_roundtrip() {
        let resp = Response::SessionUpdate {
            sessions: Vec::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_response_session_update_json_shape() {
        let json = serde_json::to_string(&Response::SessionUpdate {
            sessions: vec![make_session("s1", "%0", SessionState::Idle)],
        })
        .unwrap();
        assert!(json.contains("\"type\":\"session_update\""));
        assert!(json.contains("\"sessions\""));
    }
}
