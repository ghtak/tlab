use super::usecase;
use crate::rbac::NewRole;
use crate::{
    app::{Container, response},
    auth::jwt::AccessClaims,
    rbac::Actor,
};
use axum::{Json, extract::rejection::JsonRejection};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::rbac) struct Command {
    name: String,
    description: String,
}
pub(in crate::rbac) async fn create_role(
    State(container): State<Arc<Container>>,
    claims: AccessClaims,
    payload: Result<Json<Command>, JsonRejection>,
) -> Response {
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
    let Json(command) = match payload {
        Ok(command) => command,
        Err(error) => return response::invalid_json(error),
    };
    match usecase::execute(
        &container.database,
        &actor,
        container.role_permission_resolver.as_ref(),
        NewRole {
            name: command.name,
            description: command.description,
        },
    )
    .await
    {
        Ok(value) => response::success(StatusCode::CREATED, value),
        Err(error) => error.into_response(),
    }
}
