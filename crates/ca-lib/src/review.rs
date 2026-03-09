use crate::db::{Database, DbError};
use rusqlite::OptionalExtension;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    InProgress,
    Approved,
    ChangesRequested,
}

impl ReviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStatus::Pending => "pending",
            ReviewStatus::InProgress => "in_progress",
            ReviewStatus::Approved => "approved",
            ReviewStatus::ChangesRequested => "changes_requested",
        }
    }
}

impl fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ReviewStatus {
    type Err = ParseReviewStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(ReviewStatus::Pending),
            "in_progress" => Ok(ReviewStatus::InProgress),
            "approved" => Ok(ReviewStatus::Approved),
            "changes_requested" => Ok(ReviewStatus::ChangesRequested),
            _ => Err(ParseReviewStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseReviewStatusError(pub String);

impl fmt::Display for ParseReviewStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown review status: {}", self.0)
    }
}

impl std::error::Error for ParseReviewStatusError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: i64,
    pub session_id: Option<String>,
    pub project_id: Option<i64>,
    pub branch: String,
    pub base_commit: String,
    pub head_commit: String,
    pub status: ReviewStatus,
    pub round: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: i64,
    pub review_id: i64,
    pub commit_sha: String,
    pub file_path: String,
    pub line_number: u32,
    pub body: String,
    pub resolved: bool,
    pub created_at: i64,
}

fn row_to_review(row: &rusqlite::Row) -> rusqlite::Result<Result<Review, DbError>> {
    let status_str: String = row.get(6)?;
    let status = match status_str.parse::<ReviewStatus>() {
        Ok(s) => s,
        Err(_) => return Ok(Err(DbError::InvalidState(status_str))),
    };

    Ok(Ok(Review {
        id: row.get(0)?,
        session_id: row.get(1)?,
        project_id: row.get(2)?,
        branch: row.get(3)?,
        base_commit: row.get(4)?,
        head_commit: row.get(5)?,
        status,
        round: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    }))
}

