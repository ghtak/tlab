use super::super::service::authorize;
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::find_account,
    rbac::{Actor, PermissionSet, RolePermissionResolver, list_role_ids, resolve_in_context},
};

/// Display current DB assignments, which may differ from this Access token.
pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    account_id: i64,
) -> AppResult<PermissionSet> {
    if account_id != actor.account_id {
        authorize(actor, resolver, "rbac.read").await?;
    }
    let mut conn = db.conn().await.map_err(|error| {
        AppError::internal("read_account_access", "access lookup failed", error)
    })?;
    find_account(&mut conn.context(), account_id)
        .await
        .map_err(|error| AppError::internal("find_account", "access lookup failed", error))?
        .ok_or_else(|| {
            AppError::new(
                AppErrorCode::RbacNotFound,
                "account not found",
                "find_account",
            )
        })?;
    let ids = list_role_ids(&mut conn.context(), account_id)
        .await
        .map_err(|error| AppError::internal("list_account_roles", "access lookup failed", error))?;
    resolve_in_context(&mut conn.context(), &ids)
        .await
        .map_err(|error| AppError::internal("resolve_permissions", "access lookup failed", error))
}
