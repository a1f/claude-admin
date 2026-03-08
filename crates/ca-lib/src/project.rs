use crate::db::{Database, DbError};
use crate::git;
use rusqlite::OptionalExtension;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Running,
    Completed,
    Archived,
}

impl ProjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectStatus::Active => "active",
            ProjectStatus::Running => "running",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Archived => "archived",
        }
    }
}

impl fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectStatus {
    type Err = ParseProjectStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(ProjectStatus::Active),
            "running" => Ok(ProjectStatus::Running),
            "completed" => Ok(ProjectStatus::Completed),
            "archived" => Ok(ProjectStatus::Archived),
            _ => Err(ParseProjectStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProjectStatusError(pub String);

impl fmt::Display for ParseProjectStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown project status: {}", self.0)
    }
}

impl std::error::Error for ParseProjectStatusError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub workspace_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<Result<Project, DbError>> {
    let status_str: String = row.get(4)?;
    let status = match status_str.parse::<ProjectStatus>() {
        Ok(s) => s,
        Err(_) => return Ok(Err(DbError::InvalidState(status_str))),
    };

    Ok(Ok(Project {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        status,
        worktree_path: row.get(5)?,
        branch_name: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    }))
}

impl Database {
    pub fn create_project(
        &self,
        workspace_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<Project, DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            INSERT INTO projects (workspace_id, name, description, status,
                                  worktree_path, branch_name, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                workspace_id,
                name,
                description,
                ProjectStatus::Active.as_str(),
                None::<String>,
                None::<String>,
                now,
                now,
            ],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(Project {
            id,
            workspace_id,
            name: name.to_string(),
            description: description.map(String::from),
            status: ProjectStatus::Active,
            worktree_path: None,
            branch_name: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_project(&self, id: i64) -> Result<Option<Project>, DbError> {
        let result = self
            .connection()
            .query_row(
                r#"
                SELECT id, workspace_id, name, description, status,
                       worktree_path, branch_name, created_at, updated_at
                FROM projects WHERE id = ?1
                "#,
                params![id],
                row_to_project,
            )
            .optional()?;

        match result {
            Some(Ok(project)) => Ok(Some(project)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn list_projects_by_workspace(&self, workspace_id: i64) -> Result<Vec<Project>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, workspace_id, name, description, status,
                   worktree_path, branch_name, created_at, updated_at
            FROM projects
            WHERE workspace_id = ?1
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map(params![workspace_id], row_to_project)?;

        let mut projects = Vec::new();
        for row_result in rows {
            projects.push(row_result??);
        }
        Ok(projects)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, workspace_id, name, description, status,
                   worktree_path, branch_name, created_at, updated_at
            FROM projects
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map([], row_to_project)?;

        let mut projects = Vec::new();
        for row_result in rows {
            projects.push(row_result??);
        }
        Ok(projects)
    }

    pub fn update_project_status(&self, id: i64, status: ProjectStatus) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            UPDATE projects SET
                status = ?2,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![id, status.as_str(), now],
        )?;
        Ok(())
    }

    pub fn update_project_worktree(
        &self,
        id: i64,
        worktree_path: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            UPDATE projects SET
                worktree_path = ?2,
                branch_name = ?3,
                updated_at = ?4
            WHERE id = ?1
            "#,
            params![id, worktree_path, branch_name, now],
        )?;
        Ok(())
    }

    /// Creates a project and, if the workspace is a git repo,
    /// automatically sets up a git worktree for it.
    /// Degrades gracefully: project is still created if git ops fail.
    pub fn create_project_with_worktree(
        &self,
        workspace_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<Project, DbError> {
        let workspace = self.get_workspace(workspace_id)?.ok_or_else(|| {
            DbError::InvalidState(format!("workspace {} not found", workspace_id))
        })?;

        let workspace_path = Path::new(&workspace.path);

        if !git::is_git_repo(workspace_path) {
            return self.create_project(workspace_id, name, description);
        }

        let branch = git::sanitize_branch_name(name);
        let wt_path_str = git::worktree_path_for_project(&workspace.path, name);
        let wt_path = Path::new(&wt_path_str);

        match git::create_worktree(workspace_path, &branch, wt_path) {
            Ok(()) => {
                let project = self.create_project(workspace_id, name, description)?;
                self.update_project_worktree(project.id, Some(&wt_path_str), Some(&branch))?;
                // Re-fetch to return accurate state
                self.get_project(project.id)?
                    .ok_or_else(|| DbError::InvalidState("project vanished after create".into()))
            }
            Err(e) => {
                tracing::warn!(
                    workspace_id,
                    project_name = name,
                    error = %e,
                    "Failed to create git worktree, creating project without one"
                );
                self.create_project(workspace_id, name, description)
            }
        }
    }

    pub fn delete_project(&self, id: i64) -> Result<bool, DbError> {
        self.try_remove_project_worktree(id);

        let rows_affected = self
            .connection()
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }

    pub fn archive_project(&self, id: i64) -> Result<(), DbError> {
        self.try_remove_project_worktree(id);
        self.update_project_status(id, ProjectStatus::Archived)?;
        self.update_project_worktree(id, None, None)?;
        Ok(())
    }

    /// Best-effort worktree removal. Logs a warning on failure
    /// but never propagates the error -- callers should not fail
    /// just because a worktree could not be cleaned up.
    fn try_remove_project_worktree(&self, project_id: i64) {
        let project = match self.get_project(project_id) {
            Ok(Some(p)) => p,
            _ => return,
        };

        let wt_path_str = match &project.worktree_path {
            Some(p) => p.clone(),
            None => return,
        };

        let workspace = match self.get_workspace(project.workspace_id) {
            Ok(Some(ws)) => ws,
            _ => return,
        };

        let repo_path = Path::new(&workspace.path);
        let wt_path = Path::new(&wt_path_str);

        if let Err(e) = git::remove_worktree(repo_path, wt_path) {
            tracing::warn!(
                project_id,
                worktree_path = %wt_path_str,
                error = %e,
                "Failed to remove git worktree"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::project::ProjectStatus;
    use tempfile::tempdir;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    fn create_test_workspace(db: &Database) -> i64 {
        let ws = db
            .create_workspace("/home/user/myproject", Some("myproject"))
            .unwrap();
        ws.id
    }

    #[test]
    fn test_create_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db
            .create_project(ws_id, "My Project", Some("A test project"))
            .unwrap();

        assert!(project.id > 0);
        assert_eq!(project.workspace_id, ws_id);
        assert_eq!(project.name, "My Project");
        assert_eq!(project.description, Some("A test project".to_string()));
        assert_eq!(project.status, ProjectStatus::Active);
        assert!(project.worktree_path.is_none());
        assert!(project.branch_name.is_none());
        assert!(project.created_at > 0);
        assert!(project.updated_at > 0);
        assert_eq!(project.created_at, project.updated_at);
    }

    #[test]
    fn test_get_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let created = db.create_project(ws_id, "Fetch Me", None).unwrap();

        let fetched = db.get_project(created.id).unwrap().unwrap();

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.workspace_id, ws_id);
        assert_eq!(fetched.name, "Fetch Me");
        assert_eq!(fetched.description, None);
        assert_eq!(fetched.status, ProjectStatus::Active);
        assert_eq!(fetched.created_at, created.created_at);
        assert_eq!(fetched.updated_at, created.updated_at);
    }

    #[test]
    fn test_get_project_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_project(9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_projects_by_workspace() {
        let (db, _dir) = create_test_db();
        let ws1 = create_test_workspace(&db);
        let ws2 = db
            .create_workspace("/home/user/other", Some("other"))
            .unwrap()
            .id;

        let p1 = db.create_project(ws1, "Alpha", None).unwrap();
        let _p2 = db.create_project(ws2, "Beta", None).unwrap();
        let p3 = db.create_project(ws1, "Gamma", None).unwrap();

        let projects = db.list_projects_by_workspace(ws1).unwrap();

        assert_eq!(projects.len(), 2);
        let ids: Vec<i64> = projects.iter().map(|p| p.id).collect();
        assert!(ids.contains(&p1.id));
        assert!(ids.contains(&p3.id));
    }

    #[test]
    fn test_list_projects() {
        let (db, _dir) = create_test_db();
        let ws1 = create_test_workspace(&db);
        let ws2 = db
            .create_workspace("/home/user/other", Some("other"))
            .unwrap()
            .id;

        db.create_project(ws1, "Alpha", None).unwrap();
        db.create_project(ws2, "Beta", None).unwrap();
        db.create_project(ws1, "Gamma", None).unwrap();

        let all = db.list_projects().unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_list_projects_empty() {
        let (db, _dir) = create_test_db();

        let all = db.list_projects().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_update_project_status() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "Statusful", None).unwrap();
        let original_updated_at = project.updated_at;

        db.update_project_status(project.id, ProjectStatus::Running)
            .unwrap();

        let fetched = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProjectStatus::Running);
        assert!(fetched.updated_at >= original_updated_at);
    }

    #[test]
    fn test_delete_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "Doomed", None).unwrap();

        let deleted = db.delete_project(project.id).unwrap();
        assert!(deleted);

        let result = db.get_project(project.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_project_not_found() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_project(9999).unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_cascade_delete_workspace() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let p1 = db.create_project(ws_id, "Alpha", None).unwrap();
        let p2 = db.create_project(ws_id, "Beta", None).unwrap();

        let deleted = db.delete_workspace(ws_id).unwrap();
        assert!(deleted);

        assert!(db.get_project(p1.id).unwrap().is_none());
        assert!(db.get_project(p2.id).unwrap().is_none());
    }

    #[test]
    fn test_archive_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "Soon Archived", None).unwrap();
        let original_updated_at = project.updated_at;

        db.archive_project(project.id).unwrap();

        let fetched = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProjectStatus::Archived);
        assert!(fetched.updated_at >= original_updated_at);
    }

    #[test]
    fn test_project_status_serde_roundtrip() {
        for status in [
            ProjectStatus::Active,
            ProjectStatus::Running,
            ProjectStatus::Completed,
            ProjectStatus::Archived,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: ProjectStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_project_status_from_str() {
        assert_eq!("active".parse::<ProjectStatus>(), Ok(ProjectStatus::Active));
        assert_eq!(
            "running".parse::<ProjectStatus>(),
            Ok(ProjectStatus::Running)
        );
        assert_eq!(
            "completed".parse::<ProjectStatus>(),
            Ok(ProjectStatus::Completed)
        );
        assert_eq!(
            "archived".parse::<ProjectStatus>(),
            Ok(ProjectStatus::Archived)
        );
    }

    #[test]
    fn test_project_status_from_str_invalid() {
        let result = "unknown".parse::<ProjectStatus>();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "unknown project status: unknown"
        );
    }

    #[test]
    fn test_project_status_display() {
        assert_eq!(ProjectStatus::Active.to_string(), "active");
        assert_eq!(ProjectStatus::Running.to_string(), "running");
        assert_eq!(ProjectStatus::Completed.to_string(), "completed");
        assert_eq!(ProjectStatus::Archived.to_string(), "archived");
    }

    #[test]
    fn test_nullable_fields() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "Sparse", None).unwrap();

        let fetched = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(fetched.description, None);
        assert_eq!(fetched.worktree_path, None);
        assert_eq!(fetched.branch_name, None);
    }

    #[test]
    fn test_update_project_worktree() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "WT Test", None).unwrap();
        assert!(project.worktree_path.is_none());

        db.update_project_worktree(project.id, Some("/tmp/wt"), Some("project/wt-test"))
            .unwrap();

        let fetched = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(fetched.worktree_path, Some("/tmp/wt".to_string()));
        assert_eq!(fetched.branch_name, Some("project/wt-test".to_string()));

        // Clear worktree fields
        db.update_project_worktree(project.id, None, None).unwrap();

        let cleared = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(cleared.worktree_path, None);
        assert_eq!(cleared.branch_name, None);
    }

    #[test]
    fn test_create_project_with_worktree_non_git() {
        let (db, _dir) = create_test_db();
        let tmp = tempdir().unwrap();
        let ws = db
            .create_workspace(tmp.path().to_str().unwrap(), Some("non-git"))
            .unwrap();

        let project = db
            .create_project_with_worktree(ws.id, "feature-x", None)
            .unwrap();

        assert_eq!(project.name, "feature-x");
        assert!(project.worktree_path.is_none());
        assert!(project.branch_name.is_none());
    }

    #[test]
    fn test_archive_project_clears_worktree() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);

        let project = db.create_project(ws_id, "Archivable", None).unwrap();
        db.update_project_worktree(project.id, Some("/fake/wt"), Some("project/archivable"))
            .unwrap();

        db.archive_project(project.id).unwrap();

        let fetched = db.get_project(project.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProjectStatus::Archived);
        assert_eq!(fetched.worktree_path, None);
        assert_eq!(fetched.branch_name, None);
    }
}
