use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};

use super::usecase::{self, Error};
use crate::app::error::InvalidInput;
use crate::app::{Container, response};
use crate::auth::entity::AccountStatus;

#[derive(serde::Serialize)]
struct LoginResponse {
    account_id: i64,
    status: AccountStatus,
    access_token: String,
    refresh_token: String,
}

pub(crate) async fn login(
    State(container): State<Arc<Container>>,
    payload: Result<Json<usecase::Command>, JsonRejection>,
) -> Response {
    let result = match payload {
        Err(rejection) => response::invalid_json(rejection),
        Ok(Json(command)) => match container
            .local_login
            .execute_with_tokens(&container.database, command)
            .await
        {
            Ok(tokens) => response::success(
                StatusCode::OK,
                LoginResponse {
                    account_id: tokens.account.id,
                    status: tokens.account.status,
                    access_token: tokens.access.expose_secret().into(),
                    refresh_token: tokens.refresh.expose_secret().into(),
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
            Error::InvalidInput(InvalidInput {
                field,
                code,
                message,
            }) => response::error(StatusCode::BAD_REQUEST, code, Some(field), message),
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

/// Web transport prepares cookies after token issuance commits and never returns secrets in JSON.
pub(in crate::auth) async fn web_login(
    State(container): State<Arc<Container>>,
    cookies: tower_cookies::Cookies,
    payload: Result<Json<usecase::Command>, JsonRejection>,
) -> Response {
    use crate::auth::web;
    let result = match payload {
        Err(error) => response::invalid_json(error),
        Ok(Json(command)) => match container
            .local_login
            .execute_with_tokens(&container.database, command)
            .await
        {
            Ok(tokens) => {
                let now = sqlx::types::time::OffsetDateTime::now_utc().unix_timestamp() as u64;
                let claims = match container
                    .access_verifier
                    .verify(tokens.access.expose_secret(), now)
                {
                    Ok(claims) => claims,
                    Err(_) => {
                        return web::no_store(
                            Error::Internal(crate::app::error::InternalFailure {
                                stage: "issued_access_verify",
                                cause: tlab::Error::IllegalState(
                                    "issued access verification failed".into(),
                                ),
                            })
                            .into_response(),
                        );
                    }
                };
                let prepared =
                    match web::login_cookies(&container.config.auth.web, &tokens, &claims) {
                        Ok(prepared) => prepared,
                        Err(_) => return web::no_store(web::cookie_too_large()),
                    };
                for cookie in prepared {
                    cookies.add(cookie);
                }
                response::success(
                    StatusCode::OK,
                    serde_json::json!({"account_id": tokens.account.id, "status": tokens.account.status}),
                )
            }
            Err(error) => error.into_response(),
        },
    };
    web::no_store(result)
}
