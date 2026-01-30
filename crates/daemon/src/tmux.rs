use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::Command;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TmuxError {
    #[error("tmux not running")]
    NotRunning,
    #[error("pane not found: {0}")]
    PaneNotFound(String),
    #[error("tmux command failed: {0}")]
    CommandFailed(String),
    #[error("failed to parse tmux output: {0}")]
    ParseError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxPane {
    pub session_name: String,
    pub window_index: u32,
    pub pane_index: u32,
    pub pane_id: String,
    pub working_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    ProcessName,
    PaneContent,
}

impl DetectionMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectionMethod::ProcessName => "process_name",
            DetectionMethod::PaneContent => "pane_content",
        }
    }
}

impl fmt::Display for DetectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DetectionMethod {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "process_name" => Ok(DetectionMethod::ProcessName),
            "pane_content" => Ok(DetectionMethod::PaneContent),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaudeLocation {
    pub pane: TmuxPane,
    pub detection_method: DetectionMethod,
    pub detected_at: i64,
}

pub fn is_tmux_running() -> bool {
    Command::new("tmux")
        .args(["list-sessions"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

const PANE_FORMAT: &str = "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_id}\t#{pane_current_path}";

pub fn list_all_panes() -> Result<Vec<TmuxPane>, TmuxError> {
    let output = Command::new("tmux")
        .args(["list-panes", "-a", "-F", PANE_FORMAT])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("no sessions") {
            return Err(TmuxError::NotRunning);
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_pane_list(&stdout)
}

fn parse_pane_list(output: &str) -> Result<Vec<TmuxPane>, TmuxError> {
    let mut panes = Vec::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 5 {
            return Err(TmuxError::ParseError(format!(
                "expected 5 fields, got {}: {:?}",
                parts.len(),
                line
            )));
        }

        let window_index = parts[1].parse::<u32>().map_err(|e| {
            TmuxError::ParseError(format!("invalid window_index '{}': {}", parts[1], e))
        })?;

        let pane_index = parts[2].parse::<u32>().map_err(|e| {
            TmuxError::ParseError(format!("invalid pane_index '{}': {}", parts[2], e))
        })?;

        panes.push(TmuxPane {
            session_name: parts[0].to_string(),
            window_index,
            pane_index,
            pane_id: parts[3].to_string(),
            working_dir: parts[4].to_string(),
        });
    }

    Ok(panes)
}

pub fn get_pane_process(pane_id: &str) -> Result<String, TmuxError> {
    let output = Command::new("tmux")
        .args(["list-panes", "-t", pane_id, "-F", "#{pane_current_command}"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't find pane") || stderr.contains("no such") {
            return Err(TmuxError::PaneNotFound(pane_id.to_string()));
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

pub fn capture_pane_content(pane_id: &str, lines: u32) -> Result<String, TmuxError> {
    if lines == 0 {
        return Ok(String::new());
    }

    let start_line = format!("-{}", lines);
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", pane_id, "-S", &start_line])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't find pane") || stderr.contains("no such") {
            return Err(TmuxError::PaneNotFound(pane_id.to_string()));
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tmux_pane_serialization_roundtrip() {
        let pane = TmuxPane {
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 1,
            pane_id: "%5".to_string(),
            working_dir: "/home/user/project".to_string(),
        };

        let json = serde_json::to_string(&pane).unwrap();
        let deserialized: TmuxPane = serde_json::from_str(&json).unwrap();

        assert_eq!(pane, deserialized);
    }

    #[test]
    fn test_claude_location_serialization_roundtrip() {
        let location = ClaudeLocation {
            pane: TmuxPane {
                session_name: "dev".to_string(),
                window_index: 2,
                pane_index: 0,
                pane_id: "%12".to_string(),
                working_dir: "/tmp".to_string(),
            },
            detection_method: DetectionMethod::ProcessName,
            detected_at: 1706500000,
        };

        let json = serde_json::to_string(&location).unwrap();
        let deserialized: ClaudeLocation = serde_json::from_str(&json).unwrap();

        assert_eq!(location, deserialized);
    }

    #[test]
    fn test_detection_method_from_str() {
        assert_eq!(
            "process_name".parse::<DetectionMethod>(),
            Ok(DetectionMethod::ProcessName)
        );
        assert_eq!(
            "pane_content".parse::<DetectionMethod>(),
            Ok(DetectionMethod::PaneContent)
        );
        assert!("unknown".parse::<DetectionMethod>().is_err());
    }

    #[test]
    fn test_detection_method_display() {
        assert_eq!(DetectionMethod::ProcessName.to_string(), "process_name");
        assert_eq!(DetectionMethod::PaneContent.to_string(), "pane_content");
    }

    #[test]
    fn test_detection_method_serde_matches_display() {
        let process_json = serde_json::to_string(&DetectionMethod::ProcessName).unwrap();
        let content_json = serde_json::to_string(&DetectionMethod::PaneContent).unwrap();

        assert_eq!(process_json, "\"process_name\"");
        assert_eq!(content_json, "\"pane_content\"");

        let process_back: DetectionMethod = serde_json::from_str(&process_json).unwrap();
        let content_back: DetectionMethod = serde_json::from_str(&content_json).unwrap();

        assert_eq!(process_back, DetectionMethod::ProcessName);
        assert_eq!(content_back, DetectionMethod::PaneContent);
    }

    #[test]
    fn test_tmux_error_display() {
        assert_eq!(TmuxError::NotRunning.to_string(), "tmux not running");
        assert_eq!(
            TmuxError::CommandFailed("exit code 1".to_string()).to_string(),
            "tmux command failed: exit code 1"
        );
        assert_eq!(
            TmuxError::ParseError("invalid format".to_string()).to_string(),
            "failed to parse tmux output: invalid format"
        );
    }

    #[test]
    fn test_parse_pane_list_valid() {
        let output = "main\t0\t0\t%0\t/home/user\ndev\t1\t0\t%1\t/tmp\n";
        let panes = parse_pane_list(output).unwrap();

        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].session_name, "main");
        assert_eq!(panes[0].window_index, 0);
        assert_eq!(panes[0].pane_index, 0);
        assert_eq!(panes[0].pane_id, "%0");
        assert_eq!(panes[0].working_dir, "/home/user");

        assert_eq!(panes[1].session_name, "dev");
        assert_eq!(panes[1].window_index, 1);
        assert_eq!(panes[1].pane_index, 0);
        assert_eq!(panes[1].pane_id, "%1");
        assert_eq!(panes[1].working_dir, "/tmp");
    }

    #[test]
    fn test_parse_pane_list_empty() {
        let panes = parse_pane_list("").unwrap();
        assert!(panes.is_empty());
    }

    #[test]
    fn test_parse_pane_list_with_empty_lines() {
        let output = "main\t0\t0\t%0\t/home/user\n\ndev\t1\t0\t%1\t/tmp\n";
        let panes = parse_pane_list(output).unwrap();
        assert_eq!(panes.len(), 2);
    }

    #[test]
    fn test_parse_pane_list_malformed_too_few_fields() {
        let output = "main\t0\t0\t%0\n";
        let result = parse_pane_list(output);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, TmuxError::ParseError(_)));
        assert!(err.to_string().contains("expected 5 fields"));
    }

    #[test]
    fn test_parse_pane_list_malformed_invalid_window_index() {
        let output = "main\tabc\t0\t%0\t/home/user\n";
        let result = parse_pane_list(output);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid window_index"));
    }

    #[test]
    fn test_parse_pane_list_malformed_invalid_pane_index() {
        let output = "main\t0\txyz\t%0\t/home/user\n";
        let result = parse_pane_list(output);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid pane_index"));
    }

    #[test]
    fn test_parse_pane_list_special_chars_in_path() {
        let output = "main\t0\t0\t%0\t/home/user/my project/with spaces\n";
        let panes = parse_pane_list(output).unwrap();
        assert_eq!(panes[0].working_dir, "/home/user/my project/with spaces");
    }

    #[test]
    fn test_parse_pane_list_multiple_windows_and_panes() {
        let output = "sess\t0\t0\t%0\t/a\nsess\t0\t1\t%1\t/b\nsess\t1\t0\t%2\t/c\n";
        let panes = parse_pane_list(output).unwrap();

        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].window_index, 0);
        assert_eq!(panes[0].pane_index, 0);
        assert_eq!(panes[1].window_index, 0);
        assert_eq!(panes[1].pane_index, 1);
        assert_eq!(panes[2].window_index, 1);
        assert_eq!(panes[2].pane_index, 0);
    }

    #[test]
    fn test_parse_pane_list_session_name_with_special_chars() {
        let output = "my:session.name\t0\t0\t%0\t/home/user\n";
        let panes = parse_pane_list(output).unwrap();
        assert_eq!(panes[0].session_name, "my:session.name");
    }

    #[test]
    fn test_pane_not_found_error_display() {
        let err = TmuxError::PaneNotFound("%99".to_string());
        assert_eq!(err.to_string(), "pane not found: %99");
    }

    #[test]
    fn test_capture_pane_content_zero_lines() {
        let result = capture_pane_content("%0", 0).unwrap();
        assert!(result.is_empty());
    }
}
