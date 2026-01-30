use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

#[derive(Error, Debug)]
pub enum SocketError {
    #[error("failed to bind socket: {0}")]
    Bind(#[from] std::io::Error),
    #[error("socket already in use by running daemon")]
    InUse,
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Ping,
    Pong,
    Error { message: String },
}

pub struct SocketServer {
    listener: UnixListener,
    path: PathBuf,
}

impl SocketServer {
    pub async fn bind(path: &Path, pid_running: bool) -> Result<Self, SocketError> {
        if path.exists() {
            if pid_running {
                return Err(SocketError::InUse);
            }
            tracing::warn!(path = %path.display(), "Removing stale socket");
            std::fs::remove_file(path)?;
        }

        let listener = UnixListener::bind(path)?;
        tracing::info!(path = %path.display(), "Socket server listening");

        Ok(SocketServer {
            listener,
            path: path.to_owned(),
        })
    }

    pub async fn accept(&self) -> Result<Connection, SocketError> {
        let (stream, _) = self.listener.accept().await?;
        Ok(Connection::new(stream))
    }

    pub fn cleanup(&self) -> Result<(), std::io::Error> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
            tracing::info!(path = %self.path.display(), "Socket removed");
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            tracing::error!(error = %e, "Failed to cleanup socket on drop");
        }
    }
}

pub struct Connection {
    reader: BufReader<tokio::io::ReadHalf<UnixStream>>,
    writer: tokio::io::WriteHalf<UnixStream>,
}

impl Connection {
    fn new(stream: UnixStream) -> Self {
        let (read_half, write_half) = tokio::io::split(stream);
        Connection {
            reader: BufReader::new(read_half),
            writer: write_half,
        }
    }

    pub async fn recv(&mut self) -> Result<Option<Message>, SocketError> {
        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            return Ok(None);
        }

        let msg: Message = serde_json::from_str(line.trim())?;
        Ok(Some(msg))
    }

    pub async fn send(&mut self, msg: &Message) -> Result<(), SocketError> {
        let json = serde_json::to_string(msg)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}

pub async fn handle_connection(mut conn: Connection) -> Result<(), SocketError> {
    while let Some(msg) = conn.recv().await? {
        tracing::debug!(?msg, "Received message");

        let response = match msg {
            Message::Ping => Message::Pong,
            Message::Pong => continue,
            Message::Error { .. } => continue,
        };

        conn.send(&response).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn test_socket_created() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        assert!(socket_path.exists());

        drop(server);
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = SocketServer::bind(&socket_path, false).await.unwrap();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            handle_connection(conn).await.unwrap();
        });

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let ping = serde_json::to_string(&Message::Ping).unwrap();
        write_half.write_all(ping.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let msg: Message = serde_json::from_str(response.trim()).unwrap();

        assert!(matches!(msg, Message::Pong));

        drop(write_half);
        drop(reader);

        tokio::time::timeout(std::time::Duration::from_secs(1), server_task)
            .await
            .expect("server task timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn test_stale_socket_cleanup() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        std::fs::write(&socket_path, "").unwrap();
        assert!(socket_path.exists());

        let server = SocketServer::bind(&socket_path, false).await.unwrap();
        assert!(socket_path.exists());

        drop(server);
    }

    #[tokio::test]
    async fn test_socket_cleanup_on_shutdown() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        {
            let _server = SocketServer::bind(&socket_path, false).await.unwrap();
            assert!(socket_path.exists());
        }

        assert!(!socket_path.exists());
    }
}
