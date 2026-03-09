use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::Database;
use crate::models::SessionState;

#[derive(Error, Debug)]
pub enum NotifyError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("osascript failed: {0}")]
    CommandFailed(String),
    #[error("database error: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationRule {
    #[serde(default)]
    pub from: Option<SessionState>,
    pub to: SessionState,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub rules: Vec<NotificationRule>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: vec![NotificationRule {
                from: None,
                to: SessionState::NeedsInput,
                enabled: true,
            }],
        }
    }
}

impl NotificationConfig {
    pub fn should_notify(&self, from: &SessionState, to: &SessionState) -> bool {
        if !self.enabled {
            return false;
        }

        self.rules.iter().any(|rule| {
            rule.enabled && rule.to == *to && rule.from.as_ref().is_none_or(|f| f == from)
        })
    }

    pub fn from_settings(db: &Database) -> Self {
        let enabled = db
            .get_setting("notification_enabled")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);

        let rules = db
            .get_setting("notification_rules")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str::<Vec<NotificationRule>>(&v).ok())
            .unwrap_or_else(|| Self::default().rules);

        Self { enabled, rules }
    }

    pub fn save_to_settings(&self, db: &Database) -> Result<(), NotifyError> {
        db.set_setting("notification_enabled", &self.enabled.to_string())?;
        let rules_json = serde_json::to_string(&self.rules)?;
        db.set_setting("notification_rules", &rules_json)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            body: body.into(),
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// Send a desktop notification.
///
/// On macOS this shells out to `osascript` with `display notification`.
/// On other platforms this is a no-op that returns `Ok(())`.
pub fn send_notification(notification: &Notification) -> Result<(), NotifyError> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = build_osascript_command(notification);
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NotifyError::CommandFailed(stderr.to_string()));
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = notification;
    }

    Ok(())
}

/// Build the `osascript` command without executing it.
///
/// Separated from `send_notification` so tests can inspect the
/// generated AppleScript without actually running it.
#[cfg(target_os = "macos")]
fn build_osascript_command(notification: &Notification) -> Command {
    let script = build_applescript(notification);
    let mut cmd = Command::new("osascript");
    cmd.args(["-e", &script]);
    cmd
}

/// Produce the AppleScript string for a `display notification` call.
fn build_applescript(notification: &Notification) -> String {
    let body = escape_for_applescript(&notification.body);
    let title = escape_for_applescript(&notification.title);

    match &notification.subtitle {
        Some(sub) => {
            let subtitle = escape_for_applescript(sub);
            format!(
                "display notification \"{body}\" with title \"{title}\" subtitle \"{subtitle}\""
            )
        }
        None => {
            format!("display notification \"{body}\" with title \"{title}\"")
        }
    }
}

