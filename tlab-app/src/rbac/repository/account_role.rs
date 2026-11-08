use crate::{
    app::db::Context,
    rbac::{AccountRole, NewAccountRole, RoleId, RoleIds},
};
use sqlx::types::time::OffsetDateTime;
use tlab::Result;

pub(crate) async fn list_role_ids(ctx: &mut Context<'_>, account_id: i64) -> Result<RoleIds> {
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT role_id FROM tlab_rbac_account_role WHERE account_id=$1 ORDER BY role_id",
    )
    .bind(account_id)
    .fetch_all(ctx.backend())
    .await?;
    RoleIds::try_from_raw(ids)
        .map_err(|_| tlab::Error::IllegalState("invalid stored RBAC data".into()))
}
/// Account lock and resulting assignment count policy are caller responsibilities.
/// None means the same assignment already exists; its grant time is unchanged.
pub(crate) async fn insert(
    ctx: &mut Context<'_>,
    value: &NewAccountRole,
) -> Result<Option<AccountRole>> {
    let row = sqlx::query_as::<_, (i64, i64, OffsetDateTime)>("INSERT INTO tlab_rbac_account_role (account_id, role_id) VALUES ($1,$2) ON CONFLICT (account_id,role_id) DO NOTHING RETURNING account_id,role_id,granted_at")
        .bind(value.account_id).bind(value.role_id.get()).fetch_optional(ctx.backend()).await?;
    row.map(|(account_id, role_id, granted_at)| {
        Ok(AccountRole {
            account_id,
            role_id: RoleId::try_from(role_id)
                .map_err(|_| tlab::Error::IllegalState("invalid stored RBAC data".into()))?,
            granted_at,
        })
    })
    .transpose()
}
pub(crate) async fn delete(
    ctx: &mut Context<'_>,
    account_id: i64,
    role_id: RoleId,
) -> Result<bool> {
    Ok(
        sqlx::query("DELETE FROM tlab_rbac_account_role WHERE account_id=$1 AND role_id=$2")
            .bind(account_id)
            .bind(role_id.get())
            .execute(ctx.backend())
            .await?
            .rows_affected()
            == 1,
    )
}
pub(crate) async fn count_for_account(ctx: &mut Context<'_>, account_id: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM tlab_rbac_account_role WHERE account_id=$1")
            .bind(account_id)
            .fetch_one(ctx.backend())
            .await?,
    )
}
pub(crate) async fn is_role_assigned(ctx: &mut Context<'_>, role_id: RoleId) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tlab_rbac_account_role WHERE role_id=$1)")
            .bind(role_id.get())
            .fetch_one(ctx.backend())
            .await?,
    )
}
