use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use tlab::Error;

use crate::app::Container;

pub async fn run(container: Container) -> tlab::Result<()> {
    let container = Arc::new(container);
    let mut router: Router<Arc<Container>> = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/db_status", get(db_status));
    if let Some(files) = &container.config.server.static_files {
        router = router.merge(files.router()?);
    }
    let router: Router = router.with_state(container.clone());

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

async fn db_status(State(container): State<Arc<Container>>) -> Json<tlab::sqlxdb::PoolStatus> {
    Json(container.database.pool_status())
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
