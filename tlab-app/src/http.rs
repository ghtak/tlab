use std::sync::Arc;

use axum::{Router, extract::State, http::StatusCode, response::Response, routing::get};
use tlab::Error;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::app::{Container, response};

#[cfg(test)]
mod tests;

pub async fn run(container: Container) -> tlab::Result<()> {
    let container = Arc::new(container);
    let mut router: Router<Arc<Container>> = Router::new()
        .merge(crate::auth::router())
        .route("/health", get(|| async { "ok" }))
        .route("/db_status", get(db_status));
    if let Some(files) = &container.config.server.static_files {
        router = router.merge(files.router()?);
    }
    let router: Router = trace_requests(router).with_state(container.clone());

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

pub(crate) fn trace_requests<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(tlab::http::trace::new_request_span)
            .on_request(DefaultOnRequest::new().level(Level::INFO))
            .on_response(DefaultOnResponse::new().level(Level::INFO))
            // Routes own diagnostic logs; response status alone must not duplicate them.
            .on_failure(()),
    )
}

async fn db_status(State(container): State<Arc<Container>>) -> Response {
    response::success(StatusCode::OK, container.database.pool_status())
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
