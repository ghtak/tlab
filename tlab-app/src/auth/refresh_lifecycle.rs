//! Refresh use cases. Only trusted server code may call account-wide revocation.
#![allow(
    dead_code,
    reason = "Connected by API and account state use cases in later steps"
)]
use crate::{
    app::{
        db::{Context, Database},
        error::InternalFailure,
    },
    auth::{
        entity::RevokeReason,
        repository::{account, refresh_token},
        session_policy,
        token::{AccessIssuer, AccessToken, RefreshHash},
    },
};
use sqlx::types::time::OffsetDateTime;

#[derive(Debug)]
#[allow(
    dead_code,
    reason = "The API layer will consume internal failure details"
)]
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

/// Caller must hold the account row lock in the same transaction. This lets a
/// trusted state-changing use case combine its update and revocation atomically.
pub async fn revoke_all_locked(
    ctx: &mut Context<'_>,
    account_id: i64,
    at: OffsetDateTime,
    reason: RevokeReason,
) -> tlab::Result<u64> {
    refresh_token::revoke_account(ctx, account_id, at, reason).await
}

pub struct RefreshLifecycle;

impl RefreshLifecycle {
    pub async fn refresh(
        database: &Database,
        raw: &str,
        issuer: &AccessIssuer,
    ) -> Result<AccessToken, Error> {
        let mut tx = database
            .tx()
            .await
            .map_err(|e| internal("refresh_transaction", e))?;
        // A concurrent revocation may commit after this read. That in-flight
        // refresh may still issue one Access; later reads observe the revocation.
        let row = refresh_token::find_by_hash(&mut tx.context(), &RefreshHash::of(raw))
            .await
            .map_err(|e| internal("find_refresh", e))?
            .ok_or(Error::InvalidRefresh)?;
        let account = account::find_by_id(&mut tx.context(), row.account_id)
            .await
            .map_err(|e| internal("find_account", e))?
            .ok_or(Error::InvalidRefresh)?;
        let now = OffsetDateTime::now_utc();
        if !session_policy::allowed(&account)
            || row.revoked_at.is_some()
            || now >= row.expires_at
            || now >= row.session_expires_at
        {
            return Err(Error::InvalidRefresh);
        }
        let timestamp = u64::try_from(now.unix_timestamp())
            .map_err(|e| internal("refresh_clock", tlab::Error::Internal(e.into())))?;
        let access = issuer
            .issue(
                &row.account_id.to_string(),
                row.session_id,
                session_policy::user_claim(&account),
                timestamp,
            )
            .map_err(|e| internal("access_sign", tlab::Error::Internal(e.into())))?;
        tx.commit()
            .await
            .map_err(|e| internal("refresh_commit", e))?;
        Ok(access)
    }

    /// An existing token identifies its session even if expired or already revoked.
    pub async fn logout(database: &Database, raw: &str) -> Result<(), Error> {
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
}

#[cfg(test)]
mod tests;
