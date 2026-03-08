use ca_lib::db::Database;
use ca_lib::hooks::apply_hook_event;
use ca_lib::ipc::{Request, Response};
use ca_lib::models::Session;
use ca_lib::plan::{PlanStatus, StepStatus};
use ca_lib::project::ProjectStatus;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

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

    /// Split into reader and writer for concurrent use in subscriber mode.
    fn into_parts(
        self,
    ) -> (
        BufReader<tokio::io::ReadHalf<UnixStream>>,
        tokio::io::WriteHalf<UnixStream>,
    ) {
        (self.reader, self.writer)
    }
}

pub async fn handle_connection(
    mut conn: Connection,
    db: Arc<Mutex<Database>>,
    update_tx: broadcast::Sender<Vec<Session>>,
) -> Result<(), SocketError> {
    while let Some(request) = conn.recv().await? {
        tracing::debug!(?request, "Received IPC request");

        if matches!(request, Request::Subscribe) {
            conn.send(&Response::Subscribed).await?;
            // Send current sessions so subscriber doesn't start empty
            let sessions = run_db(Arc::clone(&db), |db| db.list_sessions())
                .await
                .unwrap_or_else(|_| Vec::new());
            conn.send(&Response::SessionUpdate { sessions }).await?;
            return handle_subscriber(conn, update_tx.subscribe()).await;
        }

        let is_hook = matches!(request, Request::HookEvent { .. });
        let response = dispatch_request(request, Arc::clone(&db)).await;
        conn.send(&response).await?;

        // Hook events change session state -- notify subscribers
        if is_hook && matches!(response, Response::HookAck { .. }) {
            crate::polling::broadcast_sessions(&db, &update_tx).await;
        }
    }
    Ok(())
}

