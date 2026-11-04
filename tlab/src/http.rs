//! One HTTP or HTTPS listener for an application-assembled Axum router.
//!
//! The application owns the number of instances, shared router assembly,
//! coordinated startup and shutdown, and the timing of TLS reloads. Each server
//! owns one listener; configuring TLS selects HTTPS.
//!
//! ```no_run
//! use std::future::Future;
//! use axum::Router;
//! use tlab::http::{Config, Server};
//!
//! async fn run(config: &Config, router: Router,
//!              shutdown: impl Future<Output = ()> + Send) -> tlab::Result<()> {
//!     // Merge StaticFilesConfig::router() and apply common layers here.
//!     let server = Server::bind(config, router).await?;
//!     server.serve(shutdown).await
//! }
//! ```

use std::{
    future::Future,
    io,
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use axum::Router;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, pem::PemObject};
use serde::Deserialize;
use tokio::{
    net::TcpListener,
    sync::Mutex,
    task::spawn_blocking,
    time::{Instant, timeout},
};
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

/// One prepared listener and its complete router. Binding does not serve requests.
pub struct Server {
    listener: StdTcpListener,
    addr: SocketAddr,
    tls: Option<TlsReloader>,
    router: Router,
    shutdown_timeout: Duration,
}

impl Server {
    /// Prepare TLS, if configured, and bind one listener.
    /// The caller assembles static routes and coordinates any other instances.
    pub async fn bind(config: &Config, router: Router) -> Result<Self> {
        let shutdown_timeout = Duration::from_secs(config.shutdown_timeout_secs);
        if shutdown_timeout.is_zero() || Instant::now().checked_add(shutdown_timeout).is_none() {
            return Err(Error::InvalidInput("invalid shutdown timeout".into()));
        }
        let tls = if let Some(files) = &config.tls {
            let mut files = files.clone();
            let cwd = std::env::current_dir().context("failed to resolve TLS file paths")?;
            // Preserve empty paths so keyfiles input validation still applies.
            for path in [&mut files.certificate_chain, &mut files.private_key] {
                if !path.as_os_str().is_empty() && path.is_relative() {
                    *path = cwd.join(&*path);
                }
            }
            let active = RustlsConfig::from_config(load_tls(files.clone()).await?);
            files.generate_if_missing = false;
            Some(TlsReloader {
                inner: Arc::new(ReloadState {
                    files,
                    active,
                    lock: Mutex::new(()),
                }),
            })
        } else {
            None
        };
        let listener = TcpListener::bind(config.bind)
            .await
            .and_then(TcpListener::into_std)
            .with_context(|| format!("failed to bind HTTP listener at {}", config.bind))?;
        let addr = listener
            .local_addr()
            .context("failed to read bound HTTP address")?;
        Ok(Self {
            listener,
            addr,
            tls,
            router,
            shutdown_timeout,
        })
    }

    /// Actual bound address, including an OS-assigned port when configured as 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Obtain independent reload access before consuming this server.
    pub fn tls_reloader(&self) -> Result<TlsReloader> {
        self.tls
            .clone()
            .ok_or_else(|| Error::IllegalState("TLS is not configured".into()))
    }

    /// Serve this listener until shutdown, then drain requests within the limit.
    ///
    /// Dropping this future requests immediate shutdown. Await it with a completed
    /// shutdown signal to observe graceful cleanup and errors. Incomplete TLS
    /// handshakes retain the backend timeout and may outlive forced shutdown.
    /// Immediate connection termination belongs to the infrastructure layer.
    pub async fn serve<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send,
    {
        let handle = Handle::new();
        let _guard = ShutdownGuard(handle.clone());
        let service = self.router.into_make_service();
        if let Some(tls) = self.tls {
            let server = axum_server::from_tcp_rustls(self.listener, tls.inner.active.clone())
                .context("failed to prepare HTTPS server")?
                .handle(handle.clone());
            serve_until_shutdown(
                server.serve(service),
                &handle,
                self.shutdown_timeout,
                shutdown,
            )
            .await
        } else {
            let server = axum_server::from_tcp(self.listener)
                .context("failed to prepare HTTP server")?
                .handle(handle.clone());
            serve_until_shutdown(
                server.serve(service),
                &handle,
                self.shutdown_timeout,
                shutdown,
            )
            .await
        }
    }
}

