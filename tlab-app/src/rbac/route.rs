use std::sync::Arc;

use axum::{
    Router,
    routing::{get, put},
};

use super::slice;
use crate::app::Container;

/// Relative paths let JSON and cookie transports share the same handlers.
pub(crate) fn router() -> Router<Arc<Container>> {
    Router::new()
        .route(
            "/rbac/permissions",
            get(slice::list_permissions::list_permissions),
        )
        .route(
            "/rbac/roles",
            get(slice::list_roles::list_roles).post(slice::create_role::create_role),
        )
        .route(
            "/rbac/roles/{id}",
            get(slice::list_roles::get_role)
                .patch(slice::update_role::update_role)
                .delete(slice::delete_role::delete_role),
        )
        .route(
            "/rbac/roles/{id}/permissions",
            put(slice::replace_role_permissions::replace_role_permissions),
        )
        .route(
            "/accounts/{account_id}/roles",
            get(slice::list_account_roles::list_account_roles),
        )
        .route(
            "/accounts/{account_id}/roles/{role_id}",
            put(slice::grant_account_role::grant_account_role)
                .delete(slice::revoke_account_role::revoke_account_role),
        )
        .route(
            "/accounts/{account_id}/permissions",
            get(slice::get_effective_permissions::get_effective_permissions),
        )
}
