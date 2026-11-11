use super::super::service::authorize;
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::{account_allowed, find_account},
    rbac::{
        Actor, PermissionCode, RoleId, RolePermissionResolver,
        repository::{account_role, role},
        require_in_set, resolve_in_context,
    },
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    account_id: i64,
    role_id: RoleId,
) -> AppResult<()> {
    authorize(actor, resolver, "rbac.assignment.manage").await?;
    let mut tx = db.tx().await.map_err(|error| {
        AppError::internal("assignment_transaction", "role assignment failed", error)
    })?;
    {
        let mut ctx = tx.context();
        let actor_account = find_account(&mut ctx, actor.account_id)
            .await
            .map_err(|error| AppError::internal("find_account", "role assignment failed", error))?;
        if !actor_account.as_ref().is_some_and(account_allowed) {
            return Err(AppError::new(
                AppErrorCode::RbacForbidden,
                "permission denied",
                "actor_status",
            ));
        }
        let target = if account_id == actor.account_id {
            actor_account
        } else {
            find_account(&mut ctx, account_id).await.map_err(|error| {
                AppError::internal("find_account", "role assignment failed", error)
            })?
        };
        if target.is_none() {
            return Err(AppError::new(
                AppErrorCode::RbacNotFound,
                "account not found",
                "find_account",
            ));
        }
        let mut ids = actor.role_ids.as_slice().to_vec();
        ids.push(role_id);
        let roles = role::lock_by_ids(&mut ctx, &ids)
            .await
            .map_err(|error| AppError::internal("lock_roles", "role assignment failed", error))?;
        // A new statement after lock waits observes committed permission edits.
        let permissions = resolve_in_context(&mut ctx, &actor.role_ids)
            .await
            .map_err(|error| {
                AppError::internal("resolve_permissions", "role assignment failed", error)
            })?;
        if !require_in_set(
            &permissions,
            &PermissionCode("rbac.assignment.manage".into()),
        ) {
            return Err(AppError::new(
                AppErrorCode::RbacForbidden,
                "permission denied",
                "authorize",
            ));
        }
        if !roles.iter().any(|role| role.id == role_id) {
            return Err(AppError::new(
                AppErrorCode::RbacNotFound,
                "role not found",
                "find_role",
            ));
        }
        account_role::delete(&mut ctx, account_id, role_id)
            .await
            .map_err(|error| {
                AppError::internal("write_assignment", "role assignment failed", error)
            })?;
    }
    tx.commit().await.map_err(|error| {
        AppError::internal("assignment_commit", "role assignment failed", error)
    })?;
    Ok(())
}