/// Cloneable access to a server's TLS settings. It cannot reopen a stopped server.
#[derive(Clone)]
pub struct TlsReloader {
    inner: Arc<ReloadState>,
}

struct ReloadState {
    files: TlsCertFile,
    active: RustlsConfig,
    lock: Mutex<()>,
}

impl TlsReloader {
    /// Reload the original file paths, preserving active TLS on failure.
    /// Reload never generates files. Calls are serialized across clones.
    pub async fn reload(&self) -> Result<()> {
        let _lock = self.inner.lock.lock().await;
        let config = load_tls(self.inner.files.clone()).await?;
        // No await between receiving validated material and committing it.
        self.inner.active.reload_from_config(config);
        Ok(())
    }
}

async fn load_tls(files: TlsCertFile) -> Result<Arc<rustls::ServerConfig>> {
    // Shared by startup and reload: keyfiles performs synchronous disk I/O,
    // PEM parsing, and key validation. Only the caller may commit the result.
    spawn_blocking(move || {
        let material = files.load()?;
        // keyfiles already validates these PEM sections; discard decoder errors
        // because their source chains may include secret input lines.
        let certificates =
            CertificateDer::pem_slice_iter(material.certificate_chain_pem().as_bytes())
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| anyhow::anyhow!("failed to decode TLS certificates"))?;
        let key = PrivatePkcs8KeyDer::from_pem_slice(material.private_key_pem().as_bytes())
            .map_err(|_| anyhow::anyhow!("failed to decode TLS private key"))?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .context("failed to select TLS protocol versions")?
        .with_no_client_auth()
        .with_single_cert(certificates, key.into())
        .context("failed to configure TLS certificate and private key")?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    })
    .await
    .context("TLS preparation task failed")?
}

struct ShutdownGuard(Handle<SocketAddr>);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

async fn serve_until_shutdown<S, F>(
    server: S,
    handle: &Handle<SocketAddr>,
    limit: Duration,
    shutdown: F,
) -> Result<()>
where
    S: Future<Output = io::Result<()>>,
    F: Future<Output = ()> + Send,
{
    tokio::pin!(server);
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        result = &mut server => {
            result.context("HTTP server failed")?;
            return Err(Error::Internal(anyhow::anyhow!("server stopped before shutdown was requested")));
        }
        _ = &mut shutdown => {}
    }
    handle.graceful_shutdown(None);
    timeout(limit, &mut server)
        .await
        .context("HTTP shutdown timed out")?
        .context("HTTP server failed during shutdown")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backend_failure_and_unexpected_exit_reach_the_caller() {
        let handle = Handle::new();
        let error = serve_until_shutdown(
            std::future::ready(Err(io::Error::other("injected backend failure"))),
            &handle,
            Duration::from_secs(1),
            std::future::pending(),
        )
        .await
        .unwrap_err();
        assert!(format!("{error:?}").contains("injected backend failure"));
        let error = serve_until_shutdown(
            std::future::ready(Ok(())),
            &handle,
            Duration::from_secs(1),
            std::future::pending(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("stopped before shutdown"));
    }

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
        let server = Server::bind(&config, Router::new()).await.unwrap();
        let reloader = server.tls_reloader().unwrap();
        let original = reloader.inner.active.get_inner();
        let lock = reloader.inner.lock.lock().await;
        let queued = reloader.clone();
        let task = tokio::spawn(async move { queued.reload().await });
        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(Arc::ptr_eq(&original, &reloader.inner.active.get_inner()));
        drop(lock);
        timeout(Duration::from_secs(1), reloader.reload())
            .await
            .unwrap()
            .unwrap();
        assert!(!Arc::ptr_eq(&original, &reloader.inner.active.get_inner()));
    }
}
