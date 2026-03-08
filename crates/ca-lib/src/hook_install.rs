use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HookInstallError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("home directory not found")]
    NoHomeDir,
}

pub struct HookInstallResult {
    pub settings_path: PathBuf,
    pub hook_types_added: Vec<String>,
    pub already_installed: bool,
}

const HOOK_TYPES: &[&str] = &["PreToolUse", "PostToolUse", "Notification", "Stop"];

pub fn settings_path() -> Result<PathBuf, HookInstallError> {
    let home = dirs::home_dir().ok_or(HookInstallError::NoHomeDir)?;
    Ok(home.join(".claude").join("settings.json"))
}

pub fn read_settings(path: &Path) -> Result<Value, HookInstallError> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let contents = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&contents)?;
    Ok(value)
}

pub fn hook_script_path() -> Result<PathBuf, HookInstallError> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    if let Some(ref dir) = exe_dir {
        let script = dir.join("claude-admin-hook.sh");
        if script.exists() {
            return Ok(script);
        }
    }

    let home = dirs::home_dir().ok_or(HookInstallError::NoHomeDir)?;
    Ok(home.join(".claude-admin").join("claude-admin-hook.sh"))
}

fn make_hook_entry(script_path: &Path) -> Value {
    json!([{
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": script_path.to_string_lossy()
        }]
    }])
}

fn has_our_hook(matcher: &Value) -> bool {
    matcher
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.contains("claude-admin-hook"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn is_hook_installed(settings: &Value, hook_type: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get(hook_type))
        .and_then(|arr| arr.as_array())
        .map(|matchers| matchers.iter().any(has_our_hook))
        .unwrap_or(false)
}

pub fn install_hooks(
    script_path: &Path,
    settings_path: &Path,
) -> Result<HookInstallResult, HookInstallError> {
    let mut settings = read_settings(settings_path)?;

    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }

    let mut hook_types_added = Vec::new();
    let mut all_installed = true;

    for hook_type in HOOK_TYPES {
        if is_hook_installed(&settings, hook_type) {
            continue;
        }
        all_installed = false;

        let entry = make_hook_entry(script_path);

        if let Some(existing) = settings["hooks"].get_mut(hook_type) {
            if let Some(arr) = existing.as_array_mut() {
                if let Some(new_entries) = entry.as_array() {
                    arr.extend(new_entries.iter().cloned());
                }
            }
        } else {
            settings["hooks"][hook_type] = entry;
        }

        hook_types_added.push(hook_type.to_string());
    }

    if !all_installed {
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(&settings)?;
        std::fs::write(settings_path, json_str)?;
    }

    Ok(HookInstallResult {
        settings_path: settings_path.to_path_buf(),
        hook_types_added,
        already_installed: all_installed,
    })
}

pub fn uninstall_hooks(settings_path: &Path) -> Result<bool, HookInstallError> {
    if !settings_path.exists() {
        return Ok(false);
    }

    let mut settings = read_settings(settings_path)?;
    let mut modified = false;

    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for hook_type in HOOK_TYPES {
            if let Some(matchers) = hooks_obj.get_mut(*hook_type) {
                if let Some(arr) = matchers.as_array_mut() {
                    let before = arr.len();
                    arr.retain(|m| !has_our_hook(m));
                    if arr.len() < before {
                        modified = true;
                    }
                }
            }
        }
    }

    if modified {
        let json_str = serde_json::to_string_pretty(&settings)?;
        std::fs::write(settings_path, json_str)?;
    }

    Ok(modified)
}

