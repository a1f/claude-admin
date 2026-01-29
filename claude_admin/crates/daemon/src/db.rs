use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to create database directory: {0}")]
    CreateDir(#[from] std::io::Error),
}

#[allow(dead_code)]
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

        Ok(Database {
            conn,
            path: path.to_owned(),
        })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}
