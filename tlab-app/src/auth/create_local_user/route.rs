use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::usecase as create_local_user;
use crate::app::error::InvalidInput;
use crate::app::{Container, response};

// RBAC will check the authenticated caller before invoking this service.
pub(crate) async fn create_local_user(
    State(container): State<Arc<Container>>,
    payload: Result<Json<create_local_user::Command>, JsonRejection>,
) -> Response {
    let Json(command) = match payload {
        Ok(command) => command,
        Err(rejection) => return response::invalid_json(rejection),
    };
    match create_local_user::execute(
        &container.database,
        container.passwd_policy.as_ref(),
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        command,
    )
    .await
    {
        Ok(created) => response::success(StatusCode::CREATED, created),
        Err(error) => error.into_response(),
    }
}

impl IntoResponse for create_local_user::Error {
    fn into_response(self) -> Response {
        use create_local_user::Error;
        match self {
            Error::InvalidInput(InvalidInput {
                field,
                code,
                message,
            }) => response::error(StatusCode::BAD_REQUEST, code, Some(field), message),
            Error::EmailAlreadyExists => response::error(
                StatusCode::CONFLICT,
                "EMAIL_ALREADY_EXISTS",
                Some("email"),
                "local email already exists",
            ),
            Error::Busy => {
                tracing::warn!(
                    operation = "create_local_user",
                    stage = "hash_capacity",
                    "password hashing is busy"
                );
                response::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BUSY",
                    None,
                    "password hashing is busy",
                )
            }
            Error::Internal(failure) => {
                tracing::error!(
                    operation = "create_local_user",
                    stage = failure.stage,
                    cause = ?failure.cause,
                    "user creation failed"
                );
                response::internal_error("user creation failed")
            }
        }
    }
}
