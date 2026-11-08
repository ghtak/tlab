//! Storage only. Callers own transactions, account-before-refresh locking and policy.
#![allow(
    dead_code,
    reason = "Refresh storage is connected by later authentication slices"
)]

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    app::db::Context,
    auth::{
        entity::{NewRefreshToken, RefreshToken, RevokeReason},
        token::RefreshHash,
    },
};

pub async fn insert(ctx: &mut Context<'_>, value: &NewRefreshToken) -> tlab::Result<RefreshToken> {
    Ok(sqlx::query_as("INSERT INTO tlab_auth_refresh_token (session_id, account_id, token_hash, session_created_at, session_expires_at, created_at, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *")
        .bind(value.session_id).bind(value.account_id).bind(value.token_hash.as_bytes().as_slice())
        .bind(value.session_created_at).bind(value.session_expires_at).bind(value.created_at).bind(value.expires_at)
        .fetch_one(ctx.backend()).await?)
}

/// Initial lookup only; after account lock, lock and re-read the row.
pub async fn find_by_hash(
    ctx: &mut Context<'_>,
    hash: &RefreshHash,
) -> tlab::Result<Option<RefreshToken>> {
    Ok(
        sqlx::query_as("SELECT * FROM tlab_auth_refresh_token WHERE token_hash = $1")
            .bind(hash.as_bytes().as_slice())
            .fetch_optional(ctx.backend())
            .await?,
    )
}

/// Call only after locking the account in the same transaction.
pub async fn lock_by_hash(
    ctx: &mut Context<'_>,
    hash: &RefreshHash,
) -> tlab::Result<Option<RefreshToken>> {
    Ok(
        sqlx::query_as("SELECT * FROM tlab_auth_refresh_token WHERE token_hash = $1 FOR UPDATE")
            .bind(hash.as_bytes().as_slice())
            .fetch_optional(ctx.backend())
            .await?,
    )
}

/// Returns the number newly revoked; already revoked rows remain unchanged.
pub async fn revoke_session(
    ctx: &mut Context<'_>,
    account_id: i64,
    session_id: Uuid,
    at: OffsetDateTime,
    reason: RevokeReason,
) -> tlab::Result<u64> {
    Ok(sqlx::query("UPDATE tlab_auth_refresh_token SET revoked_at=$3, revoke_reason=$4 WHERE account_id=$1 AND session_id=$2 AND revoked_at IS NULL")
        .bind(account_id).bind(session_id).bind(at).bind(reason).execute(ctx.backend()).await?.rows_affected())
}

/// Call after account lock in the same transaction; includes expired rows.
pub async fn revoke_account(
    ctx: &mut Context<'_>,
    account_id: i64,
    at: OffsetDateTime,
    reason: RevokeReason,
) -> tlab::Result<u64> {
    Ok(sqlx::query("UPDATE tlab_auth_refresh_token SET revoked_at=$2, revoke_reason=$3 WHERE account_id=$1 AND revoked_at IS NULL")
        .bind(account_id).bind(at).bind(reason).execute(ctx.backend()).await?.rows_affected())
}

#[cfg(test)]
mod tests;
