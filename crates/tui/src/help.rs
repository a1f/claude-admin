use crate::app::ViewMode;

/// Returns help content for a given view mode.
/// Each tuple is (key, action_description, cli_equivalent).
pub fn help_content(view_mode: ViewMode) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut entries = vec![
        (":", "Command palette", ""),
        ("?", "Toggle help", ""),
        ("q / Esc", "Quit / Back", ""),
        ("j / Down", "Move down", ""),
        ("k / Up", "Move up", ""),
    ];

    match view_mode {
        ViewMode::Sessions => {
            entries.extend([
                ("Enter", "Select session (preview)", ""),
                ("1-9", "Quick-switch to session", ""),
                ("Tab / n", "Next needs-input session", ""),
                ("p", "Switch to Projects view", "ca project list"),
                ("i", "Toggle untracked filter", ""),
                ("N", "Create workspace", "ca workspace add <path>"),
            ]);
        }
        ViewMode::Projects => {
            entries.extend([
                ("Enter", "View project plans", "ca plan list <project_id>"),
                ("b", "Back to Sessions", ""),
                ("n", "New project", "ca project create <ws_id> <name>"),
                ("d", "Delete project", "ca project delete <id>"),
                ("N", "Create workspace", "ca workspace add <path>"),
            ]);
        }
        ViewMode::Plans => {
            entries.extend([
                ("Enter", "View plan details", "ca plan show <id>"),
                ("b", "Back to Projects", ""),
                ("n", "New plan", "ca plan create <proj_id> <name>"),
                ("d", "Delete plan", "ca plan delete <id>"),
                ("N", "Create workspace", "ca workspace add <path>"),
            ]);
        }
        ViewMode::PlanDetail => {
            entries.extend([
                (
                    "s",
                    "Cycle step status",
                    "ca plan step <id> <step> <status>",
                ),
                ("o", "Open orchestrator", ""),
                ("b", "Back to Plans", ""),
            ]);
        }
        ViewMode::Orchestrator => {
            entries.extend([
                ("Tab", "Switch panel (Steps/Sessions)", ""),
                ("s", "Spawn step session", "ca spawn <plan_id> --step <id>"),
                ("a", "Attach to session", "tmux select-pane -t <id>"),
                ("b", "Back to Plan Detail", ""),
            ]);
        }
        ViewMode::Review => {
            entries.extend([
                ("j / k", "Scroll diff up/down", ""),
                ("n / p", "Next/previous hunk", ""),
                ("h / l", "Previous/next file", ""),
                ("c", "Add comment at line", ""),
                ("b", "Back", ""),
            ]);
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_content_sessions_not_empty() {
        let content = help_content(ViewMode::Sessions);
        assert!(!content.is_empty());
        assert!(content.iter().any(|(k, _, _)| *k == ":"));
        assert!(content.iter().any(|(k, _, _)| *k == "?"));
    }

    #[test]
    fn test_help_content_projects_not_empty() {
        let content = help_content(ViewMode::Projects);
        assert!(!content.is_empty());
        assert!(
            content
                .iter()
                .any(|(_, action, _)| *action == "New project")
        );
    }

    #[test]
    fn test_help_content_plans_not_empty() {
        let content = help_content(ViewMode::Plans);
        assert!(!content.is_empty());
        assert!(content.iter().any(|(_, action, _)| *action == "New plan"));
    }

    #[test]
    fn test_help_content_plan_detail_not_empty() {
        let content = help_content(ViewMode::PlanDetail);
        assert!(!content.is_empty());
        assert!(content.iter().any(|(k, _, _)| *k == "s"));
    }

    #[test]
    fn test_help_content_orchestrator_not_empty() {
        let content = help_content(ViewMode::Orchestrator);
        assert!(!content.is_empty());
        assert!(content.iter().any(|(k, _, _)| *k == "Tab"));
    }

    #[test]
    fn test_all_views_have_global_keys() {
        for mode in [
            ViewMode::Sessions,
            ViewMode::Projects,
            ViewMode::Plans,
            ViewMode::PlanDetail,
            ViewMode::Orchestrator,
            ViewMode::Review,
        ] {
            let content = help_content(mode);
            assert!(
                content.iter().any(|(k, _, _)| *k == ":"),
                "Missing : key in {:?}",
                mode
            );
            assert!(
                content.iter().any(|(k, _, _)| *k == "?"),
                "Missing ? key in {:?}",
                mode
            );
        }
    }
}
