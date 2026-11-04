use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfig {
    tracing: tlab::tracing::Config,
}

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let config_path = std::env::var_os("TLAB_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"));

    let config: AppConfig = tlab::config::load(&config_path, "TLAB_APP")?;
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    tracing::info!("application started");

    Ok(())
}
