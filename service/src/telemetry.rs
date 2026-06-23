use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rom_operator_bridge_service=info,tower_http=info"));

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(true))
        .try_init();
}
