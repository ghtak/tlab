//! Common errors and result types for tlab and its consumers.

use std::borrow::Cow;

/// A common error that preserves diagnostic messages and underlying causes.
///
/// Additional variants may be introduced as features are added. Consumers must
/// include a fallback when matching:
///
/// ```
/// use tlab::Error;
///
/// let error = Error::IllegalState("not initialized".into());
/// let message = match &error {
///     Error::IllegalState(message) => message.as_ref(),
///     _ => "another error",
/// };
/// assert_eq!(message, "not initialized");
/// ```
///
/// Matching only the currently defined variants is rejected outside tlab:
///
/// ```compile_fail,E0004
/// use tlab::Error;
///
/// fn classify(error: &Error) -> &'static str {
///     match error {
///         Error::InvalidInput(_) => "input",
///         Error::IllegalState(_) => "state",
///         Error::IoError(_) => "io",
///         Error::Config(_) => "config",
///         Error::DbError(_) => "database",
///         Error::Internal(_) => "internal",
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An argument passed to a public operation is invalid.
    #[error("invalid input: {0}")]
    InvalidInput(Cow<'static, str>),

    /// An operation cannot be performed in the current state.
    #[error("illegal state: {0}")]
    IllegalState(Cow<'static, str>),

    /// An I/O failure, retaining the original error as its source.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// A configuration failure, including file access and deserialization.
    ///
    /// File I/O errors reported by the configuration library remain in this
    /// variant's stored `ConfigError` rather than being reclassified as
    /// `IoError`. Inner causes are available through `ConfigError`'s fields;
    /// that library does not expose them through `std::error::Error::source`.
    #[error("configuration error: {0}")]
    Config(#[from] ::config::ConfigError),

    /// A SQLx failure, retaining the original error and database diagnostics.
    #[error("database error: {0}")]
    DbError(#[from] sqlx::Error),

    /// An unclassified failure, retaining its context and cause chain.
    ///
    /// Errors already wrapped in anyhow remain internal errors even when the
    /// underlying cause has a dedicated variant. Display shows the outermost
    /// message; inspect the stored error or its source chain for details.
    #[error("{0}")]
    Internal(#[from] anyhow::Error),
}

/// A result whose error type is [`Error`].
pub type Result<R> = std::result::Result<R, Error>;
