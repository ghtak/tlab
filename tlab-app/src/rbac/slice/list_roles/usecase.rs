use super::super::service::authorize;
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    rbac::{
        Actor, PermissionSet, Role, RoleId, RolePermissionResolver, repository::role,
        repository::role_permission,
    },
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
) -> AppResult<Vec<Role>> {
    authorize(actor, resolver, "rbac.read").await?;
    let mut conn = db
        .conn()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    role::list_all(&mut conn.context())
        .await
        .map_err(|error| AppError::internal("list_roles", "role management failed", error))
}

pub(crate) async fn get(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    id: RoleId,
) -> AppResult<(Role, PermissionSet)> {
    authorize(actor, resolver, "rbac.read").await?;
    let mut tx = db
        .tx()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    let result = {
        let mut ctx = tx.context();
        let role = role::lock_by_id(&mut ctx, id)
            .await
            .map_err(|error| {
                AppError::internal("rbac_management", "role management failed", error)
            })?
            .ok_or_else(|| {
                AppError::new(AppErrorCode::RbacNotFound, "role not found", "find_role")
            })?;
        let permissions = role_permission::list_for_role(&mut ctx, id)
            .await
            .map_err(|error| {
                AppError::internal("rbac_management", "role management failed", error)
            })?;
        (role, permissions)
    };
    tx.commit()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    Ok(result)
}
