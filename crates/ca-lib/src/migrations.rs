use crate::db::DbError;
use rusqlite::Connection;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn get_schema_version(conn: &Connection) -> Result<i64, rusqlite::Error> {
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        return Ok(0);
    }

    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )
}

pub fn run_migrations(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL,
            applied_at INTEGER NOT NULL
        )",
    )?;

    let current_version = get_schema_version(conn)?;

    let migrations: &[(i64, fn(&Connection) -> Result<(), rusqlite::Error>)] =
        &[(1, migrate_001_session_project_link)];

    for &(version, migrate_fn) in migrations {
        if version > current_version {
            migrate_fn(conn)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![version, now],
            )?;
            tracing::info!(version, "Applied migration");
        }
    }

    Ok(())
}

fn migrate_001_session_project_link(conn: &Connection) -> Result<(), rusqlite::Error> {
    let has_project_id = conn
        .prepare("SELECT project_id FROM sessions LIMIT 0")
        .is_ok();
    if !has_project_id {
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN project_id INTEGER")?;
    }

    let has_plan_step_id = conn
        .prepare("SELECT plan_step_id FROM sessions LIMIT 0")
        .is_ok();
    if !has_plan_step_id {
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN plan_step_id TEXT")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_legacy_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
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
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_fresh_db_schema_version_zero() {
        let conn = Connection::open_in_memory().unwrap();
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, 0);
    }

    #[test]
    fn test_run_migrations_on_fresh_db() {
        let conn = create_legacy_db();
        assert_eq!(get_schema_version(&conn).unwrap(), 0);

        run_migrations(&conn).unwrap();

        assert_eq!(get_schema_version(&conn).unwrap(), 1);
    }

    #[test]
    fn test_run_migrations_idempotent() {
        let conn = create_legacy_db();

        run_migrations(&conn).unwrap();
        let version_after_first = get_schema_version(&conn).unwrap();

        run_migrations(&conn).unwrap();
        let version_after_second = get_schema_version(&conn).unwrap();

        assert_eq!(version_after_first, version_after_second);
    }

    #[test]
    fn test_migration_adds_columns() {
        let conn = create_legacy_db();

        assert!(conn
            .prepare("SELECT project_id FROM sessions LIMIT 0")
            .is_err());
        assert!(conn
            .prepare("SELECT plan_step_id FROM sessions LIMIT 0")
            .is_err());

        run_migrations(&conn).unwrap();

        conn.prepare("SELECT project_id FROM sessions LIMIT 0")
            .expect("project_id column should exist after migration");
        conn.prepare("SELECT plan_step_id FROM sessions LIMIT 0")
            .expect("plan_step_id column should exist after migration");
    }

    #[test]
    fn test_sessions_with_new_columns() {
        let conn = create_legacy_db();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (
                id, pane_id, session_name, window_index, pane_index,
                working_dir, state, detection_method, last_activity,
                created_at, updated_at, project_id, plan_step_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                "sess-1",
                "%0",
                "main",
                0,
                0,
                "/home/user",
                "idle",
                "process_name",
                1706500000,
                1706400000,
                1706500000,
                42,
                "step-1.2"
            ],
        )
        .unwrap();

        let (project_id, plan_step_id): (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT project_id, plan_step_id FROM sessions WHERE id = 'sess-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(project_id, Some(42));
        assert_eq!(plan_step_id, Some("step-1.2".to_string()));
    }

    #[test]
    fn test_already_migrated_skips() {
        let conn = create_legacy_db();
        run_migrations(&conn).unwrap();
        assert_eq!(get_schema_version(&conn).unwrap(), 1);

        // Manually insert a version record to simulate already-migrated
        let conn2 = create_legacy_db();
        run_migrations(&conn2).unwrap();

        // Version should still be 1, not 2
        assert_eq!(get_schema_version(&conn2).unwrap(), 1);
    }
}
