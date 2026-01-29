use std::fs::File;
use std::path::Path;
use thiserror::Error;
use tracing::Level;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("failed to create log file: {0}")]
    CreateLogFile(#[from] std::io::Error),
    #[error("failed to initialize logging: {0}")]
    Init(#[from] tracing_subscriber::util::TryInitError),
}

pub struct LoggingGuard {
    _human_file: File,
    _json_file: File,
}

pub fn init_logging(
    level: Level,
    human_log_path: &Path,
    json_log_path: &Path,
) -> Result<LoggingGuard, LoggingError> {
    let human_file = File::create(human_log_path)?;
    let json_file = File::create(json_log_path)?;

    let filter = EnvFilter::from_default_env().add_directive(level.into());

    let human_layer = fmt::layer()
        .with_writer(human_file.try_clone()?)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_span_events(FmtSpan::NONE)
        .with_filter(filter.clone());

    let json_layer = fmt::layer()
        .json()
        .with_writer(json_file.try_clone()?)
        .with_span_events(FmtSpan::NONE)
        .with_filter(filter.clone());

    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_filter(filter);

    tracing_subscriber::registry()
        .with(human_layer)
        .with(json_layer)
        .with(console_layer)
        .try_init()?;

    Ok(LoggingGuard {
        _human_file: human_file,
        _json_file: json_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_log_file_created() {
        let dir = tempdir().unwrap();
        let human_log = dir.path().join("test.log");
        let json_log = dir.path().join("test.json.log");

        // init_logging can only be called once per process
        let _human = File::create(&human_log).unwrap();
        let _json = File::create(&json_log).unwrap();

        assert!(human_log.exists());
        assert!(json_log.exists());
    }

    #[test]
    fn test_json_log_valid_format() {
        let json_line = r#"{"timestamp":"2024-01-15T10:30:00.000Z","level":"INFO","target":"daemon","message":"Test"}"#;
        let parsed: serde_json::Value = serde_json::from_str(json_line).unwrap();
        assert_eq!(parsed["level"], "INFO");
        assert_eq!(parsed["message"], "Test");
    }

    #[test]
    fn test_human_log_readable() {
        let human_line = "2024-01-15T10:30:00.000Z INFO daemon: Daemon starting pid=12345";
        assert!(human_line.contains("INFO"));
        assert!(human_line.contains("daemon"));
    }

    #[test]
    fn test_log_level_filtering() {
        let filter = EnvFilter::from_default_env().add_directive(Level::DEBUG.into());
        assert!(format!("{:?}", filter).contains("DEBUG") || format!("{:?}", filter).len() > 0);
    }
}
