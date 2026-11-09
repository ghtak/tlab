use super::super::service::{authorize, locked};
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    rbac::{
        Actor, RoleId, RolePermissionResolver,
        repository::{account_role, role},
    },
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    id: RoleId,
    expected: i64,
) -> AppResult<()> {
    authorize(actor, resolver, "rbac.role.manage").await?;
    let mut tx = db
        .tx()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    {
        let mut ctx = tx.context();
        let (_, target) = locked(&mut ctx, actor, Some(id)).await?;
        if target.expect("target checked").version != expected {
            return Err(AppError::new(
                AppErrorCode::RbacConflict,
                "role conflict",
                "role_version",
            ));
        }
        if account_role::is_role_assigned(&mut ctx, id)
            .await
            .map_err(|error| {
                AppError::internal("rbac_management", "role management failed", error)
            })?
        {
            return Err(AppError::new(
                AppErrorCode::RbacConflict,
                "role conflict",
                "role_version",
            ));
        }
        if !role::delete(&mut ctx, id, expected)
            .await
            .map_err(|error| AppError::internal("rbac_write", "role management failed", error))?
        {
            return Err(AppError::new(
                AppErrorCode::RbacConflict,
                "role conflict",
                "role_version",
            ));
        }
    }
    tx.commit()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    Ok(())
}
