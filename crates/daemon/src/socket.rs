use ca_lib::db::Database;
use ca_lib::hooks::apply_hook_event;
use ca_lib::ipc::{Request, Response};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

#[derive(Error, Debug)]
pub enum SocketError {
    #[error("failed to bind socket: {0}")]
    Bind(#[from] std::io::Error),
    #[error("socket already in use by running daemon")]
    InUse,
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("request too large (>{MAX_REQUEST_BYTES} bytes)")]
    RequestTooLarge,
}

const MAX_REQUEST_BYTES: usize = 64 * 1024;

pub struct SocketServer {
    listener: UnixListener,
    path: PathBuf,
}

impl SocketServer {
    pub async fn bind(path: &Path, pid_running: bool) -> Result<Self, SocketError> {
        if path.exists() {
            if pid_running {
                return Err(SocketError::InUse);
            }
            tracing::warn!(path = %path.display(), "Removing stale socket");
            std::fs::remove_file(path)?;
        }

        let listener = UnixListener::bind(path)?;
        tracing::info!(path = %path.display(), "Socket server listening");

        Ok(SocketServer {
            listener,
            path: path.to_owned(),
        })
    }

    pub async fn accept(&self) -> Result<Connection, SocketError> {
        let (stream, _) = self.listener.accept().await?;
        Ok(Connection::new(stream))
    }

    pub fn cleanup(&self) -> Result<(), std::io::Error> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
            tracing::info!(path = %self.path.display(), "Socket removed");
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            tracing::error!(error = %e, "Failed to cleanup socket on drop");
        }
    }
}

pub struct Connection {
    reader: BufReader<tokio::io::ReadHalf<UnixStream>>,
    writer: tokio::io::WriteHalf<UnixStream>,
}

impl Connection {
    fn new(stream: UnixStream) -> Self {
        let (read_half, write_half) = tokio::io::split(stream);
        Connection {
            reader: BufReader::new(read_half),
            writer: write_half,
        }
    }

    pub async fn recv(&mut self) -> Result<Option<Request>, SocketError> {
        let mut line = String::new();
        loop {
            let bytes_read = self.reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Ok(None);
            }
            if line.len() > MAX_REQUEST_BYTES {
                return Err(SocketError::RequestTooLarge);
            }
            if line.ends_with('\n') {
                break;
            }
        }
        let request: Request = serde_json::from_str(line.trim())?;
        Ok(Some(request))
    }

    pub async fn send(&mut self, response: &Response) -> Result<(), SocketError> {
        let json = serde_json::to_string(response)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}

pub async fn handle_connection(
    mut conn: Connection,
    db: Arc<Mutex<Database>>,
) -> Result<(), SocketError> {
    while let Some(request) = conn.recv().await? {
        tracing::debug!(?request, "Received IPC request");
        let response = dispatch_request(request, Arc::clone(&db)).await;
        conn.send(&response).await?;
    }
    Ok(())
}

