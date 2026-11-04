use std::path::PathBuf;

use axum::{Router, routing::get};
use serde::Deserialize;
use tlab::{Error, http};

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

    // Prepare TLS for all enabled instances before opening listeners.
    let http = prepare(config.server.http.as_ref()).await?;
    let https = prepare(config.server.https.as_ref()).await?;
    tracing::info!("application starting");
    tokio::try_join!(
        run(http.as_ref(), router.clone()),
        run(https.as_ref(), router),
        async {
            shutdown_signal().await?;
            tracing::info!("application stopping");
            for server in [http.as_ref(), https.as_ref()].into_iter().flatten() {
                server.graceful_shutdown();
            }
            Ok::<(), Error>(())
        }
    )?;
    Ok(())
}

async fn prepare(config: Option<&http::Config>) -> tlab::Result<Option<http::Server>> {
    match config {
        Some(config) => Ok(Some(http::Server::new(config).await?)),
        None => Ok(None),
    }
}

async fn run(server: Option<&http::Server>, router: Router) -> tlab::Result<()> {
    if let Some(server) = server {
        server.run(router).await?;
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
