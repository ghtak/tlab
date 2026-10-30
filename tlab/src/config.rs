use std::path::PathBuf;

use anyhow::Context;
use serde::de::DeserializeOwned;

/// Loads configuration from a file, prefixed environment variables, or both.
pub struct Loader {
    file: Option<PathBuf>,
    env_prefix: Option<String>,
}

impl Loader {
    /// Selects a required file, read when loading rather than constructing.
    pub fn from_file(path: PathBuf) -> Self {
        Self {
            file: Some(path),
            env_prefix: None,
        }
    }

    /// Selects only environment variables matching `prefix` followed by `_`.
    pub fn from_env(prefix: impl Into<String>) -> Self {
        Self {
            file: None,
            env_prefix: Some(prefix.into()),
        }
    }

    /// Sets or replaces the environment prefix, preserving the selected file.
    pub fn env_prefix(mut self, env_prefix: impl Into<String>) -> Self {
        self.env_prefix = Some(env_prefix.into());
        self
    }

    /// Reads sources, with environment values overriding matching file keys.
    /// Environment keys use `__` for nesting; values are parsed as scalar types.
    pub fn load(self) -> crate::Result<::config::Config> {
        let mut builder = ::config::Config::builder();

        if let Some(path) = self.file {
            builder = builder.add_source(::config::File::from(path));
        }

        if let Some(prefix) = self.env_prefix {
            builder = builder.add_source(
                ::config::Environment::with_prefix(&prefix)
                    .prefix_separator("_")
                    .separator("__")
                    .try_parsing(true),
            );
        }

        builder
            .build()
            .map_err(|error| crate::Error::Internal(error.into()))
    }

    /// Loads and deserializes into the caller's owned configuration type.
    pub fn try_deserialize<T: DeserializeOwned>(self) -> crate::Result<T> {
        self.load()?
            .try_deserialize()
            .context("failed to deserialize config")
            .map_err(crate::Error::Internal)
    }
}
