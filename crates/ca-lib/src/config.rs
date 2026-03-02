use clap::Parser;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to determine home directory")]
    NoHomeDir,
    #[error("failed to create directory: {0}")]
    CreateDir(#[from] std::io::Error),
    #[error("invalid log level: {0}")]
    InvalidLogLevel(String),
}

#[derive(Parser, Debug)]
#[command(name = "daemon", about = "Claude Admin daemon process")]
pub struct Args {
    #[arg(long, default_value = "info")]
    pub log_level: String,

    #[arg(long)]
    pub log_file: Option<PathBuf>,

    #[arg(long)]
    pub socket_path: Option<PathBuf>,

    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub log_level: tracing::Level,
    pub log_file: PathBuf,
    pub json_log_file: PathBuf,
    pub socket_path: PathBuf,
    pub pid_file: PathBuf,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn from_args(args: Args) -> Result<Self, ConfigError> {
        let data_dir = get_data_dir()?;
        let log_level = parse_log_level(&args.log_level)?;

        let log_file = args.log_file.unwrap_or_else(|| data_dir.join("daemon.log"));
        let json_log_file = log_file.with_extension("json.log");
        let socket_path = args.socket_path.unwrap_or_else(|| data_dir.join("daemon.sock"));
        let pid_file = args.pid_file.unwrap_or_else(|| data_dir.join("daemon.pid"));
        let db_path = args.db_path.unwrap_or_else(|| data_dir.join("sessions.db"));

        Ok(Config {
            log_level,
            log_file,
            json_log_file,
            socket_path,
            pid_file,
            db_path,
            data_dir,
        })
    }

    pub fn ensure_data_dir(&self) -> Result<(), ConfigError> {
        std::fs::create_dir_all(&self.data_dir)?;
        Ok(())
    }
}

fn get_data_dir() -> Result<PathBuf, ConfigError> {
    dirs::home_dir()
        .map(|h| h.join(".claude-admin"))
        .ok_or(ConfigError::NoHomeDir)
}

fn parse_log_level(s: &str) -> Result<tracing::Level, ConfigError> {
    match s.to_lowercase().as_str() {
        "trace" => Ok(tracing::Level::TRACE),
        "debug" => Ok(tracing::Level::DEBUG),
        "info" => Ok(tracing::Level::INFO),
        "warn" => Ok(tracing::Level::WARN),
        "error" => Ok(tracing::Level::ERROR),
        _ => Err(ConfigError::InvalidLogLevel(s.to_string())),
    }
}

#[allow(dead_code)]
pub fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_paths() {
        let args = Args {
            log_level: "info".to_string(),
            log_file: None,
            socket_path: None,
            pid_file: None,
            db_path: None,
        };

        let config = Config::from_args(args).unwrap();
        let home = dirs::home_dir().unwrap();
        let data_dir = home.join(".claude-admin");

        assert_eq!(config.data_dir, data_dir);
        assert_eq!(config.log_file, data_dir.join("daemon.log"));
        assert_eq!(config.socket_path, data_dir.join("daemon.sock"));
        assert_eq!(config.pid_file, data_dir.join("daemon.pid"));
        assert_eq!(config.db_path, data_dir.join("sessions.db"));
    }

    #[test]
    fn test_tilde_expansion() {
        let expanded = expand_tilde("~/test/path");
        let home = dirs::home_dir().unwrap();
        assert_eq!(expanded, home.join("test/path"));

        let unchanged = expand_tilde("/absolute/path");
        assert_eq!(unchanged, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_log_level_parsing() {
        assert_eq!(parse_log_level("trace").unwrap(), tracing::Level::TRACE);
        assert_eq!(parse_log_level("DEBUG").unwrap(), tracing::Level::DEBUG);
        assert_eq!(parse_log_level("Info").unwrap(), tracing::Level::INFO);
        assert_eq!(parse_log_level("WARN").unwrap(), tracing::Level::WARN);
        assert_eq!(parse_log_level("error").unwrap(), tracing::Level::ERROR);

        assert!(parse_log_level("invalid").is_err());
    }
}