/// Escape characters that are special inside AppleScript double-quoted strings.
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_new_sets_fields() {
        let n = Notification::new("Title", "Body text");
        assert_eq!(n.title, "Title");
        assert_eq!(n.body, "Body text");
        assert!(n.subtitle.is_none());
    }

    #[test]
    fn notification_with_subtitle_builder() {
        let n = Notification::new("T", "B").with_subtitle("Sub");
        assert_eq!(n.subtitle.as_deref(), Some("Sub"));
    }

    #[test]
    fn applescript_basic_format() {
        let n = Notification::new("My Title", "Hello world");
        let script = build_applescript(&n);
        assert_eq!(
            script,
            "display notification \"Hello world\" with title \"My Title\""
        );
    }

    #[test]
    fn applescript_with_subtitle() {
        let n = Notification::new("T", "B").with_subtitle("S");
        let script = build_applescript(&n);
        assert_eq!(
            script,
            "display notification \"B\" with title \"T\" subtitle \"S\""
        );
    }

    #[test]
    fn escape_quotes_and_backslashes() {
        let n = Notification::new("He said \"hi\"", "path: C:\\Users\\test");
        let script = build_applescript(&n);
        assert!(script.contains("He said \\\"hi\\\""));
        assert!(script.contains("C:\\\\Users\\\\test"));
    }

    #[test]
    fn escape_newlines_and_tabs() {
        let n = Notification::new("Title", "line1\nline2\ttab");
        let script = build_applescript(&n);
        assert!(script.contains("line1\\nline2\\ttab"));
        assert!(!script.contains('\n'));
        assert!(!script.contains('\t'));
    }

    #[test]
    fn empty_body_handled() {
        let n = Notification::new("Title", "");
        let script = build_applescript(&n);
        assert_eq!(script, "display notification \"\" with title \"Title\"");
    }

    #[test]
    fn escape_preserves_normal_text() {
        assert_eq!(escape_for_applescript("hello world"), "hello world");
    }

    #[test]
    fn escape_handles_all_special_chars_together() {
        let input = "say \"hi\"\nnew\\line\r\ttab";
        let escaped = escape_for_applescript(input);
        assert_eq!(escaped, "say \\\"hi\\\"\\nnew\\\\line\\r\\ttab");
    }

    #[test]
    fn notify_error_display() {
        let err = NotifyError::CommandFailed("bad script".into());
        assert_eq!(err.to_string(), "osascript failed: bad script");
    }

    #[test]
    fn notify_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err: NotifyError = io_err.into();
        assert!(matches!(err, NotifyError::Io(_)));
        assert!(err.to_string().contains("not found"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_returns_ok() {
        let n = Notification::new("T", "B");
        assert!(send_notification(&n).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_command_is_osascript() {
        let n = Notification::new("Title", "Body");
        let cmd = build_osascript_command(&n);
        assert_eq!(cmd.get_program(), "osascript");

        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args[0], "-e");

        let script = args[1].to_string_lossy();
        assert!(script.starts_with("display notification"));
    }

    // -- NotificationConfig tests --

    #[test]
    fn default_config_notifies_on_needs_input() {
        let config = NotificationConfig::default();
        assert!(config.should_notify(&SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn default_config_does_not_notify_on_done() {
        let config = NotificationConfig::default();
        assert!(!config.should_notify(&SessionState::Working, &SessionState::Done));
    }

    #[test]
    fn disabled_global_blocks_all() {
        let config = NotificationConfig {
            enabled: false,
            ..NotificationConfig::default()
        };
        assert!(!config.should_notify(&SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn disabled_rule_does_not_trigger() {
        let config = NotificationConfig {
            enabled: true,
            rules: vec![NotificationRule {
                from: None,
                to: SessionState::NeedsInput,
                enabled: false,
            }],
        };
        assert!(!config.should_notify(&SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn custom_rule_matches_from() {
        let config = NotificationConfig {
            enabled: true,
            rules: vec![NotificationRule {
                from: Some(SessionState::Working),
                to: SessionState::Done,
                enabled: true,
            }],
        };
        assert!(config.should_notify(&SessionState::Working, &SessionState::Done));
        assert!(!config.should_notify(&SessionState::Idle, &SessionState::Done));
    }

    #[test]
    fn from_none_matches_any_source() {
        let config = NotificationConfig {
            enabled: true,
            rules: vec![NotificationRule {
                from: None,
                to: SessionState::NeedsInput,
                enabled: true,
            }],
        };
        assert!(config.should_notify(&SessionState::Idle, &SessionState::NeedsInput));
        assert!(config.should_notify(&SessionState::Working, &SessionState::NeedsInput));
        assert!(config.should_notify(&SessionState::Done, &SessionState::NeedsInput));
    }

    #[test]
    fn no_rules_means_no_notifications() {
        let config = NotificationConfig {
            enabled: true,
            rules: vec![],
        };
        assert!(!config.should_notify(&SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn notification_rule_serde_roundtrip() {
        let rule = NotificationRule {
            from: Some(SessionState::Working),
            to: SessionState::NeedsInput,
            enabled: true,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: NotificationRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
    }

    #[test]
    fn notification_rule_serde_without_from() {
        let rule = NotificationRule {
            from: None,
            to: SessionState::NeedsInput,
            enabled: true,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: NotificationRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
    }

    #[test]
    fn config_from_settings_uses_defaults_on_fresh_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();

        let config = NotificationConfig::from_settings(&db);
        assert!(config.enabled);
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].to, SessionState::NeedsInput);
    }

    #[test]
    fn config_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();

        let config = NotificationConfig {
            enabled: false,
            rules: vec![
                NotificationRule {
                    from: None,
                    to: SessionState::NeedsInput,
                    enabled: true,
                },
                NotificationRule {
                    from: Some(SessionState::Working),
                    to: SessionState::Done,
                    enabled: false,
                },
            ],
        };

        config.save_to_settings(&db).unwrap();

        let loaded = NotificationConfig::from_settings(&db);
        assert_eq!(loaded.enabled, config.enabled);
        assert_eq!(loaded.rules, config.rules);
    }

    #[test]
    fn config_from_settings_falls_back_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();

        db.set_setting("notification_rules", "not valid json")
            .unwrap();

        let config = NotificationConfig::from_settings(&db);
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].to, SessionState::NeedsInput);
    }
}
