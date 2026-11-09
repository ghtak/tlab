use super::super::service::{authorize, locked};
use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    rbac::{Actor, NewRole, Role, RolePermissionResolver, repository::role},
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
    value: NewRole,
) -> AppResult<Role> {
    authorize(actor, resolver, "rbac.role.manage").await?;
    if value.name.trim().is_empty() || value.name.contains('\0') || value.description.contains('\0')
    {
        return Err(AppError::new(
            AppErrorCode::RbacInvalidInput,
            "invalid role input",
            "validate_input",
        ));
    }
    let mut tx = db
        .tx()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    let result = {
        let mut ctx = tx.context();
        locked(&mut ctx, actor, None).await?;
        role::insert(&mut ctx, &value).await.map_err(|error| {
                if matches!(&error, tlab::Error::DbError(sqlx::Error::Database(db)) if db.is_unique_violation()) {
                    AppError::new(AppErrorCode::RbacConflict, "role name already exists", "write_role")
                } else {
                    AppError::internal("write_role", "role management failed", error)
                }
            })?
    };
    tx.commit()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    Ok(result)
}
