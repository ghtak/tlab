use super::usecase;
use crate::{
    app::{Container, response},
    auth::jwt::AccessClaims,
    rbac::Actor,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
pub(in crate::rbac) async fn list_permissions(
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
        Ok(value) => response::success(
            StatusCode::OK,
            value
                .into_iter()
                .map(|item| serde_json::json!({"code": item.code, "description": item.description}))
                .collect::<Vec<_>>(),
        ),
        Err(error) => error.into_response(),
    }
}
