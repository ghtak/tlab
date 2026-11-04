//! Process-wide tracing subscriber initialization.

use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;
use tracing_appender::{
    non_blocking::{NonBlockingBuilder, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub console: Option<ConsoleTraceConfig>,
    pub file: Option<FileTraceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleTraceConfig {
    /// Defaults to `info`.
    #[serde(default = "default_filter")]
    pub filter: String,
    /// Defaults to tracing-appender's buffer capacity.
    #[serde(default = "default_buffered_lines_limit")]
    pub buffered_lines_limit: usize,
    /// Drops logs at capacity when true; otherwise blocks the calling thread.
    /// Defaults to true.
    #[serde(default = "default_lossy")]
    pub lossy: bool,
    /// Enables ANSI colors. Defaults to false.
    #[serde(default)]
    pub ansi: bool,
}

/// File tracing settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTraceConfig {
    pub directory: PathBuf,
    /// Filename prefix; daily rotation appends a UTC date.
    pub filename_prefix: String,
    /// Defaults to `info`.
    #[serde(default = "default_filter")]
    pub filter: String,
    /// Defaults to tracing-appender's buffer capacity.
    #[serde(default = "default_buffered_lines_limit")]
    pub buffered_lines_limit: usize,
    /// Drops logs at capacity when true; otherwise blocks the calling thread.
    /// Defaults to true.
    #[serde(default = "default_lossy")]
    pub lossy: bool,
}

fn default_filter() -> String {
    "info".into()
}

fn default_buffered_lines_limit() -> usize {
    tracing_appender::non_blocking::DEFAULT_BUFFERED_LINES_LIMIT
}

fn default_lossy() -> bool {
    true
}

/// Keeps logging workers alive and waits for pending logs when dropped.
///
/// Retain this value in `main` until all tasks that emit logs have finished.
/// Bind it to a named variable such as `_tracing_guard`, rather than `_`.
/// Dropping it does not uninstall the global subscriber. Forced process exit
/// bypasses this cleanup, and worker shutdown has a bounded wait.
#[must_use = "keep this guard alive until application shutdown"]
pub struct TracingGuard {
    _workers: Vec<WorkerGuard>,
}

/// Installs the application's process-wide tracing subscriber.
///
/// If `RUST_LOG` is set, it overrides every configured output's filter.
/// An invalid value causes initialization to fail. Otherwise each output's
/// configured filter is used. Filters are validated before creating outputs.
///
/// Calling this after a global subscriber has been installed returns an error.
/// With no outputs configured, this is a no-op returning an empty guard;
/// it does not inspect `RUST_LOG` or install a subscriber.
pub fn initialize(config: &Config) -> crate::Result<TracingGuard> {
    let mut guards = Vec::new();
    if config.console.is_none() && config.file.is_none() {
        return Ok(TracingGuard { _workers: guards });
    }

    let env_filter = std::env::var_os(EnvFilter::DEFAULT_ENV)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("RUST_LOG is not valid Unicode"))
                .and_then(|value| EnvFilter::try_new(value).context("invalid RUST_LOG filter"))
        })
        .transpose()?;
    let console_filter = config
        .console
        .as_ref()
        .map(|config| resolve_filter(&config.filter, env_filter.as_ref()))
        .transpose()?;
    let file_filter = config
        .file
        .as_ref()
        .map(|config| resolve_filter(&config.filter, env_filter.as_ref()))
        .transpose()?;

    let console_layer = config
        .console
        .as_ref()
        .zip(console_filter)
        .map(|(config, filter)| {
            let (writer, guard) = NonBlockingBuilder::default()
                .buffered_lines_limit(config.buffered_lines_limit)
                .lossy(config.lossy)
                .finish(std::io::stderr());
            guards.push(guard);

            fmt::layer()
                .with_writer(writer)
                .compact()
                .with_ansi(config.ansi)
                .with_filter(filter)
        });

    let file_layer = config
        .file
        .as_ref()
        .zip(file_filter)
        .map(|(config, filter)| -> crate::Result<_> {
            let appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix(&config.filename_prefix)
                .build(&config.directory)
                .with_context(|| {
                    format!(
                        "failed to initialize log file `{}` in `{}`",
                        config.filename_prefix,
                        config.directory.display()
                    )
                })?;
            let (writer, guard) = NonBlockingBuilder::default()
                .buffered_lines_limit(config.buffered_lines_limit)
                .lossy(config.lossy)
                .finish(appender);
            guards.push(guard);

            Ok(fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_filter(filter))
        })
        .transpose()?;

    Registry::default()
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .context("failed to install the global tracing subscriber")?;

    Ok(TracingGuard { _workers: guards })
}

fn resolve_filter(configured: &str, env_filter: Option<&EnvFilter>) -> crate::Result<EnvFilter> {
    match env_filter {
        Some(filter) => Ok(filter.clone()),
        None => EnvFilter::try_new(configured)
            .with_context(|| format!("invalid tracing filter `{configured}`"))
            .map_err(crate::Error::Internal),
    }
}
