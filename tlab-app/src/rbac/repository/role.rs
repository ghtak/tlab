use crate::app::db::Context;
use crate::rbac::{NewRole, Role, RoleId, RoleMetadata};
use sqlx::types::time::OffsetDateTime;
use tlab::Result;

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    name: String,
    description: String,
    version: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}
impl TryFrom<Row> for Role {
    type Error = tlab::Error;
    fn try_from(row: Row) -> std::result::Result<Self, Self::Error> {
        let invalid = || tlab::Error::IllegalState("invalid stored RBAC role".into());
        if row.version <= 0 {
            return Err(invalid());
        }
        Ok(Self {
            id: RoleId::try_from(row.id).map_err(|_| invalid())?,
            name: row.name,
            description: row.description,
            version: row.version,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
pub(crate) async fn insert(ctx: &mut Context<'_>, value: &NewRole) -> Result<Role> {
    sqlx::query_as::<_, Row>(
        "INSERT INTO tlab_rbac_role (name, description) VALUES ($1, $2) RETURNING *",
    )
    .bind(&value.name)
    .bind(&value.description)
    .fetch_one(ctx.backend())
    .await?
    .try_into()
}
pub(crate) async fn find_by_id(ctx: &mut Context<'_>, id: RoleId) -> Result<Option<Role>> {
    sqlx::query_as::<_, Row>("SELECT * FROM tlab_rbac_role WHERE id = $1")
        .bind(id.get())
        .fetch_optional(ctx.backend())
        .await?
        .map(TryInto::try_into)
        .transpose()
}
/// Use only in a caller-owned transaction, after account locks.
pub(crate) async fn lock_by_id(ctx: &mut Context<'_>, id: RoleId) -> Result<Option<Role>> {
    sqlx::query_as::<_, Row>("SELECT * FROM tlab_rbac_role WHERE id = $1 FOR UPDATE")
        .bind(id.get())
        .fetch_optional(ctx.backend())
        .await?
        .map(TryInto::try_into)
        .transpose()
}
/// Locks the full actor/target union in ascending order.
/// Missing IDs are omitted. The caller decides whether they are stale or errors.
pub(crate) async fn lock_by_ids(ctx: &mut Context<'_>, ids: &[RoleId]) -> Result<Vec<Role>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut roles = Vec::new();
    for id in ids {
        if let Some(role) = lock_by_id(ctx, id).await? {
            roles.push(role);
        }
    }
    Ok(roles)
}
pub(crate) async fn list(
    ctx: &mut Context<'_>,
    after: Option<RoleId>,
    limit: u32,
) -> Result<Vec<Role>> {
    if !(1..=100).contains(&limit) {
        return Err(tlab::Error::InvalidInput(
            "role list limit must be between 1 and 100".into(),
        ));
    }
    sqlx::query_as::<_, Row>("SELECT * FROM tlab_rbac_role WHERE id > $1 ORDER BY id LIMIT $2")
        .bind(after.map_or(0, RoleId::get))
        .bind(i64::from(limit))
        .fetch_all(ctx.backend())
        .await?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}
/// None means absent or version mismatch; the usecase distinguishes under lock.
/// A metadata no-op keeps version and updated_at intact.
pub(crate) async fn update_metadata(
    ctx: &mut Context<'_>,
    id: RoleId,
    expected_version: i64,
    value: &RoleMetadata,
) -> Result<Option<Role>> {
    sqlx::query_as::<_, Row>("UPDATE tlab_rbac_role SET name=$3, description=$4, version=version + CASE WHEN (name, description) IS DISTINCT FROM ($3, $4) THEN 1 ELSE 0 END, updated_at=CASE WHEN (name, description) IS DISTINCT FROM ($3, $4) THEN CURRENT_TIMESTAMP ELSE updated_at END WHERE id=$1 AND version=$2 RETURNING *")
        .bind(id.get()).bind(expected_version).bind(&value.name).bind(&value.description)
        .fetch_optional(ctx.backend()).await?.map(TryInto::try_into).transpose()
}
/// Call once for an actual permission change, in the same tx as link writes.
pub(crate) async fn advance_version(
    ctx: &mut Context<'_>,
    id: RoleId,
    expected_version: i64,
) -> Result<Option<Role>> {
    sqlx::query_as::<_, Row>("UPDATE tlab_rbac_role SET version=version+1, updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND version=$2 RETURNING *")
        .bind(id.get()).bind(expected_version).fetch_optional(ctx.backend()).await?.map(TryInto::try_into).transpose()
}
pub(crate) async fn delete(
    ctx: &mut Context<'_>,
    id: RoleId,
    expected_version: i64,
) -> Result<bool> {
    Ok(
        sqlx::query("DELETE FROM tlab_rbac_role WHERE id=$1 AND version=$2")
            .bind(id.get())
            .bind(expected_version)
            .execute(ctx.backend())
            .await?
            .rows_affected()
            == 1,
    )
}