pub fn hooks_status(settings_path: &Path) -> Result<Vec<(String, bool)>, HookInstallError> {
    let settings = read_settings(settings_path)?;
    let result = HOOK_TYPES
        .iter()
        .map(|hook_type| {
            let installed = is_hook_installed(&settings, hook_type);
            (hook_type.to_string(), installed)
        })
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings_path(dir: &Path) -> PathBuf {
        dir.join(".claude").join("settings.json")
    }

    fn fake_script_path(dir: &Path) -> PathBuf {
        dir.join("claude-admin-hook.sh")
    }

    #[test]
    fn test_read_settings_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = read_settings(&path).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn test_read_settings_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let data = json!({"theme": "dark", "permissions": ["read"]});
        std::fs::write(&path, serde_json::to_string_pretty(&data).unwrap()).unwrap();

        let result = read_settings(&path).unwrap();
        assert_eq!(result["theme"], "dark");
        assert_eq!(result["permissions"][0], "read");
    }

    #[test]
    fn test_install_into_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        let script = fake_script_path(dir.path());

        let result = install_hooks(&script, &settings).unwrap();

        assert!(!result.already_installed);
        assert_eq!(result.hook_types_added.len(), 4);
        assert!(settings.exists());

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        for hook_type in HOOK_TYPES {
            assert!(written["hooks"][hook_type].is_array());
        }
    }

    #[test]
    fn test_install_preserves_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

        let existing = json!({
            "theme": "dark",
            "permissions": {"allow": ["Read"]},
        });
        std::fs::write(&settings, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let script = fake_script_path(dir.path());
        let result = install_hooks(&script, &settings).unwrap();

        assert!(!result.already_installed);
        assert_eq!(result.hook_types_added.len(), 4);

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["permissions"]["allow"][0], "Read");
        assert!(written["hooks"]["PreToolUse"].is_array());
    }

    #[test]
    fn test_install_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        let script = fake_script_path(dir.path());

        let first = install_hooks(&script, &settings).unwrap();
        assert!(!first.already_installed);

        let contents_after_first = std::fs::read_to_string(&settings).unwrap();

        let second = install_hooks(&script, &settings).unwrap();
        assert!(second.already_installed);
        assert!(second.hook_types_added.is_empty());

        let contents_after_second = std::fs::read_to_string(&settings).unwrap();
        assert_eq!(contents_after_first, contents_after_second);
    }

    #[test]
    fn test_install_appends_to_existing_hook_type() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

        let existing = json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Write",
                    "hooks": [{"type": "command", "command": "/usr/bin/other-script.sh"}]
                }]
            }
        });
        std::fs::write(&settings, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let script = fake_script_path(dir.path());
        install_hooks(&script, &settings).unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let pre_tool = written["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 2, "should have original + our entry");
        assert_eq!(pre_tool[0]["matcher"], "Write");
    }

    #[test]
    fn test_uninstall_removes_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        let script = fake_script_path(dir.path());

        install_hooks(&script, &settings).unwrap();
        let removed = uninstall_hooks(&settings).unwrap();
        assert!(removed);

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        for hook_type in HOOK_TYPES {
            let arr = written["hooks"][hook_type].as_array().unwrap();
            assert!(
                arr.is_empty(),
                "{hook_type} should be empty after uninstall"
            );
        }
    }

    #[test]
    fn test_uninstall_when_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{}").unwrap();

        let removed = uninstall_hooks(&settings).unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_uninstall_preserves_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

        let existing = json!({"theme": "dark"});
        std::fs::write(&settings, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let script = fake_script_path(dir.path());
        install_hooks(&script, &settings).unwrap();
        uninstall_hooks(&settings).unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
    }

    #[test]
    fn test_uninstall_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("nope.json");
        let removed = uninstall_hooks(&settings).unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_hooks_status_all_installed() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        let script = fake_script_path(dir.path());

        install_hooks(&script, &settings).unwrap();
        let status = hooks_status(&settings).unwrap();

        assert_eq!(status.len(), 4);
        for (hook_type, installed) in &status {
            assert!(installed, "{hook_type} should be installed");
        }
    }

    #[test]
    fn test_hooks_status_none_installed() {
        let dir = tempfile::tempdir().unwrap();
        let settings = temp_settings_path(dir.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{}").unwrap();

        let status = hooks_status(&settings).unwrap();

        assert_eq!(status.len(), 4);
        for (hook_type, installed) in &status {
            assert!(!installed, "{hook_type} should not be installed");
        }
    }
}
