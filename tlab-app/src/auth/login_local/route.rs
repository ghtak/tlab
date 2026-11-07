use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::post,
};

use super::usecase::{self, Error};
use crate::app::{Container, response};
use crate::auth::entity::AccountStatus;

#[derive(serde::Serialize)]
struct LoginResponse {
    account_id: i64,
    status: AccountStatus,
}

pub fn router() -> Router<Arc<Container>> {
    Router::new().route("/api/auth/local/login", post(login))
}

async fn login(
    State(container): State<Arc<Container>>,
    payload: Result<Json<usecase::Command>, JsonRejection>,
) -> Response {
    let result = match payload {
        Err(rejection) => response::invalid_json(rejection),
        Ok(Json(command)) => match container
            .local_login
            .execute(&container.database, command)
            .await
        {
            Ok(account) => response::success(
                StatusCode::OK,
                LoginResponse {
                    account_id: account.id,
                    status: account.status,
                },
            ),
            Err(error) => error.into_response(),
        },
    };
    no_store(result)
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        no_store(match self {
            Error::InvalidInput {
                field,
                code,
                message,
            } => response::error(StatusCode::BAD_REQUEST, code, Some(field), message),
            Error::InvalidCredentials => response::error(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                None,
                "email or password is incorrect",
            ),
            Error::AccountUnavailable => response::error(
                StatusCode::FORBIDDEN,
                "ACCOUNT_UNAVAILABLE",
                None,
                "account is unavailable",
            ),
            Error::Busy => {
                tracing::warn!(
                    operation = "login_local",
                    stage = "hash_capacity",
                    "password verification is busy"
                );
                response::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BUSY",
                    None,
                    "password verification is busy",
                )
            }
            Error::Internal(failure) => {
                tracing::error!(operation = "login_local", stage = failure.stage, cause = ?failure.cause, "local login failed");
                response::internal_error("local login failed")
            }
        })
    }
}
