use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode},
    },
    auth::{entity::RevokeReason, repository::refresh_token, token::RefreshHash},
};
use sqlx::types::time::OffsetDateTime;

/// An existing token identifies its session even if expired or already revoked.
pub async fn execute(database: &Database, raw: &str) -> Result<(), AppError> {
    let mut tx = database
        .tx()
        .await
        .map_err(|e| AppError::internal("logout_transaction", "logout failed", e))?;
    let row = refresh_token::find_by_hash(&mut tx.context(), &RefreshHash::of(raw))
        .await
        .map_err(|e| AppError::internal("find_refresh", "logout failed", e))?
        .ok_or(AppError::new(
            AppErrorCode::InvalidRefreshToken,
            "refresh token rejected",
            "validate_refresh",
        ))?;
    refresh_token::revoke_session(
        &mut tx.context(),
        row.account_id,
        row.session_id,
        OffsetDateTime::now_utc(),
        RevokeReason::Logout,
    )
    .await
    .map_err(|e| AppError::internal("logout_revoke", "logout failed", e))?;
    tx.commit()
        .await
        .map_err(|e| AppError::internal("logout_commit", "logout failed", e))?;
    Ok(())
}
