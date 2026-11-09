use super::super::service::authorize;
use crate::{
    app::{
        db::Database,
        error::{AppError, AppResult},
    },
    rbac::{Actor, Permission, RolePermissionResolver, repository::permission},
};

pub(crate) async fn execute(
    db: &Database,
    actor: &Actor,
    resolver: &dyn RolePermissionResolver,
) -> AppResult<Vec<Permission>> {
    authorize(actor, resolver, "rbac.read").await?;
    let mut conn = db
        .conn()
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))?;
    permission::list(&mut conn.context())
        .await
        .map_err(|error| AppError::internal("rbac_management", "role management failed", error))
}
