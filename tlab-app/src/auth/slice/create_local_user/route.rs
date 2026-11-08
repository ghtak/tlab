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
