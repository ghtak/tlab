//! Application failures: public descriptions and private diagnostic context.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppErrorCode {
    NicknameRequired,
    InvalidNickname,
    InvalidEmail,
    PasswordRequired,
    PasswordRejected,
    EmailAlreadyExists,
    InvalidCredentials,
    AccountUnavailable,
    Busy,
    InvalidRefreshToken,
    InternalError,
}

#[derive(Debug)]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: &'static str,
    pub stage: &'static str,
    pub cause: Option<tlab::Error>,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: &'static str, stage: &'static str) -> Self {
        Self {
            code,
            message,
            stage,
            cause: None,
        }
    }

    pub fn internal(
        stage: &'static str,
        message: &'static str,
        cause: impl Into<tlab::Error>,
    ) -> Self {
        Self {
            code: AppErrorCode::InternalError,
            message,
            stage,
            cause: Some(cause.into()),
        }
    }
}
