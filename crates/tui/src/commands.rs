use crate::app::AppAction;
use crate::form::FormKind;

/// Parse a command string into an AppAction.
///
/// Commands:
///   ws add <path> [name]    -> CreateWorkspace
///   ws list                 -> LoadWorkspaces
///   ws del <id>             -> DeleteWorkspace
///   proj new                -> OpenForm(CreateProject)
///   proj del <id>           -> DeleteProject
///   plan del <id>           -> DeletePlan
///   help                    -> ShowHelp
///   q / quit                -> Quit
pub fn parse_command(input: &str) -> Result<AppAction, String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    match parts[0] {
        "ws" | "workspace" => parse_ws_command(&parts[1..]),
        "proj" | "project" => parse_proj_command(&parts[1..]),
        "plan" => parse_plan_command(&parts[1..]),
        "help" | "?" => Ok(AppAction::ShowHelp),
        "q" | "quit" => Ok(AppAction::Quit),
        _ => Err(format!("Unknown command: {}", parts[0])),
    }
}

fn parse_ws_command(args: &[&str]) -> Result<AppAction, String> {
    if args.is_empty() {
        return Err("Usage: ws <add|list|del> [args]".to_string());
    }

    match args[0] {
        "add" | "create" => {
            if args.len() < 2 {
                return Err("Usage: ws add <path> [name]".to_string());
            }
            let path = args[1].to_string();
            let name = if args.len() > 2 {
                Some(args[2..].join(" "))
            } else {
                None
            };
            Ok(AppAction::CreateWorkspace { path, name })
        }
        "list" | "ls" => Ok(AppAction::LoadWorkspaces),
        "del" | "delete" | "rm" => {
            let id = parse_id(args, "ws del")?;
            Ok(AppAction::DeleteWorkspace(id))
        }
        _ => Err(format!("Unknown ws subcommand: {}", args[0])),
    }
}

fn parse_proj_command(args: &[&str]) -> Result<AppAction, String> {
    if args.is_empty() {
        return Err("Usage: proj <new|del> [args]".to_string());
    }

    match args[0] {
        "new" | "create" => {
            // workspace_id=0 is a sentinel; the actual workspace context
            // is resolved from the current selection when the form opens.
            Ok(AppAction::OpenForm(FormKind::CreateProject {
                workspace_id: 0,
            }))
        }
        "del" | "delete" | "rm" => {
            let id = parse_id(args, "proj del")?;
            Ok(AppAction::DeleteProject(id))
        }
        _ => Err(format!("Unknown proj subcommand: {}", args[0])),
    }
}

fn parse_plan_command(args: &[&str]) -> Result<AppAction, String> {
    if args.is_empty() {
        return Err("Usage: plan <del> [args]".to_string());
    }

    match args[0] {
        "del" | "delete" | "rm" => {
            let id = parse_id(args, "plan del")?;
            Ok(AppAction::DeletePlan(id))
        }
        _ => Err(format!("Unknown plan subcommand: {}", args[0])),
    }
}

