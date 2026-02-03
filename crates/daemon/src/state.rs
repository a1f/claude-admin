use crate::models::SessionState;

/// Priority: Done (last 3 lines) > NeedsInput (prompt at end) > Working > Idle
pub fn detect_state(content: &str) -> SessionState {
    let recent_lines: Vec<&str> = content.lines().rev().take(20).collect();
    let recent_content = recent_lines.iter().rev().copied().collect::<Vec<_>>().join("\n");

    let last_few_lines: Vec<&str> = content.lines().rev().take(3).collect();
    let tail_content = last_few_lines.iter().rev().copied().collect::<Vec<_>>().join("\n");

    if is_done(&tail_content) {
        return SessionState::Done;
    }

    if is_needs_input(&tail_content, content) {
        return SessionState::NeedsInput;
    }

    if is_working(&recent_content) {
        return SessionState::Working;
    }

    SessionState::Idle
}

fn is_done(content: &str) -> bool {
    let done_patterns = ["Session ended", "Goodbye", "exited with code", "connection closed"];
    done_patterns.iter().any(|p| content.contains(p))
}

fn is_working(content: &str) -> bool {
    let working_patterns = [
        "Tool:",
        "Reading",
        "Writing",
        "Searching",
        "Running",
        "Analyzing",
        "Thinking",
        "Processing",
    ];
    working_patterns.iter().any(|p| content.contains(p))
}

fn is_needs_input(recent_content: &str, full_content: &str) -> bool {
    let last_line = full_content
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");

    let trimmed = last_line.trim();

    let input_patterns = [
        "Approve?",
        "Continue?",
        "Proceed?",
        "(y/n)",
        "[Y/n]",
        "[y/N]",
        "Enter to continue",
        "Press Enter",
    ];

    if input_patterns.iter().any(|p| recent_content.contains(p)) {
        return true;
    }

    if trimmed.ends_with('>')
        || trimmed.ends_with('?')
        || trimmed.ends_with(':')
        || trimmed.ends_with("$ ")
    {
        if !is_working(recent_content) {
            return true;
        }
    }

    // Only check welcome prompts if there's no working activity in recent output
    if !is_working(recent_content)
        && (full_content.contains("What would you like to do?")
            || full_content.contains("How can I help"))
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_done_session_ended() {
        let content = "Some previous output\nWorking on task...\nSession ended";
        assert_eq!(detect_state(content), SessionState::Done);
    }

    #[test]
    fn test_detect_done_goodbye() {
        let content = "Task completed successfully.\nGoodbye";
        assert_eq!(detect_state(content), SessionState::Done);
    }

    #[test]
    fn test_detect_done_exit_code() {
        let content = "Process exited with code 0";
        assert_eq!(detect_state(content), SessionState::Done);
    }

    #[test]
    fn test_detect_working_tool_call() {
        let content = "I'll read that file for you.\nTool: Read\nReading /src/main.rs...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_working_writing() {
        let content = "Writing changes to file...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_working_searching() {
        let content = "Searching for pattern in codebase...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_working_claude_ui_box() {
        let content = "╭─ Analysis ─────────────────────────────────────────╮\n│ Analyzing the codebase structure...                │\n├─ Files found: 42                                   │";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_working_running_command() {
        let content = "Running cargo build...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_working_thinking() {
        let content = "Thinking...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_detect_needs_input_approve() {
        let content = "I'll make the following changes:\n- Update config.rs\n- Add new module\n\nApprove?";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_needs_input_continue_prompt() {
        let content = "This will modify 5 files.\nContinue? (y/n)";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_needs_input_prompt_character() {
        let content = "Welcome to Claude!\n>";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_needs_input_question() {
        let content = "What would you like to do?";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_needs_input_help_prompt() {
        let content = "How can I help you today?";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_needs_input_yn_prompt() {
        let content = "Do you want to proceed? [Y/n]";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_detect_idle_empty() {
        assert_eq!(detect_state(""), SessionState::Idle);
    }

    #[test]
    fn test_detect_idle_whitespace() {
        assert_eq!(detect_state("   \n\n   \n"), SessionState::Idle);
    }

    #[test]
    fn test_detect_idle_generic_output() {
        let content = "Some random text that doesn't match any pattern.\nJust ordinary output here.\nNothing special.";
        assert_eq!(detect_state(content), SessionState::Idle);
    }

    #[test]
    fn test_done_takes_priority_over_working() {
        let content = "Tool: Read\nReading file...\nSession ended";
        assert_eq!(detect_state(content), SessionState::Done);
    }

    #[test]
    fn test_working_takes_priority_over_needs_input() {
        let content = "What would you like to do?\n> implement feature X\nTool: Read\nReading requirements...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_prompt_in_code_not_needs_input() {
        let content = "Tool: Read\nfn compare(a: i32, b: i32) -> bool {\n    a > b\n}";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_colon_in_output_not_needs_input_when_working() {
        let content = "Tool: Read\nReading: /path/to/file";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_long_content_uses_recent_lines() {
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!("Old line {}\n", i));
        }
        content.push_str("Tool: Read\nReading file...\n");
        assert_eq!(detect_state(&content), SessionState::Working);
    }

    #[test]
    fn test_old_done_message_ignored() {
        let content = "Session ended\n--- new session ---\nWelcome!\nTool: Read\nReading config.rs...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_claude_welcome_screen() {
        let content = "╭──────────────────────────────────────────────────────────╮\n│                                                          │\n│   Welcome to Claude Code!                                │\n│                                                          │\n│   What would you like to do?                             │\n│                                                          │\n╰──────────────────────────────────────────────────────────╯\n>";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }

    #[test]
    fn test_mid_tool_execution() {
        let content = "I'll search for that pattern.\n\n╭─ Grep ──────────────────────────────────────────────────╮\n│ Searching for \"SessionState\" in src/                    │\n│ ...";
        assert_eq!(detect_state(content), SessionState::Working);
    }

    #[test]
    fn test_completed_task_awaiting_input() {
        let content = "I've completed the changes. Here's what I did:\n\n1. Updated config.rs\n2. Added new tests\n3. Fixed the bug\n\nIs there anything else you'd like me to help with?";
        assert_eq!(detect_state(content), SessionState::NeedsInput);
    }
}
