use axum::response::IntoResponse;
use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::Response,
};

use super::usecase as create_local_user;

use crate::app::{Container, response};

pub(crate) async fn create_local_user(
    State(container): State<Arc<Container>>,
    claims: crate::auth::token::AccessClaims,
    payload: Result<Json<create_local_user::Command>, JsonRejection>,
) -> Response {
    use crate::{
        app::error::{AppError, AppErrorCode},
        rbac::{Actor, PermissionCode, require_permission},
    };
    let actor = match Actor::try_from_verified_claims(&claims) {
        Ok(actor) => actor,
        Err(_) => {
            return response::error(
                StatusCode::UNAUTHORIZED,
                "INVALID_ACCESS_TOKEN",
                "authentication required",
            );
        }
    };
    match require_permission(
        &actor,
        &PermissionCode("account.create".into()),
        container.role_permission_resolver.as_ref(),
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            return AppError::new(
                AppErrorCode::RbacForbidden,
                "permission denied",
                "create_local_user_authorize",
            )
            .into_response();
        }
        Err(error) => {
            return AppError::internal(
                "create_local_user_authorize",
                "user creation failed",
                error,
            )
            .into_response();
        }
    }
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
