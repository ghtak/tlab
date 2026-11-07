use crate::{
    app::db::Context,
    auth::entity::{Account, NewAccount},
};

pub async fn find_by_id(ctx: &mut Context<'_>, id: i64) -> tlab::Result<Option<Account>> {
    Ok(sqlx::query_as("SELECT * FROM tlab_account WHERE id = $1")
        .bind(id)
        .fetch_optional(ctx.backend())
        .await?)
}

pub async fn insert(ctx: &mut Context<'_>, entity: &NewAccount) -> tlab::Result<Account> {
    Ok(sqlx::query_as(
        "INSERT INTO tlab_account (nickname, status, withdrawn_at) \
         VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(&entity.nickname)
    .bind(entity.status)
    .bind(entity.withdrawn_at)
    .fetch_one(ctx.backend())
    .await?)
}
