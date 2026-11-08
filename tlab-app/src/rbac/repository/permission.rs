use crate::{
    app::db::Context,
    rbac::{Permission, PermissionCode},
};
use tlab::Result;

#[derive(sqlx::FromRow)]
struct Row {
    code: String,
    description: String,
}
fn decode(row: Row) -> Result<Permission> {
    Ok(Permission {
        code: PermissionCode::try_from(row.code.as_str())
            .map_err(|_| tlab::Error::IllegalState("invalid stored RBAC data".into()))?,
        description: row.description,
    })
}
/// The catalog is deployment-owned; runtime storage exposes no catalog writes.
pub(crate) async fn list(ctx: &mut Context<'_>) -> Result<Vec<Permission>> {
    sqlx::query_as::<_, Row>(
        "SELECT code, description FROM tlab_rbac_permission ORDER BY code COLLATE \"C\"",
    )
    .fetch_all(ctx.backend())
    .await
    .map_err(tlab::Error::from)?
    .into_iter()
    .map(decode)
    .collect()
}
pub(crate) async fn find_by_code(
    ctx: &mut Context<'_>,
    code: &PermissionCode,
) -> Result<Option<Permission>> {
    sqlx::query_as::<_, Row>("SELECT code, description FROM tlab_rbac_permission WHERE code=$1")
        .bind(code.as_str())
        .fetch_optional(ctx.backend())
        .await
        .map_err(tlab::Error::from)?
        .map(decode)
        .transpose()
}
