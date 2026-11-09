use crate::{
    app::{
        db::Context,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::{account_allowed, lock_account},
    rbac::{
        Actor, PermissionCode, PermissionSet, Role, RoleId, RolePermissionResolver,
        repository::role, require_in_set, require_permission, resolve_in_context,
    },
};

pub(super) async fn authorize(
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    required: &str,
) -> AppResult<()> {
    if require_permission(actor, &PermissionCode(required.into()), resolver)
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?
    {
        Ok(())
    } else {
        Err(AppError::new(
            AppErrorCode::RbacForbidden,
            "permission denied",
            "authorize",
        ))
    }
}
/// Account first, then all actor and target roles in ascending order. Never reuse
/// data read before the locks when making the final policy decision.
pub(super) async fn locked(
    ctx: &mut Context<'_>,
    actor: &Actor,
    target: Option<RoleId>,
) -> AppResult<(PermissionSet, Option<Role>)> {
    let account = lock_account(ctx, actor.account_id)
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    if !account.as_ref().is_some_and(account_allowed) {
        return Err(AppError::new(
            AppErrorCode::RbacForbidden,
            "permission denied",
            "authorize",
        ));
    }
    let mut ids = actor.role_ids.as_slice().to_vec();
    if let Some(id) = target {
        ids.push(id);
    }
    let roles = role::lock_by_ids(ctx, &ids)
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    let permissions = resolve_in_context(ctx, &actor.role_ids)
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    if !require_in_set(&permissions, &PermissionCode("rbac.role.manage".into())) {
        return Err(AppError::new(
            AppErrorCode::RbacForbidden,
            "permission denied",
            "authorize",
        ));
    }
    let target_role = target.and_then(|id| roles.into_iter().find(|r| r.id == id));
    if target.is_some() && target_role.is_none() {
        return Err(AppError::new(
            AppErrorCode::RbacNotFound,
            "role not found",
            "find_role",
        ));
    }
    Ok((permissions, target_role))
}