async fn dispatch_request(request: Request, db: Arc<Mutex<Database>>) -> Response {
    match request {
        Request::Ping => Response::Pong,

        Request::ListSessions => run_db(db, |db| db.list_sessions())
            .await
            .map(|sessions| Response::SessionList { sessions })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::GetSession { id } => {
            run_db(db, move |db| db.get_session(&id))
                .await
                .map(|session| Response::Session { session })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::GetSessionByPane { pane_id } => {
            run_db(db, move |db| db.get_session_by_pane(&pane_id))
                .await
                .map(|session| Response::Session { session })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::GetEvents { session_id, limit } => {
            run_db(db, move |db| db.get_events(&session_id, limit))
                .await
                .map(|events| Response::Events { events })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::GetRecentEvents { limit } => {
            run_db(db, move |db| db.get_recent_events(limit))
                .await
                .map(|events| Response::Events { events })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::HookEvent { event } => {
            run_db(db, move |db| {
                apply_hook_event(db, &event).map_err(|e| match e {
                    ca_lib::hooks::HookError::Db(db_err) => db_err,
                })
            })
            .await
            .map(|session_id| Response::HookAck { session_id })
            .unwrap_or_else(|e| Response::Error { message: e })
        }
    }
}

/// Run a synchronous database closure on the blocking thread pool.
/// Returns Ok(T) on success, Err(String) on any failure (task panic or DB error).
async fn run_db<F, T>(db: Arc<Mutex<Database>>, f: F) -> Result<T, String>
where
    F: FnOnce(&Database) -> Result<T, ca_lib::db::DbError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let db = db.lock().expect("database mutex poisoned");
        f(&db)
    })
    .await
    .map_err(|e| format!("task panic: {e}"))?
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_lib::events::EventType;
    use ca_lib::models::{Session, SessionState};
    use tempfile::tempdir;
    use tokio::net::UnixStream;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    fn create_test_session(id: &str, pane_id: &str) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state: SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
        }
    }

    // -- Existing structural tests (unchanged) --

    #[tokio::test]
    async fn test_socket_created() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        assert!(socket_path.exists());

        drop(server);
    }

    #[tokio::test]
    async fn test_stale_socket_cleanup() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        std::fs::write(&socket_path, "").unwrap();
        assert!(socket_path.exists());

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        assert!(socket_path.exists());

        drop(server);
    }

    #[tokio::test]
    async fn test_socket_cleanup_on_shutdown() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        {
            let _server = SocketServer::bind(&socket_path, false).await.unwrap();
            assert!(socket_path.exists());
        }

        assert!(!socket_path.exists());
    }

    // -- Handler dispatch tests --

    #[tokio::test]
    async fn test_handler_ping_returns_pong() {
        let (db, _dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(Request::Ping, db).await;
        assert_eq!(response, Response::Pong);
    }

    #[tokio::test]
    async fn test_handler_list_sessions_empty_db() {
        let (db, _dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(Request::ListSessions, db).await;
        match response {
            Response::SessionList { sessions } => assert!(sessions.is_empty()),
            other => panic!("expected SessionList, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handler_list_sessions_returns_all() {
        let (db, _dir) = create_test_db();
        let s1 = create_test_session("sess-1", "%0");
        let s2 = create_test_session("sess-2", "%1");
        db.create_session(&s1).unwrap();
        db.create_session(&s2).unwrap();

        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(Request::ListSessions, db).await;
        match response {
            Response::SessionList { sessions } => {
                assert_eq!(sessions.len(), 2);
                let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
                assert!(ids.contains(&"sess-1"));
                assert!(ids.contains(&"sess-2"));
            }
            other => panic!("expected SessionList, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handler_get_session_found() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(
            Request::GetSession {
                id: "sess-1".to_string(),
            },
            db,
        )
        .await;
        match response {
            Response::Session { session: Some(s) } => assert_eq!(s.id, "sess-1"),
            other => panic!("expected Session(Some), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handler_get_session_not_found() {
        let (db, _dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(
            Request::GetSession {
                id: "nonexistent".to_string(),
            },
            db,
        )
        .await;
        match response {
            Response::Session { session: None } => {}
            other => panic!("expected Session(None), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handler_get_events_returns_events() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();
        db.log_event("sess-1", &EventType::SessionDiscovered, None)
            .unwrap();
        db.log_event(
            "sess-1",
            &EventType::StateChanged {
                from: SessionState::Idle,
                to: SessionState::Working,
            },
            None,
        )
        .unwrap();

        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(
            Request::GetEvents {
                session_id: "sess-1".to_string(),
                limit: 10,
            },
            db,
        )
        .await;
        match response {
            Response::Events { events } => assert_eq!(events.len(), 2),
            other => panic!("expected Events, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handler_get_recent_events() {
        let (db, _dir) = create_test_db();
        let s1 = create_test_session("sess-1", "%0");
        let s2 = create_test_session("sess-2", "%1");
        db.create_session(&s1).unwrap();
        db.create_session(&s2).unwrap();
        db.log_event("sess-1", &EventType::SessionDiscovered, None)
            .unwrap();
        db.log_event("sess-2", &EventType::SessionDiscovered, None)
            .unwrap();

        let db = Arc::new(Mutex::new(db));
        let response = dispatch_request(Request::GetRecentEvents { limit: 10 }, db).await;
        match response {
            Response::Events { events } => assert_eq!(events.len(), 2),
            other => panic!("expected Events, got {other:?}"),
        }
    }

    // -- Socket integration tests --

    #[tokio::test]
    async fn test_ipc_ping() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone).await.unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let ping = serde_json::to_string(&Request::Ping).unwrap();
        write_half.write_all(ping.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let resp: Response = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(resp, Response::Pong);

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_ipc_list_sessions() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();

        let s1 = create_test_session("sess-1", "%0");
        let s2 = create_test_session("sess-2", "%1");
        db.create_session(&s1).unwrap();
        db.create_session(&s2).unwrap();

        let db = Arc::new(Mutex::new(db));
        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone).await.unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let req = serde_json::to_string(&Request::ListSessions).unwrap();
        write_half.write_all(req.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let resp: Response = serde_json::from_str(response.trim()).unwrap();

        match resp {
            Response::SessionList { sessions } => {
                assert_eq!(sessions.len(), 2);
                let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
                assert!(ids.contains(&"sess-1"));
                assert!(ids.contains(&"sess-2"));
            }
            other => panic!("expected SessionList, got {other:?}"),
        }

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_ipc_hook_event() {
        use ca_lib::hooks::HookEvent;

        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();

        let mut session = create_test_session("sess-1", "%0");
        session.working_dir = "/project".to_string();
        db.create_session(&session).unwrap();

        let db = Arc::new(Mutex::new(db));
        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone).await.unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let req = serde_json::to_string(&Request::HookEvent {
            event: HookEvent {
                hook_type: "PostToolUse".to_string(),
                session_id: None,
                working_dir: "/project".to_string(),
                timestamp: 1706600000,
                payload: None,
            },
        })
        .unwrap();
        write_half.write_all(req.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let resp: Response = serde_json::from_str(response.trim()).unwrap();

        match resp {
            Response::HookAck { session_id } => {
                assert_eq!(session_id, Some("sess-1".to_string()));
            }
            other => panic!("expected HookAck, got {other:?}"),
        }

        // Verify the session state was updated in the database
        let db_guard = db.lock().unwrap();
        let updated = db_guard.get_session("sess-1").unwrap().unwrap();
        assert_eq!(updated.state, SessionState::Working);

        drop(db_guard);
        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }
}
