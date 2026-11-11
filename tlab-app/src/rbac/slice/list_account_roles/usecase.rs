use super::super::service::authorize;
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::find_account,
    rbac::{Actor, RoleIds, RolePermissionResolver, list_role_ids},
};

/// Display current DB assignments, which may differ from this Access token.
pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    account_id: i64,
) -> AppResult<RoleIds> {
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
    list_role_ids(&mut conn.context(), account_id)
        .await
        .map_err(|error| AppError::internal("list_account_roles", "access lookup failed", error))
}
