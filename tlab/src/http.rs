//! One HTTP or HTTPS listener for an application-assembled Axum router.
//!
//! The application owns the number of instances, shared router assembly,
//! coordinated startup and shutdown, and the timing of TLS reloads. Each run
//! owns one listener; configuring TLS selects HTTPS.
//!
//! ```no_run
//! use axum::Router;
//! use tlab::http::{Config, Server};
//!
//! async fn run(config: &Config, router: Router,
//!              shutdown: impl std::future::Future<Output = ()>) -> tlab::Result<()> {
//!     let server = Server::new(config).await?;
//!     tokio::try_join!(server.run(router), async {
//!         shutdown.await;
//!         server.graceful_shutdown();
//!         Ok::<(), tlab::Error>(())
//!     })?;
//!     Ok(())
//! }
//! ```

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context;
use axum::Router;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use serde::Deserialize;
use tokio::{net::TcpListener, sync::Mutex, task::spawn_blocking, time::Instant};
use tower_http::services::ServeDir;

use crate::{Error, Result, keyfiles::TlsCertFile};

/// One listener. `tls: None` serves HTTP; `Some` serves HTTPS.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub bind: SocketAddr,
    pub tls: Option<TlsCertFile>,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticFilesConfig {
    #[serde(default = "default_static_prefix")]
    pub prefix: String,
    pub directory: PathBuf,
}

fn default_shutdown_timeout_secs() -> u64 {
    30
}

fn default_static_prefix() -> String {
    "/static".into()
}

impl StaticFilesConfig {
    /// Build a router to merge before applying the application's common layers.
    ///
    /// The prefix must be a fixed, non-root URL path. The directory must exist.
    /// Symlinks follow filesystem semantics, including targets outside the root.
    /// Conflicts when merging with application routes are Axum assembly errors.
    pub fn router(&self) -> Result<Router> {
        let prefix = self.prefix.trim_end_matches('/');
        if !prefix.starts_with('/')
            || prefix.is_empty()
            || prefix.chars().any(|c| {
                c.is_control() || matches!(c, '{' | '}' | '*' | ':' | '?' | '#' | '\\' | '%')
            })
            || prefix[1..]
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(Error::InvalidInput("invalid static file prefix".into()));
        }
        if self.directory.as_os_str().is_empty() {
            return Err(Error::InvalidInput("empty static file directory".into()));
        }
        let directory = self
            .directory
            .canonicalize()
            .map_err(|_| Error::InvalidInput("static file directory is not accessible".into()))?;
        if !directory.is_dir() {
            return Err(Error::InvalidInput(
                "static file root must be a directory".into(),
            ));
        }
        Ok(Router::new().nest_service(prefix, ServeDir::new(directory)))
    }
}

/// Reusable configuration and TLS state for one HTTP or HTTPS listener.
/// The listener and router belong to each `run` call.
pub struct Server {
    config: Config,
    tls_config: Option<RustlsConfig>,
    reload_lock: Mutex<()>,
    handle: Handle<SocketAddr>,
}

impl Server {
    /// Validate settings and prepare TLS without binding a listener.
    pub async fn new(config: &Config) -> Result<Self> {
        let shutdown_timeout = Duration::from_secs(config.shutdown_timeout_secs);
        if shutdown_timeout.is_zero() || Instant::now().checked_add(shutdown_timeout).is_none() {
            return Err(Error::InvalidInput("invalid shutdown timeout".into()));
        }
        let mut config = config.clone();
        let tls_config = if let Some(files) = &mut config.tls {
            let cwd = std::env::current_dir().context("failed to resolve TLS file paths")?;
            // Preserve empty paths so keyfiles input validation still applies.
            for path in [&mut files.certificate_chain, &mut files.private_key] {
                if !path.as_os_str().is_empty() && path.is_relative() {
                    *path = cwd.join(&*path);
                }
            }
            let source = files.clone();
            let material = spawn_blocking(move || source.load())
                .await
                .context("TLS file preparation task failed")??;
            let tls_config = RustlsConfig::from_pem(
                material.certificate_chain_pem().as_bytes().to_vec(),
                material.private_key_pem().as_bytes().to_vec(),
            )
            .await
            .context("failed to configure TLS certificate and private key")?;
            Some(tls_config)
        } else {
            None
        };
        Ok(Self {
            config,
            tls_config,
            reload_lock: Mutex::new(()),
            handle: Handle::new(),
        })
    }

