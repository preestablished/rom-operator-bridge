use rom_operator_bridge_service::{api, config::ServiceConfig, telemetry};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    telemetry::init_tracing();

    let config = ServiceConfig::from_env()?;
    let bind_addr = config.bind_addr();
    let app = api::router(api::AppState::from_config(config));

    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "rom operator bridge service listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "failed to install ctrl-c handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "failed to install terminate handler");
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("rom operator bridge service shutting down");
}
