use super::usecase;
use crate::rbac::RoleId;
use crate::{
    app::{Container, response},
    auth::token::AccessClaims,
    rbac::Actor,
};
use axum::extract::Path;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
pub(in crate::rbac) async fn list_roles(
    State(container): State<Arc<Container>>,
    claims: AccessClaims,
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
    match usecase::execute(
        &container.database,
        &actor,
        container.role_permission_resolver.as_ref(),
    )
    .await
    {
        Ok(value) => response::success(StatusCode::OK, value),
        Err(error) => error.into_response(),
    }
}
pub(in crate::rbac) async fn get_role(
    State(container): State<Arc<Container>>,
    claims: AccessClaims,
    Path(id): Path<i64>,
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
    match usecase::get(
        &container.database,
        &actor,
        container.role_permission_resolver.as_ref(),
        RoleId(id),
    )
    .await
    {
        Ok(value) => response::success(
            StatusCode::OK,
            serde_json::json!({"role": value.0, "permissions": value.1}),
        ),
        Err(error) => error.into_response(),
    }
}
