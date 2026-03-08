mod logging;
mod pid;
mod polling;
mod socket;

use ca_lib::config::{Args, Config};
use clap::Parser;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::from_args(args)?;
    config.ensure_data_dir()?;

    let _logging_guard =
        logging::init_logging(config.log_level, &config.log_file, &config.json_log_file)?;

    tracing::info!(pid = std::process::id(), "Daemon starting");

    let pid_file = pid::PidFile::create(&config.pid_file)?;
    let db = Arc::new(Mutex::new(ca_lib::db::Database::open(&config.db_path)?));
    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let socket_server = socket::SocketServer::bind(&config.socket_path, false).await?;

    tracing::info!("Daemon initialized successfully");

    let (update_tx, _) = broadcast::channel::<Vec<ca_lib::models::Session>>(16);

    let polling_handle = tokio::spawn(polling::run_polling_loop(
        Arc::clone(&db),
        shutdown_tx.subscribe(),
        update_tx.clone(),
    ));

    let result = run_server(
        socket_server,
        Arc::clone(&db),
        shutdown_tx.clone(),
        update_tx,
    )
    .await;

    tracing::info!("Daemon shutting down");
    let _ = tokio::time::timeout(Duration::from_secs(10), polling_handle).await;
    drop(pid_file);

    result
}

async fn run_server(
    server: socket::SocketServer,
    db: Arc<Mutex<ca_lib::db::Database>>,
    shutdown_tx: broadcast::Sender<()>,
    update_tx: broadcast::Sender<Vec<ca_lib::models::Session>>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            accept_result = server.accept() => {
                match accept_result {
                    Ok(conn) => {
                        tracing::debug!("New client connection");
                        let db_clone = Arc::clone(&db);
                        let update_tx = update_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = socket::handle_connection(conn, db_clone, update_tx).await {
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
