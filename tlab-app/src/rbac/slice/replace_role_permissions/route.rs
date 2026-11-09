use super::usecase;
use crate::rbac::{PermissionCode, PermissionSet, RoleId};
use crate::{
    app::{Container, response},
    auth::token::AccessClaims,
    rbac::Actor,
};
use axum::extract::Path;
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
    permissions: Vec<String>,
    version: i64,
}
pub(in crate::rbac) async fn replace_role_permissions(
    State(container): State<Arc<Container>>,
    claims: AccessClaims,
    Path(id): Path<i64>,
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
        RoleId(id),
        command.version,
        PermissionSet::from_codes(command.permissions.into_iter().map(PermissionCode)),
    )
    .await
    {
        Ok(value) => response::success(StatusCode::OK, value),
        Err(error) => error.into_response(),
    }
}