async fn handle_subscriber(
    conn: Connection,
    mut update_rx: broadcast::Receiver<Vec<Session>>,
) -> Result<(), SocketError> {
    let (mut reader, mut writer) = conn.into_parts();
    let mut disconnect_buf = [0u8; 1];

    loop {
        tokio::select! {
            result = update_rx.recv() => {
                match result {
                    Ok(sessions) => {
                        let response = Response::SessionUpdate { sessions };
                        let json = serde_json::to_string(&response)?;
                        if let Err(e) = async {
                            writer.write_all(json.as_bytes()).await?;
                            writer.write_all(b"\n").await?;
                            writer.flush().await
                        }.await {
                            tracing::debug!(error = %e, "Subscriber disconnected");
                            return Ok(());
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(count = n, "Subscriber lagged, skipping messages");
                    }
                }
            }
            // Detect client disconnect by watching for EOF on the read side
            read_result = reader.read(&mut disconnect_buf) => {
                match read_result {
                    Ok(0) | Err(_) => {
                        tracing::debug!("Subscriber connection closed");
                        return Ok(());
                    }
                    Ok(_) => continue,
                }
            }
        }
    }
}

async fn dispatch_request(request: Request, db: Arc<Mutex<Database>>) -> Response {
    match request {
        Request::Ping => Response::Pong,

        Request::ListSessions => run_db(db, |db| db.list_sessions())
            .await
            .map(|sessions| Response::SessionList { sessions })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::GetSession { id } => run_db(db, move |db| db.get_session(&id))
            .await
            .map(|session| Response::Session { session })
            .unwrap_or_else(|e| Response::Error { message: e }),

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

        Request::GetRecentEvents { limit } => run_db(db, move |db| db.get_recent_events(limit))
            .await
            .map(|events| Response::Events { events })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::HookEvent { event } => run_db(db, move |db| {
            apply_hook_event(db, &event).map_err(|e| match e {
                ca_lib::hooks::HookError::Db(db_err) => db_err,
            })
        })
        .await
        .map(|session_id| Response::HookAck { session_id })
        .unwrap_or_else(|e| Response::Error { message: e }),

        // Handled before dispatch in handle_connection; included for exhaustiveness
        Request::Subscribe => Response::Subscribed,

        Request::ListWorkspaces => run_db(db, |db| db.list_workspaces())
            .await
            .map(|workspaces| Response::WorkspaceList { workspaces })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::CreateWorkspace { path, name } => {
            run_db(db, move |db| db.create_workspace(&path, name.as_deref()))
                .await
                .map(|workspace| Response::WorkspaceCreated { workspace })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::DeleteWorkspace { id } => run_db(db, move |db| db.delete_workspace(id))
            .await
            .map(|_| Response::Ok)
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::ListProjects { workspace_id } => run_db(db, move |db| match workspace_id {
            Some(ws_id) => db.list_projects_by_workspace(ws_id),
            None => db.list_projects(),
        })
        .await
        .map(|projects| Response::ProjectList { projects })
        .unwrap_or_else(|e| Response::Error { message: e }),

        Request::CreateProject {
            workspace_id,
            name,
            description,
        } => run_db(db, move |db| {
            db.create_project(workspace_id, &name, description.as_deref())
        })
        .await
        .map(|project| Response::ProjectCreated { project })
        .unwrap_or_else(|e| Response::Error { message: e }),

        Request::UpdateProjectStatus { id, status } => {
            let parsed = match status.parse::<ProjectStatus>() {
                Ok(s) => s,
                Err(_) => {
                    return Response::Error {
                        message: format!("invalid project status: {status}"),
                    };
                }
            };
            run_db(db, move |db| db.update_project_status(id, parsed))
                .await
                .map(|_| Response::Ok)
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::DeleteProject { id } => run_db(db, move |db| db.delete_project(id))
            .await
            .map(|_| Response::Ok)
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::GetPlan { id } => run_db(db, move |db| db.get_plan(id))
            .await
            .map(|plan| Response::PlanDetail { plan })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::ListPlans { project_id } => {
            run_db(db, move |db| db.list_plans_by_project(project_id))
                .await
                .map(|plans| Response::PlanList { plans })
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::CreatePlan {
            project_id,
            name,
            content,
        } => run_db(db, move |db| db.create_plan(project_id, &name, &content))
            .await
            .map(|plan| Response::PlanCreated { plan })
            .unwrap_or_else(|e| Response::Error { message: e }),

        Request::UpdatePlanStatus { id, status } => {
            let parsed = match status.parse::<PlanStatus>() {
                Ok(s) => s,
                Err(_) => {
                    return Response::Error {
                        message: format!("invalid plan status: {status}"),
                    };
                }
            };
            run_db(db, move |db| db.update_plan_status(id, parsed))
                .await
                .map(|_| Response::Ok)
                .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::UpdateStepStatus {
            plan_id,
            step_id,
            status,
        } => {
            let parsed = match status.parse::<StepStatus>() {
                Ok(s) => s,
                Err(_) => {
                    return Response::Error {
                        message: format!("invalid step status: {status}"),
                    };
                }
            };
            run_db(db, move |db| {
                db.update_step_status(plan_id, &step_id, parsed)
            })
            .await
            .map(|_| Response::Ok)
            .unwrap_or_else(|e| Response::Error { message: e })
        }

        Request::DeletePlan { id } => run_db(db, move |db| db.delete_plan(id))
            .await
            .map(|_| Response::Ok)
            .unwrap_or_else(|e| Response::Error { message: e }),
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
            project_id: None,
            plan_step_id: None,
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

    fn make_update_tx() -> broadcast::Sender<Vec<Session>> {
        let (tx, _) = broadcast::channel(16);
        tx
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
        let update_tx = make_update_tx();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx).await.unwrap();
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
        let update_tx = make_update_tx();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx).await.unwrap();
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
        let update_tx = make_update_tx();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx).await.unwrap();
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

        // Verify the session state was updated in the database.
        // Scope the lock guard so it is definitely dropped before awaiting.
        {
            let db_guard = db.lock().unwrap();
            let updated = db_guard.get_session("sess-1").unwrap().unwrap();
            assert_eq!(updated.state, SessionState::Working);
        }

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    // -- Subscription tests --

    #[tokio::test]
    async fn test_subscribe_returns_subscribed() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);
        let update_tx = make_update_tx();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx).await.unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let req = serde_json::to_string(&Request::Subscribe).unwrap();
        write_half.write_all(req.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let resp: Response = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(resp, Response::Subscribed);

        // Consume the initial session snapshot push
        let mut initial = String::new();
        reader.read_line(&mut initial).await.unwrap();
        let initial_resp: Response = serde_json::from_str(initial.trim()).unwrap();
        assert!(matches!(initial_resp, Response::SessionUpdate { .. }));

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_subscriber_receives_broadcast() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);
        let (update_tx, _) = broadcast::channel::<Vec<Session>>(16);
        let update_tx_clone = update_tx.clone();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx_clone)
                .await
                .unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        // Subscribe
        let req = serde_json::to_string(&Request::Subscribe).unwrap();
        write_half.write_all(req.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Response = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp, Response::Subscribed);

        // Consume initial session snapshot (empty DB)
        let mut initial = String::new();
        reader.read_line(&mut initial).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(initial.trim()).unwrap(),
            Response::SessionUpdate { .. }
        ));

        // Broadcast a session update
        let sessions = vec![create_test_session("sess-1", "%0")];
        update_tx.send(sessions.clone()).unwrap();

        // Read the broadcast push
        let mut push_line = String::new();
        reader.read_line(&mut push_line).await.unwrap();
        let push: Response = serde_json::from_str(push_line.trim()).unwrap();
        match push {
            Response::SessionUpdate { sessions: received } => {
                assert_eq!(received.len(), 1);
                assert_eq!(received[0].id, "sess-1");
            }
            other => panic!("expected SessionUpdate, got {other:?}"),
        }

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_subscriber_cleanup_on_disconnect() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let db_clone = Arc::clone(&db);
        let (update_tx, _) = broadcast::channel::<Vec<Session>>(16);
        let update_tx_clone = update_tx.clone();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn, db_clone, update_tx_clone)
                .await
                .unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        // Subscribe
        let req = serde_json::to_string(&Request::Subscribe).unwrap();
        write_half.write_all(req.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Response>(line.trim()).unwrap(),
            Response::Subscribed
        );

        // Consume initial session snapshot
        let mut initial = String::new();
        reader.read_line(&mut initial).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(initial.trim()).unwrap(),
            Response::SessionUpdate { .. }
        ));

        // Drop client connection
        drop(write_half);
        drop(reader);

        // Server task should complete without panic
        tokio::time::timeout(std::time::Duration::from_secs(2), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_broadcast_no_subscribers_is_noop() {
        let (tx, _) = broadcast::channel::<Vec<Session>>(16);
        let sessions = vec![create_test_session("sess-1", "%0")];

        // send() returns Err when there are no receivers, but that's expected
        let result = tx.send(sessions);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_subscribers_receive_broadcast() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let (db, _db_dir) = create_test_db();
        let db = Arc::new(Mutex::new(db));

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        let (update_tx, _) = broadcast::channel::<Vec<Session>>(16);
        let sub_req = serde_json::to_string(&Request::Subscribe).unwrap();

        // Spawn an accept loop that handles both connections
        let db1 = Arc::clone(&db);
        let tx1 = update_tx.clone();
        let db2 = Arc::clone(&db);
        let tx2 = update_tx.clone();

        // Spawn accept+handle for connection 1
        let accept_task = tokio::spawn({
            async move {
                let conn1 = server.accept().await.unwrap();
                let task1 =
                    tokio::spawn(async move { handle_connection(conn1, db1, tx1).await.unwrap() });
                let conn2 = server.accept().await.unwrap();
                let task2 =
                    tokio::spawn(async move { handle_connection(conn2, db2, tx2).await.unwrap() });
                (task1, task2)
            }
        });

        // Connect first subscriber
        let stream1 = UnixStream::connect(&socket_path).await.unwrap();
        let (r1, mut w1) = tokio::io::split(stream1);
        let mut reader1 = BufReader::new(r1);

        w1.write_all(sub_req.as_bytes()).await.unwrap();
        w1.write_all(b"\n").await.unwrap();
        w1.flush().await.unwrap();

        let mut line1 = String::new();
        reader1.read_line(&mut line1).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Response>(line1.trim()).unwrap(),
            Response::Subscribed
        );

        // Consume initial snapshot for sub1
        let mut init1 = String::new();
        reader1.read_line(&mut init1).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(init1.trim()).unwrap(),
            Response::SessionUpdate { .. }
        ));

        // Connect second subscriber
        let stream2 = UnixStream::connect(&socket_path).await.unwrap();
        let (r2, mut w2) = tokio::io::split(stream2);
        let mut reader2 = BufReader::new(r2);

        w2.write_all(sub_req.as_bytes()).await.unwrap();
        w2.write_all(b"\n").await.unwrap();
        w2.flush().await.unwrap();

        let mut line2 = String::new();
        reader2.read_line(&mut line2).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Response>(line2.trim()).unwrap(),
            Response::Subscribed
        );

        // Consume initial snapshot for sub2
        let mut init2 = String::new();
        reader2.read_line(&mut init2).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(init2.trim()).unwrap(),
            Response::SessionUpdate { .. }
        ));

        // Wait for accept_task to finish spawning both handlers
        let (server_task1, server_task2) =
            tokio::time::timeout(std::time::Duration::from_secs(2), accept_task)
                .await
                .expect("accept task timed out")
                .expect("accept task panicked");

        // Broadcast
        let sessions = vec![create_test_session("sess-x", "%5")];
        update_tx.send(sessions).unwrap();

        // Both should receive the broadcast update
        let mut push1 = String::new();
        reader1.read_line(&mut push1).await.unwrap();
        let resp1: Response = serde_json::from_str(push1.trim()).unwrap();
        match resp1 {
            Response::SessionUpdate { sessions } => {
                assert_eq!(sessions[0].id, "sess-x");
            }
            other => panic!("sub1: expected SessionUpdate, got {other:?}"),
        }

        let mut push2 = String::new();
        reader2.read_line(&mut push2).await.unwrap();
        let resp2: Response = serde_json::from_str(push2.trim()).unwrap();
        match resp2 {
            Response::SessionUpdate { sessions } => {
                assert_eq!(sessions[0].id, "sess-x");
            }
            other => panic!("sub2: expected SessionUpdate, got {other:?}"),
        }

        // Cleanup
        drop(w1);
        drop(reader1);
        drop(w2);
        drop(reader2);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task1)
            .await
            .expect("server task 1 timed out")
            .expect("server task 1 panicked");
        tokio::time::timeout(std::time::Duration::from_secs(1), server_task2)
            .await
            .expect("server task 2 timed out")
            .expect("server task 2 panicked");
    }
}
