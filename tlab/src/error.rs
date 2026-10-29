use std::borrow::Cow;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid parameter: {0}")]
    InvalidParameter(Cow<'static, str>),
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
