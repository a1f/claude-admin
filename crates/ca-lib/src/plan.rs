use crate::db::{Database, DbError};
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Abandoned,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Draft => "draft",
            PlanStatus::Active => "active",
            PlanStatus::Completed => "completed",
            PlanStatus::Abandoned => "abandoned",
        }
    }
}

impl fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PlanStatus {
    type Err = ParsePlanStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(PlanStatus::Draft),
            "active" => Ok(PlanStatus::Active),
            "completed" => Ok(PlanStatus::Completed),
            "abandoned" => Ok(PlanStatus::Abandoned),
            _ => Err(ParsePlanStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePlanStatusError(pub String);

impl fmt::Display for ParsePlanStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown plan status: {}", self.0)
    }
}

impl std::error::Error for ParsePlanStatusError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Skipped,
}

impl StepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::InProgress => "in_progress",
            StepStatus::Completed => "completed",
            StepStatus::Blocked => "blocked",
            StepStatus::Skipped => "skipped",
        }
    }
}

impl fmt::Display for StepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StepStatus {
    type Err = ParseStepStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(StepStatus::Pending),
            "in_progress" => Ok(StepStatus::InProgress),
            "completed" => Ok(StepStatus::Completed),
            "blocked" => Ok(StepStatus::Blocked),
            "skipped" => Ok(StepStatus::Skipped),
            _ => Err(ParseStepStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStepStatusError(pub String);

impl fmt::Display for ParseStepStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown step status: {}", self.0)
    }
}

