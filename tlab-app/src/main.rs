mod app;

use std::path::PathBuf;

use axum::{Router, routing::get};
use tlab::Error;

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let config_path = std::env::var_os("TLAB_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"));

    let config: app::Config = tlab::config::load(&config_path, "TLAB_APP")?;
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    let container = app::Container::new(config).await?;

    let mut router = Router::new().route("/health", get(|| async { "ok" }));
    if let Some(files) = &container.config.server.static_files {
        router = router.merge(files.router()?);
    }

    tracing::info!("application starting");
    tokio::try_join!(
        container.http.run(router.clone()),
        container.https.run(router),
        async {
            shutdown_signal().await?;
            tracing::info!("application stopping");
            for server in [&container.http, &container.https] {
                server.graceful_shutdown();
            }
            Ok::<(), Error>(())
        }
    )?;
    Ok(())
}

async fn shutdown_signal() -> tlab::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;

    Ok(())
}
