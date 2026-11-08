use crate::{
    app::{db::Database, error::InternalFailure},
    auth::{entity::RevokeReason, repository::refresh_token, token::RefreshHash},
};
use sqlx::types::time::OffsetDateTime;

#[derive(Debug)]
pub enum Error {
    InvalidRefresh,
    Internal(InternalFailure),
}

fn internal(stage: &'static str, cause: impl Into<tlab::Error>) -> Error {
    Error::Internal(InternalFailure {
        stage,
        cause: cause.into(),
    })
}

/// An existing token identifies its session even if expired or already revoked.
pub async fn execute(database: &Database, raw: &str) -> Result<(), Error> {
    let mut tx = database
        .tx()
        .await
        .map_err(|e| internal("logout_transaction", e))?;
    let row = refresh_token::find_by_hash(&mut tx.context(), &RefreshHash::of(raw))
        .await
        .map_err(|e| internal("find_refresh", e))?
        .ok_or(Error::InvalidRefresh)?;
    refresh_token::revoke_session(
        &mut tx.context(),
        row.account_id,
        row.session_id,
        OffsetDateTime::now_utc(),
        RevokeReason::Logout,
    )
    .await
    .map_err(|e| internal("logout_revoke", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("logout_commit", e))?;
    Ok(())
}
