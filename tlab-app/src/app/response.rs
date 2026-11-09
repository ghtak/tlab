//! HTTP responses and the application error-to-HTTP mapping.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub fn success<T: Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(data)).into_response()
}

#[derive(Serialize)]
struct ErrorBody<C> {
    code: C,
    message: &'static str,
}

pub fn error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(ErrorBody { code, message })).into_response()
}

pub fn invalid_json(rejection: JsonRejection) -> Response {
    // Parser diagnostics may contain user input, including passwords.
    let status = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    error(status, "INVALID_REQUEST", "invalid JSON request")
}

/// Build a generic 500 response. The caller owns diagnostic logging.
pub fn internal_error(message: &'static str) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
}

/// Log diagnostic context once at the request boundary and expose only public data.
impl IntoResponse for crate::app::error::AppError {
    fn into_response(self) -> Response {
        let error = self;
        use crate::app::error::AppErrorCode;
        let status = match error.code {
            AppErrorCode::NicknameRequired
            | AppErrorCode::InvalidNickname
            | AppErrorCode::InvalidEmail
            | AppErrorCode::PasswordRequired
            | AppErrorCode::PasswordRejected => StatusCode::BAD_REQUEST,
            AppErrorCode::EmailAlreadyExists => StatusCode::CONFLICT,
            AppErrorCode::InvalidCredentials | AppErrorCode::InvalidRefreshToken => {
                StatusCode::UNAUTHORIZED
            }
            AppErrorCode::AccountUnavailable => StatusCode::FORBIDDEN,
            AppErrorCode::RbacForbidden => StatusCode::FORBIDDEN,
            AppErrorCode::RbacNotFound => StatusCode::NOT_FOUND,
            AppErrorCode::RbacConflict => StatusCode::CONFLICT,
            AppErrorCode::RbacInvalidInput => StatusCode::BAD_REQUEST,
            AppErrorCode::Busy => StatusCode::SERVICE_UNAVAILABLE,
            AppErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        match error.code {
            AppErrorCode::Busy => {
                tracing::warn!(stage = error.stage, cause = ?error.cause, code = ?error.code, "application request rejected");
            }
            AppErrorCode::InternalError => {
                tracing::error!(stage = error.stage, cause = ?error.cause, code = ?error.code, "application request failed");
            }
            _ => {
                tracing::info!(stage = error.stage, cause = ?error.cause, code = ?error.code, "application request rejected");
            }
        }
        let mut response = (
            status,
            Json(ErrorBody {
                code: error.code,
                message: error.message,
            }),
        )
            .into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        response
    }
}

#[cfg(test)]
mod rbac_tests {
    use super::*;
    use crate::app::error::{AppError, AppErrorCode};

    #[test]
    fn rbac_failures_map_to_http_statuses() {
        for (code, status) in [
            (AppErrorCode::RbacForbidden, StatusCode::FORBIDDEN),
            (AppErrorCode::RbacNotFound, StatusCode::NOT_FOUND),
            (AppErrorCode::RbacConflict, StatusCode::CONFLICT),
            (AppErrorCode::RbacInvalidInput, StatusCode::BAD_REQUEST),
            (
                AppErrorCode::InternalError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            let response = AppError::new(code, "error", "test").into_response();
            assert_eq!(response.status(), status);
            assert_eq!(
                response.headers()[axum::http::header::CACHE_CONTROL],
                "no-store"
            );
        }
    }
}
