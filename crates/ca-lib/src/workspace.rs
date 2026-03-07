use crate::db::{Database, DbError};
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

impl Database {
    pub fn create_workspace(
        &self,
        path: &str,
        name: Option<&str>,
    ) -> Result<Workspace, DbError> {
        let resolved_name = match name {
            Some(n) => n.to_string(),
            None => Path::new(path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string()),
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            INSERT INTO workspaces (name, path, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![resolved_name, path, now, now],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(Workspace {
            id,
            name: resolved_name,
            path: path.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_workspace(&self, id: i64) -> Result<Option<Workspace>, DbError> {
        let workspace = self
            .connection()
            .query_row(
                r#"
                SELECT id, name, path, created_at, updated_at
                FROM workspaces WHERE id = ?1
                "#,
                params![id],
                |row| row_to_workspace(row),
            )
            .optional()?;

        Ok(workspace)
    }

    pub fn get_workspace_by_path(&self, path: &str) -> Result<Option<Workspace>, DbError> {
        let workspace = self
            .connection()
            .query_row(
                r#"
                SELECT id, name, path, created_at, updated_at
                FROM workspaces WHERE path = ?1
                "#,
                params![path],
                |row| row_to_workspace(row),
            )
            .optional()?;

        Ok(workspace)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, name, path, created_at, updated_at
            FROM workspaces
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map([], |row| row_to_workspace(row))?;

        let mut workspaces = Vec::new();
        for row_result in rows {
            workspaces.push(row_result?);
        }
        Ok(workspaces)
    }

    pub fn delete_workspace(&self, id: i64) -> Result<bool, DbError> {
        let rows_affected = self
            .connection()
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use tempfile::tempdir;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_create_workspace() {
        let (db, _dir) = create_test_db();

        let ws = db
            .create_workspace("/home/user/myproject", Some("myproject"))
            .unwrap();

        assert!(ws.id > 0);
        assert_eq!(ws.name, "myproject");
        assert_eq!(ws.path, "/home/user/myproject");
        assert!(ws.created_at > 0);
        assert!(ws.updated_at > 0);
        assert_eq!(ws.created_at, ws.updated_at);
    }

    #[test]
    fn test_create_workspace_name_auto_derived() {
        let (db, _dir) = create_test_db();

        let ws = db
            .create_workspace("/home/user/cool-project", None)
            .unwrap();

        assert_eq!(ws.name, "cool-project");
        assert_eq!(ws.path, "/home/user/cool-project");
    }

    #[test]
    fn test_create_workspace_explicit_name() {
        let (db, _dir) = create_test_db();

        let ws = db
            .create_workspace("/home/user/my-repo", Some("My Custom Name"))
            .unwrap();

        assert_eq!(ws.name, "My Custom Name");
        assert_eq!(ws.path, "/home/user/my-repo");
    }

    #[test]
    fn test_get_workspace() {
        let (db, _dir) = create_test_db();
        let created = db
            .create_workspace("/home/user/project", Some("project"))
            .unwrap();

        let fetched = db.get_workspace(created.id).unwrap().unwrap();

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, "project");
        assert_eq!(fetched.path, "/home/user/project");
        assert_eq!(fetched.created_at, created.created_at);
        assert_eq!(fetched.updated_at, created.updated_at);
    }

    #[test]
    fn test_get_workspace_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_workspace(9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_workspace_by_path() {
        let (db, _dir) = create_test_db();
        let created = db
            .create_workspace("/home/user/project", Some("project"))
            .unwrap();

        let fetched = db
            .get_workspace_by_path("/home/user/project")
            .unwrap()
            .unwrap();

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, "project");
        assert_eq!(fetched.path, "/home/user/project");
    }

    #[test]
    fn test_get_workspace_by_path_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_workspace_by_path("/no/such/path").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_workspaces() {
        let (db, _dir) = create_test_db();
        let ws1 = db
            .create_workspace("/home/user/alpha", Some("alpha"))
            .unwrap();
        let ws2 = db
            .create_workspace("/home/user/beta", Some("beta"))
            .unwrap();

        let all = db.list_workspaces().unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, ws2.id);
        assert_eq!(all[1].id, ws1.id);
        assert_eq!(all[0].name, "beta");
        assert_eq!(all[1].name, "alpha");
    }

    #[test]
    fn test_list_workspaces_empty() {
        let (db, _dir) = create_test_db();

        let all = db.list_workspaces().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_delete_workspace() {
        let (db, _dir) = create_test_db();
        let ws = db
            .create_workspace("/home/user/project", Some("project"))
            .unwrap();

        let deleted = db.delete_workspace(ws.id).unwrap();
        assert!(deleted);

        let result = db.get_workspace(ws.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_workspace_not_found() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_workspace(9999).unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_duplicate_path_error() {
        let (db, _dir) = create_test_db();

        db.create_workspace("/home/user/project", Some("first"))
            .unwrap();
        let result = db.create_workspace("/home/user/project", Some("second"));

        assert!(result.is_err());
    }

    #[test]
    fn test_timestamps_set_correctly() {
        let (db, _dir) = create_test_db();

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let ws = db
            .create_workspace("/home/user/project", None)
            .unwrap();

        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert!(ws.created_at >= before);
        assert!(ws.created_at <= after);
        assert_eq!(ws.created_at, ws.updated_at);
    }
}