fn parse_id(args: &[&str], usage_prefix: &str) -> Result<i64, String> {
    if args.len() < 2 {
        return Err(format!("Usage: {usage_prefix} <id>"));
    }
    args[1]
        .parse()
        .map_err(|_| format!("Invalid ID: {}", args[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_add_with_path() {
        let result = parse_command("ws add /home/user/project").unwrap();
        match result {
            AppAction::CreateWorkspace { path, name } => {
                assert_eq!(path, "/home/user/project");
                assert!(name.is_none());
            }
            _ => panic!("expected CreateWorkspace"),
        }
    }

    #[test]
    fn test_ws_add_with_path_and_name() {
        let result = parse_command("ws add /home/user/project my workspace").unwrap();
        match result {
            AppAction::CreateWorkspace { path, name } => {
                assert_eq!(path, "/home/user/project");
                assert_eq!(name.unwrap(), "my workspace");
            }
            _ => panic!("expected CreateWorkspace"),
        }
    }

    #[test]
    fn test_ws_add_missing_path() {
        let result = parse_command("ws add");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage"));
    }

    #[test]
    fn test_ws_list() {
        let result = parse_command("ws list").unwrap();
        assert!(matches!(result, AppAction::LoadWorkspaces));
    }

    #[test]
    fn test_ws_ls_alias() {
        let result = parse_command("ws ls").unwrap();
        assert!(matches!(result, AppAction::LoadWorkspaces));
    }

    #[test]
    fn test_ws_del() {
        let result = parse_command("ws del 42").unwrap();
        match result {
            AppAction::DeleteWorkspace(id) => assert_eq!(id, 42),
            _ => panic!("expected DeleteWorkspace"),
        }
    }

    #[test]
    fn test_ws_rm_alias() {
        let result = parse_command("ws rm 7").unwrap();
        match result {
            AppAction::DeleteWorkspace(id) => assert_eq!(id, 7),
            _ => panic!("expected DeleteWorkspace"),
        }
    }

    #[test]
    fn test_ws_del_invalid_id() {
        let result = parse_command("ws del abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid ID"));
    }

    #[test]
    fn test_ws_del_missing_id() {
        let result = parse_command("ws del");
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_missing_subcommand() {
        let result = parse_command("ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_unknown_subcommand() {
        let result = parse_command("ws foo");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown ws subcommand"));
    }

    #[test]
    fn test_workspace_alias() {
        let result = parse_command("workspace list").unwrap();
        assert!(matches!(result, AppAction::LoadWorkspaces));
    }

    #[test]
    fn test_proj_new() {
        let result = parse_command("proj new").unwrap();
        assert!(matches!(result, AppAction::OpenForm(_)));
    }

    #[test]
    fn test_proj_create_alias() {
        let result = parse_command("project create").unwrap();
        assert!(matches!(result, AppAction::OpenForm(_)));
    }

    #[test]
    fn test_proj_del() {
        let result = parse_command("proj del 5").unwrap();
        match result {
            AppAction::DeleteProject(id) => assert_eq!(id, 5),
            _ => panic!("expected DeleteProject"),
        }
    }

    #[test]
    fn test_proj_del_missing_id() {
        let result = parse_command("proj del");
        assert!(result.is_err());
    }

    #[test]
    fn test_proj_missing_subcommand() {
        let result = parse_command("proj");
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_del() {
        let result = parse_command("plan del 3").unwrap();
        match result {
            AppAction::DeletePlan(id) => assert_eq!(id, 3),
            _ => panic!("expected DeletePlan"),
        }
    }

    #[test]
    fn test_plan_del_missing_id() {
        let result = parse_command("plan del");
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_missing_subcommand() {
        let result = parse_command("plan");
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_unknown_subcommand() {
        let result = parse_command("plan create");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown plan subcommand"));
    }

    #[test]
    fn test_help() {
        let result = parse_command("help").unwrap();
        assert!(matches!(result, AppAction::ShowHelp));
    }

    #[test]
    fn test_question_mark_help() {
        let result = parse_command("?").unwrap();
        assert!(matches!(result, AppAction::ShowHelp));
    }

    #[test]
    fn test_quit() {
        let result = parse_command("q").unwrap();
        assert!(matches!(result, AppAction::Quit));
    }

    #[test]
    fn test_quit_full() {
        let result = parse_command("quit").unwrap();
        assert!(matches!(result, AppAction::Quit));
    }

    #[test]
    fn test_unknown_command() {
        let result = parse_command("foobar");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown"));
    }

    #[test]
    fn test_empty_command() {
        let result = parse_command("");
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_only_command() {
        let result = parse_command("   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_leading_trailing_whitespace() {
        let result = parse_command("  ws list  ").unwrap();
        assert!(matches!(result, AppAction::LoadWorkspaces));
    }
}
