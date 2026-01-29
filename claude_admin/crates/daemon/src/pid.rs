use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PidError {
    #[error("daemon already running with PID {0}")]
    AlreadyRunning(u32),
    #[error("failed to write PID file: {0}")]
    Write(#[from] std::io::Error),
    #[error("failed to parse PID from file: {0}")]
    Parse(#[from] std::num::ParseIntError),
}

pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn create(path: &Path) -> Result<Self, PidError> {
        if path.exists() {
            if let Ok(existing_pid) = read_pid(path) {
                if is_process_running(existing_pid) {
                    return Err(PidError::AlreadyRunning(existing_pid));
                }
                tracing::warn!(pid = existing_pid, "Removing stale PID file (process not running)");
                let _ = fs::remove_file(path);
            }
        }

        let pid = std::process::id();
        let mut file = fs::File::create(path)?;
        writeln!(file, "{}", pid)?;
        file.sync_all()?;

        tracing::info!(pid = pid, path = %path.display(), "PID file created");

        Ok(PidFile { path: path.to_owned() })
    }

    pub fn remove(&self) -> Result<(), std::io::Error> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
            tracing::info!(path = %self.path.display(), "PID file removed");
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        if let Err(e) = self.remove() {
            tracing::error!(error = %e, "Failed to remove PID file on drop");
        }
    }
}

fn read_pid(path: &Path) -> Result<u32, PidError> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents.trim().parse()?)
}

fn is_process_running(pid: u32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_pid_file_written() {
        let dir = tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        let pid_file = PidFile::create(&pid_path).unwrap();
        assert!(pid_path.exists());

        let content = fs::read_to_string(&pid_path).unwrap();
        let written_pid: u32 = content.trim().parse().unwrap();
        assert_eq!(written_pid, std::process::id());

        drop(pid_file);
    }

    #[test]
    fn test_pid_matches_process() {
        let dir = tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        let _pid_file = PidFile::create(&pid_path).unwrap();

        let content = fs::read_to_string(&pid_path).unwrap();
        let written_pid: u32 = content.trim().parse().unwrap();

        assert_eq!(written_pid, std::process::id());
        assert!(is_process_running(written_pid));
    }

    #[test]
    fn test_duplicate_instance_rejected() {
        let dir = tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        let pid = std::process::id();
        fs::write(&pid_path, format!("{}\n", pid)).unwrap();

        let result = PidFile::create(&pid_path);
        assert!(matches!(result, Err(PidError::AlreadyRunning(_))));
    }

    #[test]
    fn test_stale_pid_file_handled() {
        let dir = tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        fs::write(&pid_path, "999999999\n").unwrap();

        let pid_file = PidFile::create(&pid_path);
        assert!(pid_file.is_ok());
    }

    #[test]
    fn test_pid_cleanup_on_shutdown() {
        let dir = tempdir().unwrap();
        let pid_path = dir.path().join("test.pid");

        {
            let _pid_file = PidFile::create(&pid_path).unwrap();
            assert!(pid_path.exists());
        }

        assert!(!pid_path.exists());
    }
}
