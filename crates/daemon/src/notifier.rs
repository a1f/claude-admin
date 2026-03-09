use std::collections::HashMap;
use std::time::{Duration, Instant};

use ca_lib::models::SessionState;
use ca_lib::notify::{Notification, NotificationConfig, send_notification};

const RATE_LIMIT: Duration = Duration::from_secs(30);

pub struct Notifier {
    config: NotificationConfig,
    last_notified: HashMap<String, (SessionState, Instant)>,
}

impl Notifier {
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            config,
            last_notified: HashMap::new(),
        }
    }

    /// Pure logic check: should we fire a notification for this transition?
    ///
    /// Returns false if the config doesn't match, or dedup suppresses
    /// (same target state already notified), or rate limit applies.
    fn should_fire(&self, session_id: &str, from: &SessionState, to: &SessionState) -> bool {
        if !self.config.should_notify(from, to) {
            return false;
        }

        if let Some((last_state, last_time)) = self.last_notified.get(session_id) {
            if last_state == to {
                return false;
            }
            if last_time.elapsed() < RATE_LIMIT {
                return false;
            }
        }

        true
    }

    pub fn check_and_notify(&mut self, session_id: &str, from: &SessionState, to: &SessionState) {
        if !self.should_fire(session_id, from, to) {
            return;
        }

        let short_id = truncate_session_id(session_id);
        let body = notification_body(to);
        let notification = Notification::new("claude-admin", body).with_subtitle(short_id);

        match send_notification(&notification) {
            Ok(()) => {
                tracing::info!(
                    session_id,
                    from = %from,
                    to = %to,
                    "Notification sent"
                );
            }
            Err(e) => {
                tracing::warn!(
                    session_id,
                    error = %e,
                    "Failed to send notification"
                );
            }
        }

        self.last_notified
            .insert(session_id.to_string(), (*to, Instant::now()));
    }

    pub fn cleanup_stale(&mut self, active_session_ids: &[&str]) {
        self.last_notified
            .retain(|id, _| active_session_ids.contains(&id.as_str()));
    }
}

fn truncate_session_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn notification_body(state: &SessionState) -> &'static str {
    match state {
        SessionState::NeedsInput => "Session needs input",
        SessionState::Done => "Session finished",
        SessionState::Working => "Session started working",
        SessionState::Idle => "Session became idle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_notifier() -> Notifier {
        Notifier::new(NotificationConfig::default())
    }

    #[test]
    fn fires_on_matching_transition() {
        let notifier = default_notifier();
        assert!(notifier.should_fire("s1", &SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn skips_non_matching_transition() {
        let notifier = default_notifier();
        // Default config only triggers on -> NeedsInput
        assert!(!notifier.should_fire("s1", &SessionState::Working, &SessionState::Done));
    }

    #[test]
    fn dedup_prevents_repeat_notification() {
        let mut notifier = default_notifier();
        notifier.last_notified.insert(
            "s1".to_string(),
            (
                SessionState::NeedsInput,
                Instant::now() - Duration::from_secs(60),
            ),
        );
        // Same target state -- should be suppressed even though rate limit has passed
        assert!(!notifier.should_fire("s1", &SessionState::Working, &SessionState::NeedsInput));
    }

    #[test]
    fn rate_limit_suppresses_rapid_notifications() {
        let mut notifier = Notifier::new(NotificationConfig {
            enabled: true,
            rules: vec![
                ca_lib::notify::NotificationRule {
                    from: None,
                    to: SessionState::NeedsInput,
                    enabled: true,
                },
                ca_lib::notify::NotificationRule {
                    from: None,
                    to: SessionState::Done,
                    enabled: true,
                },
            ],
        });
        // Simulate a recent notification for a different state
        notifier
            .last_notified
            .insert("s1".to_string(), (SessionState::NeedsInput, Instant::now()));
        // Different target state but within rate limit window
        assert!(!notifier.should_fire("s1", &SessionState::Working, &SessionState::Done));
    }

    #[test]
    fn allows_after_rate_limit_expires() {
        let mut notifier = Notifier::new(NotificationConfig {
            enabled: true,
            rules: vec![
                ca_lib::notify::NotificationRule {
                    from: None,
                    to: SessionState::NeedsInput,
                    enabled: true,
                },
                ca_lib::notify::NotificationRule {
                    from: None,
                    to: SessionState::Done,
                    enabled: true,
                },
            ],
        });
        // Old notification, rate limit expired
        notifier.last_notified.insert(
            "s1".to_string(),
            (
                SessionState::NeedsInput,
                Instant::now() - Duration::from_secs(60),
            ),
        );
        // Different target state, rate limit passed
        assert!(notifier.should_fire("s1", &SessionState::Working, &SessionState::Done));
    }

    #[test]
    fn cleanup_stale_removes_inactive() {
        let mut notifier = default_notifier();
        notifier
            .last_notified
            .insert("s1".to_string(), (SessionState::NeedsInput, Instant::now()));
        notifier
            .last_notified
            .insert("s2".to_string(), (SessionState::Done, Instant::now()));

        notifier.cleanup_stale(&["s1"]);

        assert!(notifier.last_notified.contains_key("s1"));
        assert!(!notifier.last_notified.contains_key("s2"));
    }

    #[test]
    fn truncate_session_id_short() {
        assert_eq!(truncate_session_id("abc"), "abc");
    }

    #[test]
    fn truncate_session_id_long() {
        assert_eq!(
            truncate_session_id("550e8400-e29b-41d4-a716-446655440000"),
            "550e8400"
        );
    }

    #[test]
    fn notification_body_messages() {
        assert_eq!(
            notification_body(&SessionState::NeedsInput),
            "Session needs input"
        );
        assert_eq!(notification_body(&SessionState::Done), "Session finished");
        assert_eq!(
            notification_body(&SessionState::Working),
            "Session started working"
        );
        assert_eq!(
            notification_body(&SessionState::Idle),
            "Session became idle"
        );
    }
}
