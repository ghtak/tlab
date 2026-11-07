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

/// Build a generic 500 response. The caller owns diagnostic logging.
pub fn internal_error(message: &'static str) -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        None,
        message,
    )
}
