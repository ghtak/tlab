//! Typed configuration loaded from a file and application environment variables.

use std::path::Path;

use ::config::{Config, Environment, File};
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// Loads a required file, applies environment overrides, and deserializes `T`.
///
/// File formats and name discovery follow the `config` crate's default features.
/// Relative paths use the process's current directory. A basename such as `app`
/// can discover `app.toml`, `app.yaml`, or another supported format.
///
/// With prefix `APP`, `APP_SERVER_CONFIG__PORT=9090` overrides
/// `server_config.port`. Keys are case insensitive; a single underscore belongs
/// to the field name and `__` separates nested fields. Pass the prefix without
/// a trailing separator. Unrelated environment variables are excluded.
/// Values stay strings until deserialization, preserving leading zeroes and
/// empty strings for string fields. No list parsing syntax is added.
/// Missing values and unknown fields follow `T`'s Serde attributes.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for an empty prefix and [`Error::Config`]
/// for file discovery, reading, parsing, environment, or deserialization errors.
/// Environment variables cannot bypass a missing or malformed file.
/// Underlying error messages may contain input values; callers control logging.
///
/// # Example
///
/// ```no_run
/// use serde::Deserialize;
///
/// mod server {
///     #[derive(serde::Deserialize)]
///     pub struct Config {
///         pub host: String,
///         pub port: u16,
///     }
/// }
///
/// #[derive(Deserialize)]
/// struct AppConfig {
///     pub server_config: server::Config,
/// }
///
/// # fn main() -> tlab::Result<()> {
/// let settings: AppConfig = tlab::config::load("app.yaml", "APP")?;
/// # Ok(())
/// # }
/// ```
pub fn load<T>(filename: impl AsRef<Path>, env_prefix: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    if env_prefix.is_empty() {
        return Err(Error::InvalidInput(
            "environment prefix must not be empty".into(),
        ));
    }

    let settings = Config::builder()
        .add_source(File::from(filename.as_ref()).required(true))
        .add_source(
            Environment::with_prefix(env_prefix)
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    Ok(settings.try_deserialize()?)
}
