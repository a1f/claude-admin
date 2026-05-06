//! SQLite-backed persistent state for ca-daemon.
//!
//! Schema is in `migrations/`. The daemon opens the DB on startup and runs
//! `sqlx::migrate!()` (idempotent). Helpers exist for the inserts and selects
//! M1+ will need; M1-T3 wires only `count_*` (used by the startup log) and
//! the round-trip pair for `Architector` (used by the schema round-trip
//! test). Real CRUD lands in M2/M3 as the architector + task RPCs do.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use ca_lib::{Architector, ArchitectorState};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Open (or create) the SQLite database at `path` and run all pending
/// migrations. The pool has foreign-key enforcement enabled.
pub async fn open(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }

    let url = format!("sqlite://{}", path.display());
    let opts = SqliteConnectOptions::from_str(&url)
        .with_context(|| format!("parsing sqlite URL {url}"))?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;

    sqlx::migrate!()
        .run(&pool)
        .await
        .context("running migrations")?;

    Ok(pool)
}

/// Number of architector rows. Used by the startup log.
pub async fn count_architectors(pool: &SqlitePool) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM architectors")
        .fetch_one(pool)
        .await
        .context("counting architectors")?;
    Ok(n)
}

/// Number of task rows. Used by the startup log.
pub async fn count_tasks(pool: &SqlitePool) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks")
        .fetch_one(pool)
        .await
        .context("counting tasks")?;
    Ok(n)
}

/// Persist an `Architector`. M2's `ArchitectRegister` RPC will call this.
#[allow(dead_code)] // Wired up by M2's RPCs; covered by db tests today.
pub async fn insert_architector(pool: &SqlitePool, a: &Architector) -> Result<()> {
    let state_json = serde_json::to_string(&a.state).context("serializing state")?;
    sqlx::query(
        "INSERT INTO architectors (id, repo, milestone_id, issue_url, state, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&a.id)
    .bind(&a.repo)
    .bind(&a.milestone_id)
    .bind(&a.issue_url)
    .bind(&state_json)
    .bind(&a.created_at)
    .execute(pool)
    .await
    .with_context(|| format!("inserting architector {}", a.id))?;
    Ok(())
}

/// Fetch an `Architector` by id. Returns `Ok(None)` if not found.
#[allow(dead_code)] // Wired up by M2's RPCs; covered by db tests today.
pub async fn get_architector(pool: &SqlitePool, id: &str) -> Result<Option<Architector>> {
    let row: Option<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, repo, milestone_id, issue_url, state, created_at \
         FROM architectors WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("fetching architector {id}"))?;

    let Some((id, repo, milestone_id, issue_url, state_json, created_at)) = row else {
        return Ok(None);
    };
    let state: ArchitectorState =
        serde_json::from_str(&state_json).context("deserializing state")?;
    Ok(Some(Architector {
        id,
        repo,
        milestone_id,
        issue_url,
        state,
        created_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_lib::{Architector, ArchitectorOutcome, ArchitectorState};

    /// Build a fresh in-memory pool with all migrations applied.
    async fn fresh_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(opts)
            .await
            .expect("connect in-memory");
        sqlx::migrate!().run(&pool).await.expect("apply migrations");
        pool
    }

    #[tokio::test]
    async fn migrations_apply_to_empty_db() {
        let pool = fresh_pool().await;
        assert_eq!(count_architectors(&pool).await.unwrap(), 0);
        assert_eq!(count_tasks(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn migrations_idempotent() {
        let pool = fresh_pool().await;
        // Re-apply: should be a no-op, no errors.
        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("second migrate run");
        assert_eq!(count_architectors(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn schema_round_trips_one_architector() {
        let pool = fresh_pool().await;
        let a = Architector {
            id: "01J0K2X9".to_owned(),
            repo: "a1f/claude-admin".to_owned(),
            milestone_id: "M1".to_owned(),
            issue_url: "https://github.com/a1f/claude-admin/issues/9".to_owned(),
            state: ArchitectorState::Active,
            created_at: "2026-05-05T00:00:00Z".to_owned(),
        };
        insert_architector(&pool, &a).await.unwrap();
        let got = get_architector(&pool, "01J0K2X9").await.unwrap();
        assert_eq!(got, Some(a));
    }

    #[tokio::test]
    async fn schema_round_trips_state_with_payload() {
        let pool = fresh_pool().await;
        let a = Architector {
            id: "01J0K2XA".to_owned(),
            repo: "a1f/claude-admin".to_owned(),
            milestone_id: "M1".to_owned(),
            issue_url: "https://example/1".to_owned(),
            state: ArchitectorState::Closed {
                outcome: ArchitectorOutcome::Shipped,
            },
            created_at: "2026-05-05T00:00:00Z".to_owned(),
        };
        insert_architector(&pool, &a).await.unwrap();
        let got = get_architector(&pool, "01J0K2XA").await.unwrap();
        assert_eq!(got, Some(a));
    }

    #[tokio::test]
    async fn get_architector_returns_none_for_missing() {
        let pool = fresh_pool().await;
        assert_eq!(get_architector(&pool, "no-such-id").await.unwrap(), None);
    }
}
