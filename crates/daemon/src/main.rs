mod config;
mod db;
mod events;
mod logging;
mod models;
mod pid;
mod socket;
mod state;
mod tmux;

use clap::Parser;
use config::{Args, Config};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::from_args(args)?;
    config.ensure_data_dir()?;

    let _logging_guard = logging::init_logging(
        config.log_level,
        &config.log_file,
        &config.json_log_file,
    )?;

    tracing::info!(pid = std::process::id(), "Daemon starting");

    let pid_file = pid::PidFile::create(&config.pid_file)?;
    let _db = Arc::new(db::Database::open(&config.db_path)?);
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);
    let socket_server = socket::SocketServer::bind(&config.socket_path, false).await?;

    tracing::info!("Daemon initialized successfully");

    let result = run_server(socket_server, shutdown_tx.clone()).await;

    tracing::info!("Daemon shutting down");
    drop(pid_file);

    result
}

async fn run_server(
    server: socket::SocketServer,
    shutdown_tx: broadcast::Sender<()>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            accept_result = server.accept() => {
                match accept_result {
                    Ok(conn) => {
                        tracing::debug!("New client connection");
                        tokio::spawn(async move {
                            if let Err(e) = socket::handle_connection(conn).await {
                                tracing::error!(error = %e, "Connection handler error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to accept connection");
                    }
                }
            }

            _ = signal::ctrl_c() => {
                tracing::info!("Received SIGINT, initiating shutdown");
                let _ = shutdown_tx.send(());
                break;
            }

            _ = async {
                #[cfg(unix)]
                {
                    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                        .expect("Failed to register SIGTERM handler");
                    sigterm.recv().await
                }
                #[cfg(not(unix))]
                {
                    std::future::pending::<Option<()>>().await
                }
            } => {
                tracing::info!("Received SIGTERM, initiating shutdown");
                let _ = shutdown_tx.send(());
                break;
            }
        }
    }

    Ok(())
}