    /// Reload the original TLS file paths, preserving active TLS on failure.
    /// Reload never generates files. Concurrent calls are serialized.
    pub async fn reload_tls_config(&self) -> Result<()> {
        let (Some(tls), Some(files)) = (&self.tls_config, &self.config.tls) else {
            return Err(Error::IllegalState("TLS is not configured".into()));
        };
        let _lock = self.reload_lock.lock().await;
        let mut files = files.clone();
        files.generate_if_missing = false;
        let material = spawn_blocking(move || files.load())
            .await
            .context("TLS file preparation task failed")??;
        tls.reload_from_pem(
            material.certificate_chain_pem().as_bytes().to_vec(),
            material.private_key_pem().as_bytes().to_vec(),
        )
        .await
        .context("failed to reload TLS certificate and private key")?;
        Ok(())
    }

    /// Stop accepting connections and drain requests within the configured limit.
    /// Callers await `run()` to observe shutdown completion and errors.
    pub fn graceful_shutdown(&self) {
        self.handle
            .graceful_shutdown(Some(Duration::from_secs(self.config.shutdown_timeout_secs)));
    }

    /// Bind and serve until the caller requests shutdown.
    /// Use one run per Server; create a new Server for a subsequent execution.
    /// Port 0 is supported, but the assigned address is not exposed.
    /// Dropping this future requests immediate connection shutdown. Incomplete
    /// TLS handshakes retain the backend timeout and may outlive shutdown.
    pub async fn run(&self, router: Router) -> Result<()> {
        let listener = TcpListener::bind(self.config.bind)
            .await
            .and_then(TcpListener::into_std)
            .with_context(|| format!("failed to bind HTTP listener at {}", self.config.bind))?;
        let address = listener
            .local_addr()
            .context("failed to read bound HTTP address")?;
        tracing::info!(%address, tls = self.tls_config.is_some(), "listener started");
        let handle = self.handle.clone();
        let _guard = ShutdownGuard(handle.clone());
        let service = router.into_make_service_with_connect_info::<SocketAddr>();
        if let Some(tls) = &self.tls_config {
            axum_server::from_tcp_rustls(listener, tls.clone())
                .context("failed to prepare HTTPS server")?
                .handle(handle.clone())
                .serve(service)
                .await
                .context("HTTPS server failed")?;
        } else {
            axum_server::from_tcp(listener)
                .context("failed to prepare HTTP server")?
                .handle(handle.clone())
                .serve(service)
                .await
                .context("HTTP server failed")?;
        }
        Ok(())
    }
}

struct ShutdownGuard(Handle<SocketAddr>);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn cancelling_a_queued_reload_preserves_tls_and_releases_the_waiter() {
        let directory = tempfile::tempdir().unwrap();
        let files = TlsCertFile {
            certificate_chain: directory.path().join("chain.pem"),
            private_key: directory.path().join("key.pem"),
            generate_if_missing: true,
            generation: None,
        };
        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            tls: Some(files),
            shutdown_timeout_secs: 1,
        };
        let server = Arc::new(Server::new(&config).await.unwrap());
        let tls = server.tls_config.as_ref().unwrap();
        let original = tls.get_inner();
        let lock = server.reload_lock.lock().await;
        let queued = server.clone();
        let task = tokio::spawn(async move { queued.reload_tls_config().await });
        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(Arc::ptr_eq(&original, &tls.get_inner()));
        drop(lock);
        timeout(Duration::from_secs(1), server.reload_tls_config())
            .await
            .unwrap()
            .unwrap();
        assert!(!Arc::ptr_eq(&original, &tls.get_inner()));
    }
}
