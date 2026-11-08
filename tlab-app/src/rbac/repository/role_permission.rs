use crate::{
    app::db::Context,
    rbac::{NewRolePermission, PermissionCode, PermissionSet, RoleId, RoleIds},
};
use tlab::Result;

pub(crate) async fn list_for_role(ctx: &mut Context<'_>, role_id: RoleId) -> Result<PermissionSet> {
    let codes = sqlx::query_scalar::<_, String>(
        "SELECT permission_code FROM tlab_rbac_role_permission WHERE role_id=$1 ORDER BY permission_code COLLATE \"C\"",
    ).bind(role_id.get()).fetch_all(ctx.backend()).await?;
    decode(codes)
}
/// One statement provides a consistent, deduplicated permission union.
pub(crate) async fn resolve_in_context(
    ctx: &mut Context<'_>,
    role_ids: &RoleIds,
) -> Result<PermissionSet> {
    if role_ids.as_slice().is_empty() {
        return Ok(PermissionSet::default());
    }
    let ids: Vec<_> = role_ids.as_slice().iter().map(|id| id.get()).collect();
    let codes = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT permission_code COLLATE \"C\" FROM tlab_rbac_role_permission WHERE role_id = ANY($1) ORDER BY permission_code COLLATE \"C\"",
    ).bind(ids).fetch_all(ctx.backend()).await?;
    decode(codes)
}
fn decode(codes: Vec<String>) -> Result<PermissionSet> {
    let codes = codes
        .iter()
        .map(|code| {
            PermissionCode::try_from(code.as_str())
                .map_err(|_| tlab::Error::IllegalState("invalid stored RBAC data".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PermissionSet::from_codes(codes))
}
/// Link writes do not change the role version; caller composes them with
/// advance_version in one transaction after checking the expected version.
pub(crate) async fn insert(ctx: &mut Context<'_>, value: &NewRolePermission) -> Result<bool> {
    Ok(sqlx::query("INSERT INTO tlab_rbac_role_permission (role_id,permission_code) VALUES ($1,$2) ON CONFLICT (role_id,permission_code) DO NOTHING")
        .bind(value.role_id.get()).bind(value.permission_code.as_str()).execute(ctx.backend()).await?.rows_affected() == 1)
}
pub(crate) async fn delete(
    ctx: &mut Context<'_>,
    role_id: RoleId,
    code: &PermissionCode,
) -> Result<bool> {
    Ok(
        sqlx::query(
            "DELETE FROM tlab_rbac_role_permission WHERE role_id=$1 AND permission_code=$2",
        )
        .bind(role_id.get())
        .bind(code.as_str())
        .execute(ctx.backend())
        .await?
        .rows_affected()
            == 1,
    )
}
pub(crate) async fn delete_for_role(ctx: &mut Context<'_>, role_id: RoleId) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM tlab_rbac_role_permission WHERE role_id=$1")
            .bind(role_id.get())
            .execute(ctx.backend())
            .await?
            .rows_affected(),
    )
}
