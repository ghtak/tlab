use crate::{
    app::db::Context,
    rbac::{Permission, PermissionCode},
};
use tlab::Result;

/// The catalog is deployment-owned; runtime storage exposes no catalog writes.
pub(crate) async fn list(ctx: &mut Context<'_>) -> Result<Vec<Permission>> {
    Ok(sqlx::query_as(
        "SELECT code, description FROM tlab_rbac_permission ORDER BY code COLLATE \"C\"",
    )
    .fetch_all(ctx.backend())
    .await?)
}
pub(crate) async fn find_by_code(
    ctx: &mut Context<'_>,
    code: &PermissionCode,
) -> Result<Option<Permission>> {
    Ok(
        sqlx::query_as("SELECT code, description FROM tlab_rbac_permission WHERE code=$1")
            .bind(code.as_str())
            .fetch_optional(ctx.backend())
            .await?,
    )
}
