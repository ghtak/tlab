//! Shared JSON responses. Feature routes decide their own status and error codes.

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
struct ErrorBody {
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
    message: &'static str,
}

pub fn error(
    status: StatusCode,
    code: &'static str,
    field: Option<&'static str>,
    message: &'static str,
) -> Response {
    (
        status,
        Json(ErrorBody {
            code,
            field,
            message,
        }),
    )
        .into_response()
}

pub fn invalid_json(rejection: JsonRejection) -> Response {
    // Parser diagnostics may contain user input, including passwords.
    let status = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    error(status, "INVALID_REQUEST", None, "invalid JSON request")
}

/// Keep diagnostic categories on the server without logging arbitrary error text.
pub fn internal_error(
    operation: &'static str,
    stage: &'static str,
    cause: &(dyn std::error::Error + 'static),
    message: &'static str,
) -> Response {
    let (error_kind, sqlstate) = error_metadata(cause);
    tracing::error!(
        operation,
        stage,
        error_kind,
        sqlstate = sqlstate.as_deref().unwrap_or(""),
        "request failed"
    );
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        None,
        message,
    )
}

fn error_metadata(cause: &(dyn std::error::Error + 'static)) -> (&'static str, Option<String>) {
    let mut current = Some(cause);
    let mut fallback = "unknown";
    while let Some(cause) = current {
        if let Some(error) = cause.downcast_ref::<sqlx::Error>() {
            return match error {
                sqlx::Error::Database(error) => {
                    ("database", error.code().map(|code| code.into_owned()))
                }
                sqlx::Error::PoolTimedOut => ("database_pool_timeout", None),
                sqlx::Error::PoolClosed => ("database_pool_closed", None),
                sqlx::Error::Io(_) => ("database_io", None),
                _ => ("database_client", None),
            };
        }
        if let Some(error) = cause.downcast_ref::<tokio::task::JoinError>() {
            return (
                if error.is_panic() {
                    "task_panic"
                } else {
                    "task_cancelled"
                },
                None,
            );
        }
        if cause.is::<std::io::Error>() {
            return ("io", None);
        }
        if let Some(error) = cause.downcast_ref::<tlab::Error>() {
            fallback = match error {
                tlab::Error::InvalidInput(_) => "invalid_input",
                tlab::Error::IllegalState(_) => "illegal_state",
                tlab::Error::IoError(_) => "io",
                tlab::Error::Config(_) => "configuration",
                tlab::Error::DbError(_) => "database",
                tlab::Error::Internal(_) => "internal",
                _ => "application",
            };
        }
        current = cause.source();
    }
    (fallback, None)
}
