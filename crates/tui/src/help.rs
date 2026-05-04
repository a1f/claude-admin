/// Returns help content for the dashboard.
/// Each tuple is (key, action_description).
pub fn help_content() -> Vec<(&'static str, &'static str)> {
    vec![
        ("j / Down", "Move down"),
        ("k / Up", "Move up"),
        ("1-9", "Quick select session"),
        ("Enter", "Jump to session tmux pane"),
        ("Ctrl-I", "Toggle untracked filter"),
        ("?", "Toggle help"),
        ("q", "Quit"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_content_not_empty() {
        let content = help_content();
        assert!(!content.is_empty());
        assert!(content.iter().any(|(k, _)| *k == "q"));
        assert!(content.iter().any(|(k, _)| *k == "Enter"));
    }
}
