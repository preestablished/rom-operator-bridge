use rom_operator_bridge_service::{api, config::ServiceConfig, lease_store::LeaseStore, telemetry};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    telemetry::init_tracing();

    let config = ServiceConfig::from_env()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|value| value == "clear-dangling-intents")
    {
        let required = ["--bridge-stopped", "--worker-restarted", "--full-capacity"];
        if required
            .iter()
            .any(|flag| !args.iter().any(|arg| arg == flag))
        {
            return Err("intent acknowledgement requires stopped bridge, restarted worker, and full-capacity confirmations".into());
        }
        let _runtime_lock = config.private_config().acquire_bridge_runtime_lock()?;
        let store = LeaseStore::new(config.private_config().clone());
        let ids = store.dangling_intent_ids()?;
        println!("dangling={} operation_ids={}", ids.len(), ids.join(","));
        let selected: Vec<String> = args
            .into_iter()
            .skip(1)
            .filter(|arg| !arg.starts_with("--"))
            .collect();
        let cleared = store.clear_dangling_intents(&selected)?;
        tracing::info!(
            cleared,
            operation_ids = %selected.join(","),
            "audited dangling lease intents acknowledged after external recovery"
        );
        return Ok(());
    }
    let _runtime_lock = if config.private_config().is_placeholder() {
        None
    } else {
        Some(config.private_config().acquire_bridge_runtime_lock()?)
    };
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