impl std::error::Error for ParseStepStatusError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContent {
    pub phases: Vec<Phase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub description: String,
    pub status: StepStatus,
    pub exit_criteria: ExitCriteria,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitCriteria {
    pub description: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub content: PlanContent,
    pub status: PlanStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_plan(row: &rusqlite::Row) -> rusqlite::Result<Result<Plan, DbError>> {
    let status_str: String = row.get(4)?;
    let status = match status_str.parse::<PlanStatus>() {
        Ok(s) => s,
        Err(_) => return Ok(Err(DbError::InvalidState(status_str))),
    };

    let content_str: String = row.get(3)?;
    let content: PlanContent = match serde_json::from_str(&content_str) {
        Ok(c) => c,
        Err(e) => return Ok(Err(DbError::Serialization(e))),
    };

    Ok(Ok(Plan {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        content,
        status,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    }))
}

impl Database {
    pub fn create_plan(
        &self,
        project_id: i64,
        name: &str,
        content: &PlanContent,
    ) -> Result<Plan, DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let content_json = serde_json::to_string(content)?;

        self.connection().execute(
            r#"
            INSERT INTO plans (project_id, name, content, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                project_id,
                name,
                content_json,
                PlanStatus::Draft.as_str(),
                now,
                now,
            ],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(Plan {
            id,
            project_id,
            name: name.to_string(),
            content: content.clone(),
            status: PlanStatus::Draft,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_plan(&self, id: i64) -> Result<Option<Plan>, DbError> {
        let result = self
            .connection()
            .query_row(
                r#"
                SELECT id, project_id, name, content, status, created_at, updated_at
                FROM plans WHERE id = ?1
                "#,
                params![id],
                |row| row_to_plan(row),
            )
            .optional()?;

        match result {
            Some(Ok(plan)) => Ok(Some(plan)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn get_active_plan(&self, project_id: i64) -> Result<Option<Plan>, DbError> {
        let result = self
            .connection()
            .query_row(
                r#"
                SELECT id, project_id, name, content, status, created_at, updated_at
                FROM plans
                WHERE project_id = ?1 AND status = ?2
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
                params![project_id, PlanStatus::Active.as_str()],
                |row| row_to_plan(row),
            )
            .optional()?;

        match result {
            Some(Ok(plan)) => Ok(Some(plan)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn list_plans_by_project(&self, project_id: i64) -> Result<Vec<Plan>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, project_id, name, content, status, created_at, updated_at
            FROM plans
            WHERE project_id = ?1
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map(params![project_id], |row| row_to_plan(row))?;

        let mut plans = Vec::new();
        for row_result in rows {
            plans.push(row_result??);
        }
        Ok(plans)
    }

    pub fn update_plan_status(&self, id: i64, status: PlanStatus) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            UPDATE plans SET
                status = ?2,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![id, status.as_str(), now],
        )?;
        Ok(())
    }

    pub fn update_step_status(
        &self,
        plan_id: i64,
        step_id: &str,
        status: StepStatus,
    ) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let content_str: String = self
            .connection()
            .query_row(
                "SELECT content FROM plans WHERE id = ?1",
                params![plan_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| DbError::InvalidState(format!("plan not found: {plan_id}")))?;

        let mut content: PlanContent = serde_json::from_str(&content_str)?;

        let step = content
            .phases
            .iter_mut()
            .flat_map(|phase| phase.steps.iter_mut())
            .find(|step| step.id == step_id)
            .ok_or_else(|| DbError::InvalidState(format!("step not found: {step_id}")))?;

        step.status = status;

        let updated_json = serde_json::to_string(&content)?;

        self.connection().execute(
            r#"
            UPDATE plans SET
                content = ?2,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![plan_id, updated_json, now],
        )?;

        Ok(())
    }

    pub fn delete_plan(&self, id: i64) -> Result<bool, DbError> {
        let rows_affected = self
            .connection()
            .execute("DELETE FROM plans WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::plan::{
        ExitCriteria, Phase, PlanContent, PlanStatus, Step, StepStatus,
    };
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

    fn create_test_project(db: &Database, ws_id: i64) -> i64 {
        let project = db
            .create_project(ws_id, "Test Project", None)
            .unwrap();
        project.id
    }

    fn sample_content() -> PlanContent {
        PlanContent {
            phases: vec![
                Phase {
                    name: "Setup".to_string(),
                    steps: vec![
                        Step {
                            id: "0.1".to_string(),
                            description: "Initialize project".to_string(),
                            status: StepStatus::Pending,
                            exit_criteria: ExitCriteria {
                                description: "Project compiles".to_string(),
                                commands: vec!["cargo build".to_string()],
                            },
                        },
                        Step {
                            id: "0.2".to_string(),
                            description: "Add dependencies".to_string(),
                            status: StepStatus::Pending,
                            exit_criteria: ExitCriteria {
                                description: "All deps resolve".to_string(),
                                commands: vec!["cargo check".to_string()],
                            },
                        },
                    ],
                },
                Phase {
                    name: "Implementation".to_string(),
                    steps: vec![
                        Step {
                            id: "1.1".to_string(),
                            description: "Add models".to_string(),
                            status: StepStatus::Pending,
                            exit_criteria: ExitCriteria {
                                description: "Tests pass".to_string(),
                                commands: vec![
                                    "cargo test".to_string(),
                                    "cargo clippy".to_string(),
                                ],
                            },
                        },
                        Step {
                            id: "1.2".to_string(),
                            description: "Add API".to_string(),
                            status: StepStatus::Pending,
                            exit_criteria: ExitCriteria {
                                description: "Endpoints respond".to_string(),
                                commands: vec![],
                            },
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn test_create_plan() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "My Plan", &content).unwrap();

        assert!(plan.id > 0);
        assert_eq!(plan.project_id, proj_id);
        assert_eq!(plan.name, "My Plan");
        assert_eq!(plan.status, PlanStatus::Draft);
        assert_eq!(plan.content, content);
        assert!(plan.created_at > 0);
        assert_eq!(plan.created_at, plan.updated_at);
    }

    #[test]
    fn test_create_plan_json_roundtrip() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Roundtrip", &content).unwrap();
        let fetched = db.get_plan(plan.id).unwrap().unwrap();

        assert_eq!(fetched.content, content);
    }

    #[test]
    fn test_get_plan() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let created = db.create_plan(proj_id, "Fetch Me", &content).unwrap();
        let fetched = db.get_plan(created.id).unwrap().unwrap();

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.project_id, proj_id);
        assert_eq!(fetched.name, "Fetch Me");
        assert_eq!(fetched.status, PlanStatus::Draft);
        assert_eq!(fetched.content, content);
        assert_eq!(fetched.created_at, created.created_at);
    }

    #[test]
    fn test_get_plan_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_plan(9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_active_plan() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let _draft = db.create_plan(proj_id, "Draft Plan", &content).unwrap();
        let active = db.create_plan(proj_id, "Active Plan", &content).unwrap();
        db.update_plan_status(active.id, PlanStatus::Active).unwrap();

        let fetched = db.get_active_plan(proj_id).unwrap().unwrap();
        assert_eq!(fetched.id, active.id);
        assert_eq!(fetched.name, "Active Plan");
        assert_eq!(fetched.status, PlanStatus::Active);
    }

    #[test]
    fn test_get_active_plan_none() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        db.create_plan(proj_id, "Draft Plan", &content).unwrap();

        let result = db.get_active_plan(proj_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_plans_by_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj1 = create_test_project(&db, ws_id);
        let proj2 = db.create_project(ws_id, "Other Project", None).unwrap().id;
        let content = sample_content();

        let p1 = db.create_plan(proj1, "Plan A", &content).unwrap();
        let _p2 = db.create_plan(proj2, "Plan B", &content).unwrap();
        let p3 = db.create_plan(proj1, "Plan C", &content).unwrap();

        let plans = db.list_plans_by_project(proj1).unwrap();
        assert_eq!(plans.len(), 2);

        let ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        assert!(ids.contains(&p1.id));
        assert!(ids.contains(&p3.id));
    }

    #[test]
    fn test_list_plans_empty() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);

        let plans = db.list_plans_by_project(proj_id).unwrap();
        assert!(plans.is_empty());
    }

    #[test]
    fn test_update_plan_status() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Statusful", &content).unwrap();
        let original_updated_at = plan.updated_at;

        db.update_plan_status(plan.id, PlanStatus::Active).unwrap();

        let fetched = db.get_plan(plan.id).unwrap().unwrap();
        assert_eq!(fetched.status, PlanStatus::Active);
        assert!(fetched.updated_at >= original_updated_at);
    }

    #[test]
    fn test_update_step_status() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Step Plan", &content).unwrap();

        db.update_step_status(plan.id, "1.1", StepStatus::InProgress).unwrap();

        let fetched = db.get_plan(plan.id).unwrap().unwrap();
        let step = fetched
            .content
            .phases
            .iter()
            .flat_map(|p| &p.steps)
            .find(|s| s.id == "1.1")
            .unwrap();
        assert_eq!(step.status, StepStatus::InProgress);

        // Other steps remain unchanged
        assert_eq!(fetched.content.phases[0].steps[0].status, StepStatus::Pending);
        assert_eq!(fetched.content.phases[1].steps[1].status, StepStatus::Pending);
    }

    #[test]
    fn test_update_step_status_not_found() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Step Plan", &content).unwrap();

        let result = db.update_step_status(plan.id, "9.9", StepStatus::Completed);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("step not found: 9.9"), "got: {err_msg}");
    }

    #[test]
    fn test_update_step_status_plan_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.update_step_status(9999, "0.1", StepStatus::Completed);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("plan not found: 9999"), "got: {err_msg}");
    }

    #[test]
    fn test_update_step_status_updates_timestamp() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Timed Plan", &content).unwrap();
        let original_updated_at = plan.updated_at;

        db.update_step_status(plan.id, "0.1", StepStatus::Completed).unwrap();

        let fetched = db.get_plan(plan.id).unwrap().unwrap();
        assert!(fetched.updated_at >= original_updated_at);
    }

    #[test]
    fn test_delete_plan() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let plan = db.create_plan(proj_id, "Doomed", &content).unwrap();

        let deleted = db.delete_plan(plan.id).unwrap();
        assert!(deleted);

        let result = db.get_plan(plan.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_plan_not_found() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_plan(9999).unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_cascade_delete_project() {
        let (db, _dir) = create_test_db();
        let ws_id = create_test_workspace(&db);
        let proj_id = create_test_project(&db, ws_id);
        let content = sample_content();

        let p1 = db.create_plan(proj_id, "Plan A", &content).unwrap();
        let p2 = db.create_plan(proj_id, "Plan B", &content).unwrap();

        db.delete_project(proj_id).unwrap();

        assert!(db.get_plan(p1.id).unwrap().is_none());
        assert!(db.get_plan(p2.id).unwrap().is_none());
    }

    #[test]
    fn test_plan_content_json_roundtrip() {
        let content = sample_content();
        let json = serde_json::to_string(&content).unwrap();
        let parsed: PlanContent = serde_json::from_str(&json).unwrap();
        assert_eq!(content, parsed);
    }

    #[test]
    fn test_plan_status_serde_roundtrip() {
        for status in [
            PlanStatus::Draft,
            PlanStatus::Active,
            PlanStatus::Completed,
            PlanStatus::Abandoned,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: PlanStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_step_status_serde_roundtrip() {
        for status in [
            StepStatus::Pending,
            StepStatus::InProgress,
            StepStatus::Completed,
            StepStatus::Blocked,
            StepStatus::Skipped,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: StepStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_plan_status_from_str() {
        assert_eq!("draft".parse::<PlanStatus>(), Ok(PlanStatus::Draft));
        assert_eq!("active".parse::<PlanStatus>(), Ok(PlanStatus::Active));
        assert_eq!("completed".parse::<PlanStatus>(), Ok(PlanStatus::Completed));
        assert_eq!("abandoned".parse::<PlanStatus>(), Ok(PlanStatus::Abandoned));
    }

    #[test]
    fn test_plan_status_from_str_invalid() {
        let result = "garbage".parse::<PlanStatus>();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "unknown plan status: garbage"
        );
    }

    #[test]
    fn test_step_status_from_str() {
        assert_eq!("pending".parse::<StepStatus>(), Ok(StepStatus::Pending));
        assert_eq!("in_progress".parse::<StepStatus>(), Ok(StepStatus::InProgress));
        assert_eq!("completed".parse::<StepStatus>(), Ok(StepStatus::Completed));
        assert_eq!("blocked".parse::<StepStatus>(), Ok(StepStatus::Blocked));
        assert_eq!("skipped".parse::<StepStatus>(), Ok(StepStatus::Skipped));
    }

    #[test]
    fn test_step_status_from_str_invalid() {
        let result = "garbage".parse::<StepStatus>();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "unknown step status: garbage"
        );
    }

    #[test]
    fn test_plan_status_display() {
        assert_eq!(PlanStatus::Draft.to_string(), "draft");
        assert_eq!(PlanStatus::Active.to_string(), "active");
        assert_eq!(PlanStatus::Completed.to_string(), "completed");
        assert_eq!(PlanStatus::Abandoned.to_string(), "abandoned");
    }

    #[test]
    fn test_step_status_display() {
        assert_eq!(StepStatus::Pending.to_string(), "pending");
        assert_eq!(StepStatus::InProgress.to_string(), "in_progress");
        assert_eq!(StepStatus::Completed.to_string(), "completed");
        assert_eq!(StepStatus::Blocked.to_string(), "blocked");
        assert_eq!(StepStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn test_plan_content_complex() {
        let content = PlanContent {
            phases: vec![
                Phase {
                    name: "Phase 0: Baseline".to_string(),
                    steps: vec![Step {
                        id: "0.1".to_string(),
                        description: "Verify build".to_string(),
                        status: StepStatus::Completed,
                        exit_criteria: ExitCriteria {
                            description: "Clean build".to_string(),
                            commands: vec!["cargo build".to_string()],
                        },
                    }],
                },
                Phase {
                    name: "Phase 1: Core".to_string(),
                    steps: vec![
                        Step {
                            id: "1.1".to_string(),
                            description: "Add schema".to_string(),
                            status: StepStatus::InProgress,
                            exit_criteria: ExitCriteria {
                                description: "Migration runs".to_string(),
                                commands: vec![],
                            },
                        },
                        Step {
                            id: "1.2".to_string(),
                            description: "Add CRUD".to_string(),
                            status: StepStatus::Blocked,
                            exit_criteria: ExitCriteria {
                                description: "All tests pass".to_string(),
                                commands: vec![
                                    "cargo test -p ca-lib".to_string(),
                                    "cargo clippy -- -D warnings".to_string(),
                                ],
                            },
                        },
                    ],
                },
                Phase {
                    name: "Phase 2: Polish".to_string(),
                    steps: vec![Step {
                        id: "2.1".to_string(),
                        description: "Cleanup".to_string(),
                        status: StepStatus::Skipped,
                        exit_criteria: ExitCriteria {
                            description: "No warnings".to_string(),
                            commands: vec!["cargo clippy".to_string()],
                        },
                    }],
                },
            ],
        };

        let json = serde_json::to_string(&content).unwrap();
        let parsed: PlanContent = serde_json::from_str(&json).unwrap();
        assert_eq!(content, parsed);
        assert_eq!(parsed.phases.len(), 3);
        assert_eq!(parsed.phases[1].steps.len(), 2);
        assert_eq!(parsed.phases[1].steps[1].status, StepStatus::Blocked);
    }

    #[test]
    fn test_exit_criteria_with_commands() {
        let with_commands = ExitCriteria {
            description: "All checks pass".to_string(),
            commands: vec![
                "cargo test".to_string(),
                "cargo clippy -- -D warnings".to_string(),
                "cargo fmt --check".to_string(),
            ],
        };

        let json = serde_json::to_string(&with_commands).unwrap();
        let parsed: ExitCriteria = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.commands.len(), 3);
        assert_eq!(parsed.commands[0], "cargo test");

        let empty_commands = ExitCriteria {
            description: "Manual review".to_string(),
            commands: vec![],
        };

        let json_empty = serde_json::to_string(&empty_commands).unwrap();
        let parsed_empty: ExitCriteria = serde_json::from_str(&json_empty).unwrap();
        assert_eq!(parsed_empty.commands.len(), 0);
    }
}
