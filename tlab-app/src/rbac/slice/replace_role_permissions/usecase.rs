use super::super::service::{authorize, locked};
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    rbac::{
        Actor, NewRolePermission, PermissionSet, Role, RoleId, RolePermissionResolver,
        check_role_edit,
        repository::{role, role_permission},
    },
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    id: RoleId,
    expected: i64,
    after: PermissionSet,
) -> AppResult<Role> {
    authorize(actor, resolver, "rbac.role.manage").await?;
    let mut tx = db
        .tx()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    let result = {
        let mut ctx = tx.context();
        let (actor_permissions, target) = locked(&mut ctx, actor, Some(id)).await?;
        let target = target.expect("target checked");
        if target.version != expected {
            return Err(AppError::new(
                AppErrorCode::RbacConflict,
                "role conflict",
                "role_version",
            ));
        }
        let before = role_permission::list_for_role(&mut ctx, id)
            .await
            .map_err(|error| {
                AppError::internal("rbac_management", "role management failed", error)
            })?;
        if !check_role_edit(&actor_permissions, &before, &after) {
            return Err(AppError::new(
                AppErrorCode::RbacForbidden,
                "permission denied",
                "authorize",
            ));
        }
        if before == after {
            target
        } else {
            for code in before.difference(&after).iter() {
                role_permission::delete(&mut ctx, id, code)
                    .await
                    .map_err(|error| {
                        AppError::internal("rbac_write", "role management failed", error)
                    })?;
            }
            for code in after.difference(&before).iter() {
                role_permission::insert(
                    &mut ctx,
                    &NewRolePermission {
                        role_id: id,
                        permission_code: code.clone(),
                    },
                )
                .await
                .map_err(|error| {
                    AppError::internal("rbac_write", "role management failed", error)
                })?;
            }
            role::advance_version(&mut ctx, id, expected)
                .await
                .map_err(|error| AppError::internal("rbac_write", "role management failed", error))?
                .ok_or_else(|| {
                    AppError::new(AppErrorCode::RbacConflict, "role conflict", "role_version")
                })?
        }
    };
    tx.commit()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    Ok(result)
}
