use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode},
    },
    auth::{
        repository::{account, refresh_token},
        session_policy,
        token::{AccessIssuer, AccessToken, RefreshHash},
    },
};
use chrono::Utc;

pub async fn execute(
    database: &Database,
    raw: &str,
    issuer: &AccessIssuer,
) -> Result<AccessToken, AppError> {
    let mut conn = database
        .conn()
        .await
        .map_err(|e| AppError::internal("refresh_conn", "refresh operation failed", e))?;
    // A concurrent revocation may commit after this read. That in-flight
    // refresh may still issue one Access; later reads observe the revocation.
    let row = refresh_token::find_by_hash(&mut conn.context(), &RefreshHash::of(raw))
        .await
        .map_err(|e| AppError::internal("find_refresh", "refresh operation failed", e))?
        .ok_or(AppError::new(
            AppErrorCode::InvalidRefreshToken,
            "refresh token rejected",
            "validate_refresh",
        ))?;
    let account = account::find_by_id(&mut conn.context(), row.account_id)
        .await
        .map_err(|e| AppError::internal("find_account", "refresh operation failed", e))?
        .ok_or(AppError::new(
            AppErrorCode::InvalidRefreshToken,
            "refresh token rejected",
            "validate_refresh",
        ))?;
    let now = Utc::now();
    if !session_policy::allowed(&account)
        || row.revoked_at.is_some()
        || now >= row.expires_at
        || now >= row.session_expires_at
    {
        return Err(AppError::new(
            AppErrorCode::InvalidRefreshToken,
            "refresh token rejected",
            "validate_refresh",
        ));
    }
    let role_ids = crate::rbac::list_role_ids(&mut conn.context(), account.id)
        .await
        .map_err(|e| AppError::internal("list_role_ids", "refresh operation failed", e))?;
    let timestamp = u64::try_from(now.timestamp()).map_err(|e| {
        AppError::internal(
            "refresh_clock",
            "refresh operation failed",
            tlab::Error::Internal(e.into()),
        )
    })?;
    let access = issuer
        .issue(
            &row.account_id.to_string(),
            row.session_id,
            session_policy::user_claim(&account, &role_ids),
            timestamp,
        )
        .map_err(|e| {
            AppError::internal(
                "access_sign",
                "refresh operation failed",
                tlab::Error::Internal(e.into()),
            )
        })?;
    Ok(access)
}
