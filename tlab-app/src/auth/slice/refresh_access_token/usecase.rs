use crate::{
    app::{db::Database, error::InternalFailure},
    auth::{
        repository::{account, refresh_token},
        session_policy,
        token::{AccessIssuer, AccessToken, RefreshHash},
    },
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

pub async fn execute(
    database: &Database,
    raw: &str,
    issuer: &AccessIssuer,
) -> Result<AccessToken, Error> {
    let mut conn = database
        .conn()
        .await
        .map_err(|e| internal("refresh_conn", e))?;
    // A concurrent revocation may commit after this read. That in-flight
    // refresh may still issue one Access; later reads observe the revocation.
    let row = refresh_token::find_by_hash(&mut conn.context(), &RefreshHash::of(raw))
        .await
        .map_err(|e| internal("find_refresh", e))?
        .ok_or(Error::InvalidRefresh)?;
    let account = account::find_by_id(&mut conn.context(), row.account_id)
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
    Ok(access)
}
