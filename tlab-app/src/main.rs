use std::path::PathBuf;

use axum::{Router, routing::get};
use serde::Deserialize;
use tlab::{Error, http};
use tokio::sync::watch;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfig {
    tracing: tlab::tracing::Config,
    server: ServerConfig,
}

/// The app owns the set of instances and the shared static file configuration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerConfig {
    http: Option<http::Config>,
    https: Option<http::Config>,
    static_files: Option<http::StaticFilesConfig>,
}

impl ServerConfig {
    fn validate(&self) -> tlab::Result<()> {
        if self.http.is_none() && self.https.is_none() {
            return Err(Error::InvalidInput(
                "no HTTP or HTTPS instance configured".into(),
            ));
        }
        if self
            .http
            .as_ref()
            .is_some_and(|config| config.tls.is_some())
        {
            return Err(Error::InvalidInput(
                "server.http must not contain TLS settings".into(),
            ));
        }
        if self
            .https
            .as_ref()
            .is_some_and(|config| config.tls.is_none())
        {
            return Err(Error::InvalidInput(
                "server.https requires TLS settings".into(),
            ));
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let config_path = std::env::var_os("TLAB_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"));

    let config: AppConfig = tlab::config::load(&config_path, "TLAB_APP")?;
    config.server.validate()?;
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    let mut router = Router::new().route("/health", get(|| async { "ok" }));
    if let Some(files) = &config.server.static_files {
        router = router.merge(files.router()?);
    }

    // Prepare all enabled listeners before serving any requests. On a later
    // failure, previously prepared Server values are dropped and release ports.
    let http = bind(config.server.http.as_ref(), router.clone()).await?;
    let https = bind(config.server.https.as_ref(), router).await?;
    let (stop, signal) = watch::channel(false);
    let running = async {
        tokio::try_join!(serve(http, signal.clone()), serve(https, signal))?;
        Ok(())
    };
    tokio::pin!(running);

    tracing::info!("application started");
    tokio::select! {
        result = &mut running => result,
        result = shutdown_signal() => {
            result?;
            tracing::info!("application stopping");
            stop.send_replace(true);
            running.await
        }
    }
}

async fn bind(config: Option<&http::Config>, router: Router) -> tlab::Result<Option<http::Server>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let server = http::Server::bind(config, router).await?;
    tracing::info!(address = %server.local_addr(), tls = config.tls.is_some(), "listener prepared");
    Ok(Some(server))
}

async fn serve(
    server: Option<http::Server>,
    mut signal: watch::Receiver<bool>,
) -> tlab::Result<()> {
    if let Some(server) = server {
        server
            .serve(async move {
                let _ = signal.wait_for(|stop| *stop).await;
            })
            .await?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_selection_rejects_missing_instances_and_protocol_mismatch() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"https": {"bind": "127.0.0.1:0"}}),
            serde_json::json!({"http": {"bind": "127.0.0.1:0", "tls": {
                "certificate_chain": "chain.pem", "private_key": "key.pem"
            }}}),
        ] {
            let config: ServerConfig = serde_json::from_value(value).unwrap();
            assert!(matches!(config.validate(), Err(Error::InvalidInput(_))));
        }
        for value in [
            serde_json::json!({"http": {"bind": "127.0.0.1:0"}}),
            serde_json::json!({"https": {"bind": "127.0.0.1:0", "tls": {
                "certificate_chain": "chain.pem", "private_key": "key.pem"
            }}}),
        ] {
            let config: ServerConfig = serde_json::from_value(value).unwrap();
            config.validate().unwrap();
        }
    }

    #[test]
    fn checked_in_settings_enable_only_local_http() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
        let config: AppConfig = tlab::config::load(path, "TLAB_APP_SETTINGS_TEST").unwrap();
        config.server.validate().unwrap();
        assert_eq!(
            config.server.http.unwrap().bind,
            "127.0.0.1:8080".parse().unwrap()
        );
        assert!(config.server.https.is_none());
        assert!(config.server.static_files.is_none());
    }
}
