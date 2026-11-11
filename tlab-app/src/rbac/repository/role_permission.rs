use crate::{
    app::db::{Context, QueryBuilder},
    rbac::{NewRolePermission, PermissionCode, PermissionSet, RoleId, RoleIds},
};
use tlab::Result;

pub(crate) async fn list_for_role(ctx: &mut Context<'_>, role_id: RoleId) -> Result<PermissionSet> {
    let codes = sqlx::query_scalar::<_, PermissionCode>(
        "SELECT permission_code FROM tlab_rbac_role_permission WHERE role_id=$1 ORDER BY permission_code COLLATE \"C\"",
    ).bind(role_id.get()).fetch_all(ctx.backend()).await?;
    Ok(PermissionSet::from_codes(codes))
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
    let codes = sqlx::query_scalar::<_, PermissionCode>(
        "SELECT DISTINCT permission_code COLLATE \"C\" FROM tlab_rbac_role_permission WHERE role_id = ANY($1) ORDER BY permission_code COLLATE \"C\"",
    ).bind(ids).fetch_all(ctx.backend()).await?;
    Ok(PermissionSet::from_codes(codes))
}
/// Link writes do not change the role version; caller composes them with
/// advance_version in one transaction after checking the expected version.
pub(crate) async fn insert(ctx: &mut Context<'_>, value: &NewRolePermission) -> Result<bool> {
    Ok(sqlx::query("INSERT INTO tlab_rbac_role_permission (role_id,permission_code) VALUES ($1,$2) ON CONFLICT (role_id,permission_code) DO NOTHING")
        .bind(value.role_id.get()).bind(value.permission_code.as_str()).execute(ctx.backend()).await?.rows_affected() == 1)
}
pub(crate) async fn insert_many(
    ctx: &mut Context<'_>,
    values: &[NewRolePermission],
) -> Result<u64> {
    let mut inserted = 0;
    // PostgreSQL supports u16::MAX bind parameters; each row uses two.
    for batch in values.chunks(usize::from(u16::MAX) / 2) {
        let mut query =
            QueryBuilder::new("INSERT INTO tlab_rbac_role_permission (role_id, permission_code) ");
        query.push_values(batch, |mut row, value| {
            row.push_bind(value.role_id.get())
                .push_bind(value.permission_code.as_str());
        });
        query.push(" ON CONFLICT (role_id, permission_code) DO NOTHING");
        inserted += query.build().execute(ctx.backend()).await?.rows_affected();
    }
    Ok(inserted)
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
