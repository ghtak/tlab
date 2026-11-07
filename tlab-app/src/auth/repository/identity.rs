use crate::{
    app::db::Context,
    auth::entity::{Identity, NewIdentity},
};

pub async fn find_local_by_email(
    ctx: &mut Context<'_>,
    email: &str,
) -> tlab::Result<Option<Identity>> {
    Ok(sqlx::query_as(
        "SELECT * FROM tlab_account_identity WHERE provider = 'LOCAL' AND lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(ctx.backend())
    .await?)
}

pub async fn insert(ctx: &mut Context<'_>, entity: &NewIdentity) -> tlab::Result<Identity> {
    Ok(sqlx::query_as(
        "INSERT INTO tlab_account_identity \
         (account_id, priority, provider, provider_subject, email, email_verified_at, password_hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(entity.account_id)
    .bind(entity.priority)
    .bind(entity.provider)
    .bind(&entity.provider_subject)
    .bind(&entity.email)
    .bind(entity.email_verified_at)
    .bind(&entity.password_hash)
    .fetch_one(ctx.backend())
    .await?)
}
