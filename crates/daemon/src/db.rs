use crate::events::{Event, EventType};
use crate::models::{Session, SessionState};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to create database directory: {0}")]
    CreateDir(#[from] std::io::Error),
    #[error("invalid session state in database: {0}")]
    InvalidState(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;

        let version: String = conn.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;

        tracing::info!(
            path = %path.display(),
            sqlite_version = %version,
            "Database initialized"
        );

        let db = Database {
            conn,
            path: path.to_owned(),
        };

        db.init_schema()?;

        Ok(db)
    }

    fn init_schema(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                pane_id TEXT NOT NULL UNIQUE,
                session_name TEXT NOT NULL,
                window_index INTEGER NOT NULL,
                pane_index INTEGER NOT NULL,
                working_dir TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'idle',
                detection_method TEXT NOT NULL,
                last_activity INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT,
                timestamp INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_pane_id ON sessions(pane_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_state ON sessions(state);
            CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);
            CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
            "#,
        )?;

        tracing::debug!("Database schema initialized");
        Ok(())
    }

    #[allow(dead_code)]
    pub fn journal_mode(&self) -> Result<String, DbError> {
        let mode: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        Ok(mode)
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[allow(dead_code)]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn create_session(&self, session: &Session) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            INSERT INTO sessions (
                id, pane_id, session_name, window_index, pane_index,
                working_dir, state, detection_method, last_activity,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                session.id,
                session.pane_id,
                session.session_name,
                session.window_index,
                session.pane_index,
                session.working_dir,
                session.state.as_str(),
                session.detection_method,
                session.last_activity,
                session.created_at,
                session.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>, DbError> {
        let result = self
            .conn
            .query_row(
                r#"
                SELECT id, pane_id, session_name, window_index, pane_index,
                       working_dir, state, detection_method, last_activity,
                       created_at, updated_at
                FROM sessions WHERE id = ?1
                "#,
                params![id],
                |row| self.row_to_session(row),
            )
            .optional()?;

        match result {
            Some(Ok(session)) => Ok(Some(session)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn get_session_by_pane(&self, pane_id: &str) -> Result<Option<Session>, DbError> {
        let result = self
            .conn
            .query_row(
                r#"
                SELECT id, pane_id, session_name, window_index, pane_index,
                       working_dir, state, detection_method, last_activity,
                       created_at, updated_at
                FROM sessions WHERE pane_id = ?1
                "#,
                params![pane_id],
                |row| self.row_to_session(row),
            )
            .optional()?;

        match result {
            Some(Ok(session)) => Ok(Some(session)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn update_session(&self, session: &Session) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            UPDATE sessions SET
                pane_id = ?2,
                session_name = ?3,
                window_index = ?4,
                pane_index = ?5,
                working_dir = ?6,
                state = ?7,
                detection_method = ?8,
                last_activity = ?9,
                updated_at = ?10
            WHERE id = ?1
            "#,
            params![
                session.id,
                session.pane_id,
                session.session_name,
                session.window_index,
                session.pane_index,
                session.working_dir,
                session.state.as_str(),
                session.detection_method,
                session.last_activity,
                session.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_session_state(
        &self,
        id: &str,
        state: SessionState,
        timestamp: i64,
    ) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            UPDATE sessions SET
                state = ?2,
                last_activity = ?3,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![id, state.as_str(), timestamp],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, pane_id, session_name, window_index, pane_index,
                   working_dir, state, detection_method, last_activity,
                   created_at, updated_at
            FROM sessions
            ORDER BY created_at DESC
            "#,
        )?;

        let rows = stmt.query_map([], |row| self.row_to_session(row))?;

        let mut sessions = Vec::new();
        for row_result in rows {
            sessions.push(row_result??);
        }
        Ok(sessions)
    }

    pub fn delete_session(&self, id: &str) -> Result<bool, DbError> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }

    fn row_to_session(&self, row: &rusqlite::Row) -> rusqlite::Result<Result<Session, DbError>> {
        let state_str: String = row.get(6)?;
        let state = match state_str.parse::<SessionState>() {
            Ok(s) => s,
            Err(_) => return Ok(Err(DbError::InvalidState(state_str))),
        };

        Ok(Ok(Session {
            id: row.get(0)?,
            pane_id: row.get(1)?,
            session_name: row.get(2)?,
            window_index: row.get(3)?,
            pane_index: row.get(4)?,
            working_dir: row.get(5)?,
            state,
            detection_method: row.get(7)?,
            last_activity: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        }))
    }

    pub fn log_event(
        &self,
        session_id: &str,
        event_type: &EventType,
        payload: Option<&serde_json::Value>,
    ) -> Result<i64, DbError> {
        let event_type_json = serde_json::to_string(event_type)?;
        let payload_json = payload.map(|p| serde_json::to_string(p)).transpose()?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn.execute(
            r#"
            INSERT INTO events (session_id, event_type, payload, timestamp)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![session_id, event_type_json, payload_json, timestamp],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_events(&self, session_id: &str, limit: usize) -> Result<Vec<Event>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, session_id, event_type, payload, timestamp
            FROM events
            WHERE session_id = ?1
            ORDER BY timestamp DESC
            LIMIT ?2
            "#,
        )?;

        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            self.row_to_event(row)
        })?;

        let mut events = Vec::new();
        for row_result in rows {
            events.push(row_result??);
        }
        Ok(events)
    }

    pub fn get_recent_events(&self, limit: usize) -> Result<Vec<Event>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, session_id, event_type, payload, timestamp
            FROM events
            ORDER BY timestamp DESC
            LIMIT ?1
            "#,
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| self.row_to_event(row))?;

        let mut events = Vec::new();
        for row_result in rows {
            events.push(row_result??);
        }
        Ok(events)
    }

    fn row_to_event(&self, row: &rusqlite::Row) -> rusqlite::Result<Result<Event, DbError>> {
        let event_type_str: String = row.get(2)?;
        let payload_str: Option<String> = row.get(3)?;

        let event_type = match serde_json::from_str(&event_type_str) {
            Ok(et) => et,
            Err(e) => return Ok(Err(DbError::Serialization(e))),
        };

        let payload = match payload_str {
            Some(s) => match serde_json::from_str(&s) {
                Ok(p) => Some(p),
                Err(e) => return Ok(Err(DbError::Serialization(e))),
            },
            None => None,
        };

        Ok(Ok(Event {
            id: row.get(0)?,
            session_id: row.get(1)?,
            event_type,
            payload,
            timestamp: row.get(4)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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

    #[test]
    fn test_db_file_created() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let _db = Database::open(&db_path).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn test_db_wal_mode_enabled() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let db = Database::open(&db_path).unwrap();
        let mode = db.journal_mode().unwrap();

        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn test_db_connection_valid() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let db = Database::open(&db_path).unwrap();

        let result: i32 = db
            .connection()
            .query_row("SELECT 1 + 1", [], |row| row.get(0))
            .unwrap();

        assert_eq!(result, 2);
    }

    #[test]
    fn test_schema_created() {
        let (db, _dir) = create_test_db();

        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"events".to_string()));
    }

    #[test]
    fn test_indexes_created() {
        let (db, _dir) = create_test_db();

        let indexes: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(indexes.contains(&"idx_sessions_pane_id".to_string()));
        assert!(indexes.contains(&"idx_sessions_state".to_string()));
        assert!(indexes.contains(&"idx_events_session_id".to_string()));
        assert!(indexes.contains(&"idx_events_timestamp".to_string()));
    }

    #[test]
    fn test_create_and_get_session() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");

        db.create_session(&session).unwrap();

        let retrieved = db.get_session("sess-1").unwrap().unwrap();
        assert_eq!(retrieved.id, session.id);
        assert_eq!(retrieved.pane_id, session.pane_id);
        assert_eq!(retrieved.session_name, session.session_name);
        assert_eq!(retrieved.state, session.state);
    }

    #[test]
    fn test_get_session_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_session("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_session_by_pane() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%5");

        db.create_session(&session).unwrap();

        let retrieved = db.get_session_by_pane("%5").unwrap().unwrap();
        assert_eq!(retrieved.id, "sess-1");
    }

    #[test]
    fn test_get_session_by_pane_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_session_by_pane("%99").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_session() {
        let (db, _dir) = create_test_db();
        let mut session = create_test_session("sess-1", "%0");

        db.create_session(&session).unwrap();

        session.state = SessionState::Working;
        session.working_dir = "/tmp".to_string();
        session.updated_at = 1706600000;

        db.update_session(&session).unwrap();

        let retrieved = db.get_session("sess-1").unwrap().unwrap();
        assert_eq!(retrieved.state, SessionState::Working);
        assert_eq!(retrieved.working_dir, "/tmp");
        assert_eq!(retrieved.updated_at, 1706600000);
    }

    #[test]
    fn test_update_session_state() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");

        db.create_session(&session).unwrap();
        db.update_session_state("sess-1", SessionState::NeedsInput, 1706600000)
            .unwrap();

        let retrieved = db.get_session("sess-1").unwrap().unwrap();
        assert_eq!(retrieved.state, SessionState::NeedsInput);
        assert_eq!(retrieved.last_activity, 1706600000);
        assert_eq!(retrieved.updated_at, 1706600000);
    }

    #[test]
    fn test_list_sessions() {
        let (db, _dir) = create_test_db();

        let mut session1 = create_test_session("sess-1", "%0");
        session1.created_at = 1000;
        let mut session2 = create_test_session("sess-2", "%1");
        session2.created_at = 2000;

        db.create_session(&session1).unwrap();
        db.create_session(&session2).unwrap();

        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "sess-2");
        assert_eq!(sessions[1].id, "sess-1");
    }

    #[test]
    fn test_list_sessions_empty() {
        let (db, _dir) = create_test_db();

        let sessions = db.list_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_delete_session() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");

        db.create_session(&session).unwrap();

        let deleted = db.delete_session("sess-1").unwrap();
        assert!(deleted);

        let result = db.get_session("sess-1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_session_not_found() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_session("nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_pane_id_unique_constraint() {
        let (db, _dir) = create_test_db();

        let session1 = create_test_session("sess-1", "%0");
        let session2 = create_test_session("sess-2", "%0");

        db.create_session(&session1).unwrap();
        let result = db.create_session(&session2);

        assert!(result.is_err());
    }

    #[test]
    fn test_log_event_session_discovered() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let event_id = db
            .log_event("sess-1", &EventType::SessionDiscovered, None)
            .unwrap();

        assert!(event_id > 0);
    }

    #[test]
    fn test_log_event_state_changed() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let event_type = EventType::StateChanged {
            from: SessionState::Idle,
            to: SessionState::Working,
        };

        let event_id = db.log_event("sess-1", &event_type, None).unwrap();
        assert!(event_id > 0);
    }

    #[test]
    fn test_log_event_with_payload() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let payload = serde_json::json!({"key": "value", "count": 42});
        let event_id = db
            .log_event("sess-1", &EventType::SessionDiscovered, Some(&payload))
            .unwrap();

        assert!(event_id > 0);
    }

    #[test]
    fn test_get_events() {
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

        let events = db.get_events("sess-1", 10).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_get_events_limit() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        for _ in 0..5 {
            db.log_event("sess-1", &EventType::SessionDiscovered, None)
                .unwrap();
        }

        let events = db.get_events("sess-1", 3).unwrap();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_get_events_empty() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let events = db.get_events("sess-1", 10).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_get_recent_events() {
        let (db, _dir) = create_test_db();

        let session1 = create_test_session("sess-1", "%0");
        let session2 = create_test_session("sess-2", "%1");
        db.create_session(&session1).unwrap();
        db.create_session(&session2).unwrap();

        db.log_event("sess-1", &EventType::SessionDiscovered, None)
            .unwrap();
        db.log_event("sess-2", &EventType::SessionDiscovered, None)
            .unwrap();

        let events = db.get_recent_events(10).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_get_recent_events_limit() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        for _ in 0..10 {
            db.log_event("sess-1", &EventType::SessionDiscovered, None)
                .unwrap();
        }

        let events = db.get_recent_events(5).unwrap();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn test_event_roundtrip_with_payload() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let payload = serde_json::json!({"hook": "PostToolUse", "tool": "Edit"});
        let event_type = EventType::HookReceived {
            hook_type: "PostToolUse".to_string(),
        };

        db.log_event("sess-1", &event_type, Some(&payload)).unwrap();

        let events = db.get_events("sess-1", 1).unwrap();
        assert_eq!(events.len(), 1);

        let event = &events[0];
        assert_eq!(event.session_id, "sess-1");
        assert_eq!(event.event_type, event_type);
        assert!(event.payload.is_some());

        let retrieved_payload = event.payload.as_ref().unwrap();
        assert_eq!(retrieved_payload["hook"], "PostToolUse");
        assert_eq!(retrieved_payload["tool"], "Edit");
    }
}
