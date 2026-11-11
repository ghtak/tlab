use super::usecase;
use crate::{
    app::{Container, response},
    auth::token::AccessClaims,
    rbac::Actor,
    rbac::RoleId,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub(in crate::rbac) async fn grant_account_role(
    State(container): State<Arc<Container>>,
    claims: AccessClaims,
    Path((account_id, role_id)): Path<(i64, i64)>,
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
        account_id,
        RoleId(role_id),
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error.into_response(),
    }
}
