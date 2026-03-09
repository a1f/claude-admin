use crate::db::{Database, DbError};
use rusqlite::OptionalExtension;
use rusqlite::params;

const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("poll_interval", "2"),
    ("max_sessions", "50"),
    ("notification_enabled", "true"),
    (
        "notification_rules",
        r#"[{"to":"needs_input","enabled":true}]"#,
    ),
];

impl Database {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let value = self
            .connection()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;

        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.connection().execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            params![key, value, now],
        )?;

        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<bool, DbError> {
        let rows_affected = self
            .connection()
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;

        Ok(rows_affected > 0)
    }

    pub fn list_settings(&self) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self
            .connection()
            .prepare("SELECT key, value FROM settings ORDER BY key")?;

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut settings = Vec::new();
        for row_result in rows {
            settings.push(row_result?);
        }
        Ok(settings)
    }

    pub fn ensure_defaults(&self) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for &(key, value) in DEFAULT_SETTINGS {
            self.connection().execute(
                r#"
                INSERT OR IGNORE INTO settings (key, value, updated_at)
                VALUES (?1, ?2, ?3)
                "#,
                params![key, value, now],
            )?;
        }

        Ok(())
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

    // -- Schema --

    #[test]
    fn test_settings_table_exists() {
        let (db, _dir) = create_test_db();

        let table_exists: bool = db
            .connection()
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(table_exists);
    }

    #[test]
    fn test_settings_table_columns() {
        let (db, _dir) = create_test_db();

        let mut stmt = db
            .connection()
            .prepare("PRAGMA table_info(settings)")
            .unwrap();

        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(columns, vec!["key", "value", "updated_at"]);
    }

    #[test]
    fn test_settings_key_is_primary_key() {
        let (db, _dir) = create_test_db();

        db.connection()
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('dup', 'a', 1)",
                [],
            )
            .unwrap();

        let result = db.connection().execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('dup', 'b', 2)",
            [],
        );

        assert!(result.is_err());
    }

    // -- set_setting --

    #[test]
    fn test_set_setting_inserts_new_key() {
        let (db, _dir) = create_test_db();

        db.set_setting("theme", "dark").unwrap();

        let value = db.get_setting("theme").unwrap();
        assert_eq!(value, Some("dark".to_string()));
    }

    #[test]
    fn test_set_setting_upsert_overwrites() {
        let (db, _dir) = create_test_db();

        db.set_setting("poll_interval", "2").unwrap();
        db.set_setting("poll_interval", "5").unwrap();

        let value = db.get_setting("poll_interval").unwrap();
        assert_eq!(value, Some("5".to_string()));
    }

    #[test]
    fn test_set_setting_empty_value() {
        let (db, _dir) = create_test_db();

        db.set_setting("empty_key", "").unwrap();

        let value = db.get_setting("empty_key").unwrap();
        assert_eq!(value, Some(String::new()));
    }

    // -- get_setting --

    #[test]
    fn test_get_setting_returns_none_for_missing_key() {
        let (db, _dir) = create_test_db();

        let value = db.get_setting("nonexistent").unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_get_setting_returns_value() {
        let (db, _dir) = create_test_db();

        db.set_setting("color", "blue").unwrap();

        let value = db.get_setting("color").unwrap();
        assert_eq!(value, Some("blue".to_string()));
    }

    #[test]
    fn test_get_setting_returns_latest_value_after_upsert() {
        let (db, _dir) = create_test_db();

        db.set_setting("version", "1").unwrap();
        db.set_setting("version", "2").unwrap();
        db.set_setting("version", "3").unwrap();

        let value = db.get_setting("version").unwrap();
        assert_eq!(value, Some("3".to_string()));
    }

    // -- delete_setting --

    #[test]
    fn test_delete_setting_removes_key() {
        let (db, _dir) = create_test_db();

        db.set_setting("doomed", "value").unwrap();
        let deleted = db.delete_setting("doomed").unwrap();

        assert!(deleted);
        assert!(db.get_setting("doomed").unwrap().is_none());
    }

    #[test]
    fn test_delete_setting_returns_false_for_missing_key() {
        let (db, _dir) = create_test_db();

        let deleted = db.delete_setting("never_set").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_delete_setting_only_removes_target() {
        let (db, _dir) = create_test_db();

        db.set_setting("keep", "yes").unwrap();
        db.set_setting("remove", "bye").unwrap();

        db.delete_setting("remove").unwrap();

        assert_eq!(db.get_setting("keep").unwrap(), Some("yes".to_string()));
        assert!(db.get_setting("remove").unwrap().is_none());
    }

    #[test]
    fn test_delete_setting_then_reinsert() {
        let (db, _dir) = create_test_db();

        db.set_setting("cycle", "first").unwrap();
        db.delete_setting("cycle").unwrap();
        db.set_setting("cycle", "second").unwrap();

        let value = db.get_setting("cycle").unwrap();
        assert_eq!(value, Some("second".to_string()));
    }

    // -- list_settings --

    #[test]
    fn test_list_settings_empty_after_clearing() {
        let (db, _dir) = create_test_db();

        db.delete_setting("poll_interval").unwrap();
        db.delete_setting("max_sessions").unwrap();
        db.delete_setting("notification_enabled").unwrap();
        db.delete_setting("notification_rules").unwrap();

        let settings = db.list_settings().unwrap();
        assert!(settings.is_empty());
    }

    #[test]
    fn test_list_settings_returns_all() {
        let (db, _dir) = create_test_db();

        db.set_setting("extra_a", "1").unwrap();
        db.set_setting("extra_b", "2").unwrap();

        let settings = db.list_settings().unwrap();
        assert_eq!(settings.len(), 6);
    }

    #[test]
    fn test_list_settings_ordered_by_key() {
        let (db, _dir) = create_test_db();

        db.delete_setting("poll_interval").unwrap();
        db.delete_setting("max_sessions").unwrap();
        db.delete_setting("notification_enabled").unwrap();
        db.delete_setting("notification_rules").unwrap();

        db.set_setting("zebra", "z").unwrap();
        db.set_setting("apple", "a").unwrap();
        db.set_setting("mango", "m").unwrap();

        let settings = db.list_settings().unwrap();
        let keys: Vec<&str> = settings.iter().map(|(k, _)| k.as_str()).collect();

        assert_eq!(keys, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn test_list_settings_returns_key_value_pairs() {
        let (db, _dir) = create_test_db();

        db.delete_setting("poll_interval").unwrap();
        db.delete_setting("max_sessions").unwrap();
        db.delete_setting("notification_enabled").unwrap();
        db.delete_setting("notification_rules").unwrap();

        db.set_setting("name", "alice").unwrap();

        let settings = db.list_settings().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0], ("name".to_string(), "alice".to_string()));
    }

    // -- ensure_defaults --

    #[test]
    fn test_defaults_populated_on_open() {
        let (db, _dir) = create_test_db();

        assert_eq!(
            db.get_setting("poll_interval").unwrap(),
            Some("2".to_string())
        );
        assert_eq!(
            db.get_setting("max_sessions").unwrap(),
            Some("50".to_string())
        );
        assert_eq!(
            db.get_setting("notification_enabled").unwrap(),
            Some("true".to_string())
        );
        assert!(db.get_setting("notification_rules").unwrap().is_some());
    }

    #[test]
    fn test_ensure_defaults_idempotent() {
        let (db, _dir) = create_test_db();

        db.ensure_defaults().unwrap();

        let settings = db.list_settings().unwrap();
        assert_eq!(settings.len(), 4);

        assert_eq!(
            db.get_setting("poll_interval").unwrap(),
            Some("2".to_string())
        );
    }

    #[test]
    fn test_ensure_defaults_does_not_overwrite_user_changes() {
        let (db, _dir) = create_test_db();

        db.set_setting("poll_interval", "10").unwrap();

        db.ensure_defaults().unwrap();

        let value = db.get_setting("poll_interval").unwrap();
        assert_eq!(value, Some("10".to_string()));
    }

    #[test]
    fn test_ensure_defaults_fills_missing_after_delete() {
        let (db, _dir) = create_test_db();

        db.delete_setting("max_sessions").unwrap();
        assert!(db.get_setting("max_sessions").unwrap().is_none());

        db.ensure_defaults().unwrap();

        assert_eq!(
            db.get_setting("max_sessions").unwrap(),
            Some("50".to_string())
        );
    }

    #[test]
    fn test_ensure_defaults_multiple_calls_stable() {
        let (db, _dir) = create_test_db();

        db.ensure_defaults().unwrap();
        db.ensure_defaults().unwrap();
        db.ensure_defaults().unwrap();

        let settings = db.list_settings().unwrap();
        assert_eq!(settings.len(), 4);
    }

    // -- Integration --

    #[test]
    fn test_settings_full_lifecycle() {
        let (db, _dir) = create_test_db();

        db.set_setting("lang", "en").unwrap();
        assert_eq!(db.get_setting("lang").unwrap(), Some("en".to_string()));

        db.set_setting("lang", "fr").unwrap();
        assert_eq!(db.get_setting("lang").unwrap(), Some("fr".to_string()));

        let all = db.list_settings().unwrap();
        assert_eq!(all.len(), 5);
        assert!(all.iter().any(|(k, v)| k == "lang" && v == "fr"));

        let deleted = db.delete_setting("lang").unwrap();
        assert!(deleted);
        assert!(db.get_setting("lang").unwrap().is_none());

        let all = db.list_settings().unwrap();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_settings_independent_of_other_tables() {
        let (db, _dir) = create_test_db();

        let ws = db
            .create_workspace("/home/user/project", Some("project"))
            .unwrap();

        db.set_setting("custom_key", "custom_value").unwrap();
        db.delete_setting("poll_interval").unwrap();

        let fetched = db.get_workspace(ws.id).unwrap().unwrap();
        assert_eq!(fetched.name, "project");

        assert_eq!(
            db.get_setting("custom_key").unwrap(),
            Some("custom_value".to_string())
        );
    }

    #[test]
    fn test_database_reopen_preserves_settings() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        {
            let db = Database::open(&db_path).unwrap();
            db.set_setting("poll_interval", "7").unwrap();
            db.set_setting("user_pref", "custom").unwrap();
        }

        {
            let db = Database::open(&db_path).unwrap();

            assert_eq!(
                db.get_setting("poll_interval").unwrap(),
                Some("7".to_string())
            );
            assert_eq!(
                db.get_setting("user_pref").unwrap(),
                Some("custom".to_string())
            );
            assert_eq!(
                db.get_setting("max_sessions").unwrap(),
                Some("50".to_string())
            );
            assert_eq!(
                db.get_setting("notification_enabled").unwrap(),
                Some("true".to_string())
            );
            assert!(db.get_setting("notification_rules").unwrap().is_some());
        }
    }
}