fn row_to_comment(row: &rusqlite::Row) -> rusqlite::Result<ReviewComment> {
    let resolved_int: i32 = row.get(6)?;
    let line_number: i32 = row.get(4)?;

    Ok(ReviewComment {
        id: row.get(0)?,
        review_id: row.get(1)?,
        commit_sha: row.get(2)?,
        file_path: row.get(3)?,
        line_number: line_number as u32,
        body: row.get(5)?,
        resolved: resolved_int != 0,
        created_at: row.get(7)?,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl Database {
    pub fn create_review(
        &self,
        session_id: Option<&str>,
        project_id: Option<i64>,
        branch: &str,
        base_commit: &str,
        head_commit: &str,
    ) -> Result<Review, DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            INSERT INTO reviews (session_id, project_id, branch, base_commit,
                                 head_commit, status, round, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                session_id,
                project_id,
                branch,
                base_commit,
                head_commit,
                ReviewStatus::Pending.as_str(),
                1,
                now,
                now,
            ],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(Review {
            id,
            session_id: session_id.map(String::from),
            project_id,
            branch: branch.to_string(),
            base_commit: base_commit.to_string(),
            head_commit: head_commit.to_string(),
            status: ReviewStatus::Pending,
            round: 1,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_review(&self, id: i64) -> Result<Option<Review>, DbError> {
        let result = self
            .connection()
            .query_row(
                r#"
                SELECT id, session_id, project_id, branch, base_commit,
                       head_commit, status, round, created_at, updated_at
                FROM reviews WHERE id = ?1
                "#,
                params![id],
                row_to_review,
            )
            .optional()?;

        match result {
            Some(Ok(review)) => Ok(Some(review)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn list_reviews_by_project(&self, project_id: i64) -> Result<Vec<Review>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, session_id, project_id, branch, base_commit,
                   head_commit, status, round, created_at, updated_at
            FROM reviews
            WHERE project_id = ?1
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map(params![project_id], row_to_review)?;

        let mut reviews = Vec::new();
        for row_result in rows {
            reviews.push(row_result??);
        }
        Ok(reviews)
    }

    pub fn list_reviews_by_session(&self, session_id: &str) -> Result<Vec<Review>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, session_id, project_id, branch, base_commit,
                   head_commit, status, round, created_at, updated_at
            FROM reviews
            WHERE session_id = ?1
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map(params![session_id], row_to_review)?;

        let mut reviews = Vec::new();
        for row_result in rows {
            reviews.push(row_result??);
        }
        Ok(reviews)
    }

    pub fn update_review_status(&self, id: i64, status: ReviewStatus) -> Result<(), DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            UPDATE reviews SET
                status = ?2,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![id, status.as_str(), now],
        )?;
        Ok(())
    }

    pub fn increment_review_round(&self, id: i64) -> Result<(), DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            UPDATE reviews SET
                round = round + 1,
                updated_at = ?2
            WHERE id = ?1
            "#,
            params![id, now],
        )?;
        Ok(())
    }

    pub fn delete_review(&self, id: i64) -> Result<bool, DbError> {
        let rows_affected = self
            .connection()
            .execute("DELETE FROM reviews WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }

    pub fn add_review_comment(
        &self,
        review_id: i64,
        commit_sha: &str,
        file_path: &str,
        line_number: u32,
        body: &str,
    ) -> Result<ReviewComment, DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            INSERT INTO review_comments (review_id, commit_sha, file_path,
                                         line_number, body, resolved, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                review_id,
                commit_sha,
                file_path,
                line_number as i32,
                body,
                0,
                now
            ],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(ReviewComment {
            id,
            review_id,
            commit_sha: commit_sha.to_string(),
            file_path: file_path.to_string(),
            line_number,
            body: body.to_string(),
            resolved: false,
            created_at: now,
        })
    }

    pub fn get_review_comments(&self, review_id: i64) -> Result<Vec<ReviewComment>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, review_id, commit_sha, file_path, line_number,
                   body, resolved, created_at
            FROM review_comments
            WHERE review_id = ?1
            ORDER BY file_path, line_number
            "#,
        )?;

        let rows = stmt.query_map(params![review_id], row_to_comment)?;

        let mut comments = Vec::new();
        for row_result in rows {
            comments.push(row_result?);
        }
        Ok(comments)
    }

    pub fn resolve_comment(&self, comment_id: i64) -> Result<(), DbError> {
        self.connection().execute(
            "UPDATE review_comments SET resolved = 1 WHERE id = ?1",
            params![comment_id],
        )?;
        Ok(())
    }

    pub fn get_review_with_comments(
        &self,
        id: i64,
    ) -> Result<Option<(Review, Vec<ReviewComment>)>, DbError> {
        let review = match self.get_review(id)? {
            Some(r) => r,
            None => return Ok(None),
        };

        let comments = self.get_review_comments(id)?;
        Ok(Some((review, comments)))
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::review::ReviewStatus;
    use tempfile::tempdir;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    fn create_test_workspace(db: &Database) -> i64 {
        db.create_workspace("/home/user/myproject", Some("myproject"))
            .unwrap()
            .id
    }

    fn create_test_project(db: &Database, ws_id: i64) -> i64 {
        db.create_project(ws_id, "Test Project", None).unwrap().id
    }

    #[test]
    fn test_create_review() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);

        let review = db
            .create_review(
                Some("sess-1"),
                Some(proj_id),
                "feature/login",
                "abc123",
                "def456",
            )
            .unwrap();

        assert!(review.id > 0);
        assert_eq!(review.session_id, Some("sess-1".to_string()));
        assert_eq!(review.project_id, Some(proj_id));
        assert_eq!(review.branch, "feature/login");
        assert_eq!(review.base_commit, "abc123");
        assert_eq!(review.head_commit, "def456");
        assert_eq!(review.status, ReviewStatus::Pending);
        assert_eq!(review.round, 1);
        assert!(review.created_at > 0);
        assert_eq!(review.created_at, review.updated_at);

        let fetched = db.get_review(review.id).unwrap().unwrap();
        assert_eq!(fetched.id, review.id);
        assert_eq!(fetched.branch, "feature/login");
    }

    #[test]
    fn test_list_reviews_by_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj1 = create_test_project(&db, ws_id);
        let proj2 = db.create_project(ws_id, "Other Project", None).unwrap().id;

        db.create_review(None, Some(proj1), "main", "aaa", "bbb")
            .unwrap();
        db.create_review(None, Some(proj2), "main", "ccc", "ddd")
            .unwrap();
        db.create_review(None, Some(proj1), "dev", "eee", "fff")
            .unwrap();

        let reviews = db.list_reviews_by_project(proj1).unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(reviews.iter().all(|r| r.project_id == Some(proj1)));
    }

    #[test]
    fn test_list_reviews_by_session() {
        let (db, _dir) = create_test_db();

        db.create_review(Some("sess-a"), None, "main", "aaa", "bbb")
            .unwrap();
        db.create_review(Some("sess-b"), None, "main", "ccc", "ddd")
            .unwrap();
        db.create_review(Some("sess-a"), None, "dev", "eee", "fff")
            .unwrap();

        let reviews = db.list_reviews_by_session("sess-a").unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(
            reviews
                .iter()
                .all(|r| r.session_id.as_deref() == Some("sess-a"))
        );
    }

    #[test]
    fn test_update_review_status() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();
        assert_eq!(review.status, ReviewStatus::Pending);

        db.update_review_status(review.id, ReviewStatus::InProgress)
            .unwrap();

        let fetched = db.get_review(review.id).unwrap().unwrap();
        assert_eq!(fetched.status, ReviewStatus::InProgress);
        assert!(fetched.updated_at >= review.updated_at);

        db.update_review_status(review.id, ReviewStatus::Approved)
            .unwrap();

        let fetched2 = db.get_review(review.id).unwrap().unwrap();
        assert_eq!(fetched2.status, ReviewStatus::Approved);
    }

    #[test]
    fn test_increment_review_round() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();
        assert_eq!(review.round, 1);

        db.increment_review_round(review.id).unwrap();
        let fetched = db.get_review(review.id).unwrap().unwrap();
        assert_eq!(fetched.round, 2);

        db.increment_review_round(review.id).unwrap();
        let fetched2 = db.get_review(review.id).unwrap().unwrap();
        assert_eq!(fetched2.round, 3);
        assert!(fetched2.updated_at >= review.updated_at);
    }

    #[test]
    fn test_delete_review_cascades_comments() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();

        let c1 = db
            .add_review_comment(review.id, "aaa", "src/main.rs", 10, "Fix this")
            .unwrap();
        let c2 = db
            .add_review_comment(review.id, "aaa", "src/lib.rs", 20, "And this")
            .unwrap();

        assert!(c1.id > 0);
        assert!(c2.id > 0);

        let deleted = db.delete_review(review.id).unwrap();
        assert!(deleted);

        assert!(db.get_review(review.id).unwrap().is_none());

        let comments = db.get_review_comments(review.id).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn test_add_review_comment() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();

        let comment = db
            .add_review_comment(review.id, "abc123", "src/main.rs", 42, "Needs refactor")
            .unwrap();

        assert!(comment.id > 0);
        assert_eq!(comment.review_id, review.id);
        assert_eq!(comment.commit_sha, "abc123");
        assert_eq!(comment.file_path, "src/main.rs");
        assert_eq!(comment.line_number, 42);
        assert_eq!(comment.body, "Needs refactor");
        assert!(!comment.resolved);
        assert!(comment.created_at > 0);
    }

    #[test]
    fn test_get_review_comments() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();

        db.add_review_comment(review.id, "aaa", "src/b.rs", 5, "Comment B")
            .unwrap();
        db.add_review_comment(review.id, "aaa", "src/a.rs", 10, "Comment A")
            .unwrap();
        db.add_review_comment(review.id, "aaa", "src/a.rs", 3, "Comment A early")
            .unwrap();

        let comments = db.get_review_comments(review.id).unwrap();
        assert_eq!(comments.len(), 3);

        // Ordered by file_path then line_number
        assert_eq!(comments[0].file_path, "src/a.rs");
        assert_eq!(comments[0].line_number, 3);
        assert_eq!(comments[1].file_path, "src/a.rs");
        assert_eq!(comments[1].line_number, 10);
        assert_eq!(comments[2].file_path, "src/b.rs");
        assert_eq!(comments[2].line_number, 5);
    }

    #[test]
    fn test_resolve_comment() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();
        let comment = db
            .add_review_comment(review.id, "aaa", "src/main.rs", 1, "Fix me")
            .unwrap();

        assert!(!comment.resolved);

        db.resolve_comment(comment.id).unwrap();

        let comments = db.get_review_comments(review.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].resolved);
    }

    #[test]
    fn test_get_review_with_comments() {
        let (db, _dir) = create_test_db();

        let review = db
            .create_review(Some("sess-1"), None, "main", "aaa", "bbb")
            .unwrap();

        db.add_review_comment(review.id, "aaa", "src/main.rs", 10, "First")
            .unwrap();
        db.add_review_comment(review.id, "aaa", "src/main.rs", 20, "Second")
            .unwrap();

        let (fetched_review, comments) = db.get_review_with_comments(review.id).unwrap().unwrap();

        assert_eq!(fetched_review.id, review.id);
        assert_eq!(fetched_review.branch, "main");
        assert_eq!(comments.len(), 2);

        // Not found case
        let missing = db.get_review_with_comments(9999).unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_review_status_from_str() {
        assert_eq!("pending".parse::<ReviewStatus>(), Ok(ReviewStatus::Pending));
        assert_eq!(
            "in_progress".parse::<ReviewStatus>(),
            Ok(ReviewStatus::InProgress)
        );
        assert_eq!(
            "approved".parse::<ReviewStatus>(),
            Ok(ReviewStatus::Approved)
        );
        assert_eq!(
            "changes_requested".parse::<ReviewStatus>(),
            Ok(ReviewStatus::ChangesRequested)
        );

        let err = "garbage".parse::<ReviewStatus>();
        assert!(err.is_err());
        assert_eq!(
            err.unwrap_err().to_string(),
            "unknown review status: garbage"
        );
    }

    #[test]
    fn test_review_status_serde_roundtrip() {
        for status in [
            ReviewStatus::Pending,
            ReviewStatus::InProgress,
            ReviewStatus::Approved,
            ReviewStatus::ChangesRequested,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: ReviewStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_review_status_display() {
        assert_eq!(ReviewStatus::Pending.to_string(), "pending");
        assert_eq!(ReviewStatus::InProgress.to_string(), "in_progress");
        assert_eq!(ReviewStatus::Approved.to_string(), "approved");
        assert_eq!(
            ReviewStatus::ChangesRequested.to_string(),
            "changes_requested"
        );
    }

    #[test]
    fn test_create_review_with_null_optionals() {
        let (db, _dir) = create_test_db();

        let review = db.create_review(None, None, "main", "aaa", "bbb").unwrap();

        assert!(review.session_id.is_none());
        assert!(review.project_id.is_none());

        let fetched = db.get_review(review.id).unwrap().unwrap();
        assert!(fetched.session_id.is_none());
        assert!(fetched.project_id.is_none());
    }

    #[test]
    fn test_get_review_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_review(9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_review_not_found() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_review(9999).unwrap();
        assert!(!deleted);
    }
}
