use crate::db::{Database, DbError};
use crate::tmux::{TmuxError, TmuxPane};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHost {
    pub id: i64,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub key_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl fmt::Display for RemoteHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}:{}", self.user, self.hostname, self.port)
    }
}

fn row_to_remote_host(row: &rusqlite::Row) -> rusqlite::Result<RemoteHost> {
    let port: i32 = row.get(3)?;
    Ok(RemoteHost {
        id: row.get(0)?,
        hostname: row.get(1)?,
        user: row.get(2)?,
        port: port as u16,
        key_path: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl Database {
    pub fn register_remote_host(
        &self,
        hostname: &str,
        user: &str,
        port: u16,
        key_path: Option<&str>,
    ) -> Result<RemoteHost, DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            INSERT INTO remote_hosts (hostname, user, port, key_path, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![hostname, user, port as i32, key_path, now, now],
        )?;

        let id = self.connection().last_insert_rowid();

        Ok(RemoteHost {
            id,
            hostname: hostname.to_string(),
            user: user.to_string(),
            port,
            key_path: key_path.map(String::from),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_remote_host(&self, id: i64) -> Result<Option<RemoteHost>, DbError> {
        let host = self
            .connection()
            .query_row(
                r#"
                SELECT id, hostname, user, port, key_path, created_at, updated_at
                FROM remote_hosts WHERE id = ?1
                "#,
                params![id],
                row_to_remote_host,
            )
            .optional()?;

        Ok(host)
    }

    pub fn list_remote_hosts(&self) -> Result<Vec<RemoteHost>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT id, hostname, user, port, key_path, created_at, updated_at
            FROM remote_hosts
            ORDER BY created_at DESC, id DESC
            "#,
        )?;

        let rows = stmt.query_map([], row_to_remote_host)?;

        let mut hosts = Vec::new();
        for row_result in rows {
            hosts.push(row_result?);
        }
        Ok(hosts)
    }

    pub fn delete_remote_host(&self, id: i64) -> Result<(), DbError> {
        self.connection()
            .execute("DELETE FROM remote_hosts WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_remote_host(
        &self,
        id: i64,
        hostname: &str,
        user: &str,
        port: u16,
        key_path: Option<&str>,
    ) -> Result<(), DbError> {
        let now = now_unix();

        self.connection().execute(
            r#"
            UPDATE remote_hosts SET
                hostname = ?2,
                user = ?3,
                port = ?4,
                key_path = ?5,
                updated_at = ?6
            WHERE id = ?1
            "#,
            params![id, hostname, user, port as i32, key_path, now],
        )?;
        Ok(())
    }
}

pub fn test_ssh_connection(host: &RemoteHost) -> Result<bool, std::io::Error> {
    let mut cmd = std::process::Command::new("ssh");
    cmd.args(["-o", "ConnectTimeout=5", "-o", "BatchMode=yes"]);
    if let Some(key) = &host.key_path {
        cmd.args(["-i", key]);
    }
    cmd.args(["-p", &host.port.to_string()]);
    cmd.arg(format!("{}@{}", host.user, host.hostname));
    cmd.args(["echo", "ok"]);

    let output = cmd.output()?;
    Ok(output.status.success())
}

// --- SSH-based tmux operations ---

/// Build SSH argument list for a remote host.
/// Separated from Command construction to make unit testing straightforward.
pub(crate) fn ssh_args(host: &RemoteHost) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
    ];
    if let Some(key) = &host.key_path {
        args.push("-i".to_string());
        args.push(key.clone());
    }
    args.push("-p".to_string());
    args.push(host.port.to_string());
    args.push(format!("{}@{}", host.user, host.hostname));
    args
}

fn build_ssh_command(host: &RemoteHost) -> Command {
    let mut cmd = Command::new("ssh");
    for arg in ssh_args(host) {
        cmd.arg(arg);
    }
    cmd
}

/// List all tmux panes on a remote host.
pub fn list_remote_panes(host: &RemoteHost) -> Result<Vec<TmuxPane>, TmuxError> {
    let pane_format =
        "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_id}\t#{pane_current_path}";
    let mut cmd = build_ssh_command(host);
    cmd.args(["tmux", "list-panes", "-a", "-F", pane_format]);

    let output = cmd.output().map_err(TmuxError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("no sessions") {
            return Err(TmuxError::NotRunning);
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    crate::tmux::parse_pane_list(&stdout)
}

/// Capture pane content from a remote tmux pane.
pub fn capture_remote_pane(
    host: &RemoteHost,
    pane_id: &str,
    lines: u32,
) -> Result<String, TmuxError> {
    if lines == 0 {
        return Ok(String::new());
    }

    let start_line = format!("-{}", lines);
    let mut cmd = build_ssh_command(host);
    cmd.args([
        "tmux",
        "capture-pane",
        "-p",
        "-t",
        pane_id,
        "-S",
        &start_line,
    ]);

    let output = cmd.output().map_err(TmuxError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't find pane") || stderr.contains("no such") {
            return Err(TmuxError::PaneNotFound(pane_id.to_string()));
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Send keys to a remote tmux pane.
pub fn send_remote_keys(host: &RemoteHost, pane_id: &str, keys: &str) -> Result<(), TmuxError> {
    let mut cmd = build_ssh_command(host);
    cmd.args(["tmux", "send-keys", "-t", pane_id, keys, "Enter"]);

    let output = cmd.output().map_err(TmuxError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    Ok(())
}

/// Get the current process running in a remote tmux pane.
pub fn get_remote_pane_process(host: &RemoteHost, pane_id: &str) -> Result<String, TmuxError> {
    let mut cmd = build_ssh_command(host);
    cmd.args([
        "tmux",
        "list-panes",
        "-t",
        pane_id,
        "-F",
        "#{pane_current_command}",
    ]);

    let output = cmd.output().map_err(TmuxError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't find pane") || stderr.contains("no such") {
            return Err(TmuxError::PaneNotFound(pane_id.to_string()));
        }
        return Err(TmuxError::CommandFailed(stderr.into_owned()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::tempdir;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_register_remote_host() {
        let (db, _dir) = create_test_db();

        let host = db
            .register_remote_host("server1.example.com", "deploy", 22, None)
            .unwrap();

        assert!(host.id > 0);
        assert_eq!(host.hostname, "server1.example.com");
        assert_eq!(host.user, "deploy");
        assert_eq!(host.port, 22);
        assert!(host.key_path.is_none());
        assert!(host.created_at > 0);
        assert_eq!(host.created_at, host.updated_at);
    }

    #[test]
    fn test_get_remote_host() {
        let (db, _dir) = create_test_db();

        let created = db
            .register_remote_host(
                "db.example.com",
                "admin",
                2222,
                Some("/home/user/.ssh/id_rsa"),
            )
            .unwrap();

        let fetched = db.get_remote_host(created.id).unwrap().unwrap();

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.hostname, "db.example.com");
        assert_eq!(fetched.user, "admin");
        assert_eq!(fetched.port, 2222);
        assert_eq!(fetched.key_path, Some("/home/user/.ssh/id_rsa".to_string()));
        assert_eq!(fetched.created_at, created.created_at);
        assert_eq!(fetched.updated_at, created.updated_at);
    }

    #[test]
    fn test_get_remote_host_not_found() {
        let (db, _dir) = create_test_db();

        let result = db.get_remote_host(9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_remote_hosts() {
        let (db, _dir) = create_test_db();

        let h1 = db
            .register_remote_host("alpha.example.com", "user", 22, None)
            .unwrap();
        let h2 = db
            .register_remote_host("beta.example.com", "user", 22, None)
            .unwrap();

        let hosts = db.list_remote_hosts().unwrap();

        assert_eq!(hosts.len(), 2);
        // Ordered by created_at DESC, id DESC
        assert_eq!(hosts[0].id, h2.id);
        assert_eq!(hosts[1].id, h1.id);
    }

    #[test]
    fn test_delete_remote_host() {
        let (db, _dir) = create_test_db();

        let host = db
            .register_remote_host("doomed.example.com", "user", 22, None)
            .unwrap();

        db.delete_remote_host(host.id).unwrap();

        let result = db.get_remote_host(host.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_remote_host() {
        let (db, _dir) = create_test_db();

        let host = db
            .register_remote_host("old.example.com", "olduser", 22, None)
            .unwrap();

        db.update_remote_host(
            host.id,
            "new.example.com",
            "newuser",
            3022,
            Some("/keys/id_ed25519"),
        )
        .unwrap();

        let fetched = db.get_remote_host(host.id).unwrap().unwrap();
        assert_eq!(fetched.hostname, "new.example.com");
        assert_eq!(fetched.user, "newuser");
        assert_eq!(fetched.port, 3022);
        assert_eq!(fetched.key_path, Some("/keys/id_ed25519".to_string()));
        assert!(fetched.updated_at >= host.updated_at);
    }

    #[test]
    fn test_duplicate_host_user_fails() {
        let (db, _dir) = create_test_db();

        db.register_remote_host("same.example.com", "sameuser", 22, None)
            .unwrap();

        let result = db.register_remote_host("same.example.com", "sameuser", 2222, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_workspace_host_association() {
        let (db, _dir) = create_test_db();

        let ws = db
            .create_workspace("/home/user/project", Some("project"))
            .unwrap();
        let host = db
            .register_remote_host("remote.example.com", "deploy", 22, None)
            .unwrap();

        assert!(ws.host_id.is_none());

        db.set_workspace_host(ws.id, Some(host.id)).unwrap();

        let fetched = db.get_workspace(ws.id).unwrap().unwrap();
        assert_eq!(fetched.host_id, Some(host.id));

        // Clear the association
        db.set_workspace_host(ws.id, None).unwrap();
        let cleared = db.get_workspace(ws.id).unwrap().unwrap();
        assert!(cleared.host_id.is_none());
    }

    fn make_host(hostname: &str, user: &str, port: u16, key_path: Option<&str>) -> RemoteHost {
        RemoteHost {
            id: 1,
            hostname: hostname.to_string(),
            user: user.to_string(),
            port,
            key_path: key_path.map(String::from),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_ssh_args_basic() {
        let host = make_host("server.example.com", "deploy", 22, None);
        let args = ssh_args(&host);

        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert!(args.contains(&"deploy@server.example.com".to_string()));
    }

    #[test]
    fn test_ssh_args_with_key() {
        let host = make_host("s.example.com", "u", 22, Some("/keys/id_ed25519"));
        let args = ssh_args(&host);

        let key_idx = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[key_idx + 1], "/keys/id_ed25519");
    }

    #[test]
    fn test_ssh_args_custom_port() {
        let host = make_host("s.example.com", "u", 2222, None);
        let args = ssh_args(&host);

        let port_idx = args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(args[port_idx + 1], "2222");
    }

    #[test]
    fn test_ssh_args_default_port() {
        let host = make_host("s.example.com", "u", 22, None);
        let args = ssh_args(&host);

        let port_idx = args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(args[port_idx + 1], "22");
    }

    #[test]
    fn test_ssh_timeout_options() {
        let host = make_host("s.example.com", "u", 22, None);
        let args = ssh_args(&host);

        assert!(args.contains(&"ConnectTimeout=10".to_string()));
    }

    #[test]
    fn test_ssh_strict_host_key_checking() {
        let host = make_host("s.example.com", "u", 22, None);
        let args = ssh_args(&host);

        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_string()));
    }

    #[test]
    fn test_capture_remote_pane_zero_lines() {
        let host = make_host("s.example.com", "u", 22, None);
        let result = capture_remote_pane(&host, "%0", 0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_remote_host_display_format() {
        let host = make_host("server.example.com", "deploy", 22, None);
        assert_eq!(host.to_string(), "deploy@server.example.com:22");
    }

    #[test]
    fn test_remote_host_display_custom_port() {
        let host = make_host("db.internal", "admin", 2222, None);
        assert_eq!(host.to_string(), "admin@db.internal:2222");
    }

    #[test]
    fn test_parse_pane_list_reuse_from_remote_context() {
        let output = "work\t0\t0\t%10\t/remote/project\nwork\t1\t0\t%11\t/remote/other\n";
        let panes = crate::tmux::parse_pane_list(output).unwrap();

        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].session_name, "work");
        assert_eq!(panes[0].pane_id, "%10");
        assert_eq!(panes[0].working_dir, "/remote/project");
        assert_eq!(panes[1].pane_id, "%11");
    }

    #[test]
    fn test_ssh_args_no_key_omits_i_flag() {
        let host = make_host("s.example.com", "u", 22, None);
        let args = ssh_args(&host);

        assert!(!args.contains(&"-i".to_string()));
    }
}
