use chrono::Timelike;
use std::sync::Arc;

use email_address::{EmailAddress, Options};
use serde::Deserialize;
use tlab::password::PasswordHasher;
use tokio::sync::{Semaphore, TryAcquireError};

use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode},
    },
    auth::{
        entity::{Account, NewRefreshToken},
        jwt::AccessToken,
        repository::{account, identity, refresh_token},
        token::RefreshToken,
    },
};

// No Debug: the command owns a plaintext password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub email: String,
    pub password: String,
}

/// Token responses are returned only after the transaction commits.
#[allow(dead_code, reason = "Token response is connected in API step 6")]
pub struct LoginTokens {
    pub account: Account,
    pub access: AccessToken,
    pub refresh: RefreshToken,
}

pub struct LocalLogin {
    hasher: Arc<dyn PasswordHasher>,
    slots: Arc<Semaphore>,
    dummy_password_hash: Arc<str>,
    access_issuer: Arc<crate::auth::jwt::AccessIssuer>,
}

impl LocalLogin {
    pub async fn new(
        hasher: Arc<dyn PasswordHasher>,
        slots: Arc<Semaphore>,
        access_issuer: Arc<crate::auth::jwt::AccessIssuer>,
    ) -> tlab::Result<Self> {
        let dummy_hasher = hasher.clone();
        let dummy_password_hash =
            tokio::task::spawn_blocking(move || dummy_hasher.hash("local-login-dummy-password"))
                .await
                .map_err(|error| tlab::Error::Internal(error.into()))??
                .into();
        Ok(Self {
            hasher,
            slots,
            dummy_password_hash,
            access_issuer,
        })
    }

    /// Authenticate outside the lock, then serialize issuance with account changes.
    #[allow(dead_code, reason = "Token response is connected in API step 6")]
    pub async fn execute_with_tokens(
        &self,
        database: &Database,
        command: Command,
    ) -> Result<LoginTokens, AppError> {
        let authenticated = self.execute(database, command).await?;
        let mut tx = database
            .tx()
            .await
            .map_err(|e| AppError::internal("login_transaction", "local login failed", e))?;
        let account = account::lock_by_id(&mut tx.context(), authenticated.id)
            .await
            .map_err(|e| AppError::internal("lock_account", "local login failed", e))?
            .ok_or_else(|| {
                AppError::internal(
                    "lock_account",
                    "local login failed",
                    tlab::Error::IllegalState("identity account is missing".into()),
                )
            })?;
        if !crate::auth::session_policy::allowed(&account) {
            return Err(AppError::new(
                AppErrorCode::AccountUnavailable,
                "account is unavailable",
                "account_status",
            ));
        }
        let role_ids = crate::rbac::list_role_ids(&mut tx.context(), account.id)
            .await
            .map_err(|e| AppError::internal("list_role_ids", "local login failed", e))?;
        let now = chrono::Utc::now()
            .with_nanosecond(0)
            .expect("zero nanoseconds is valid");
        let timestamp = u64::try_from(now.timestamp()).map_err(|e| {
            AppError::internal(
                "login_clock",
                "local login failed",
                tlab::Error::Internal(e.into()),
            )
        })?;
        let expires = now + std::time::Duration::from_secs(168 * 3600);
        let sid = uuid::Uuid::new_v4();
        let refresh = RefreshToken::generate().map_err(|e| {
            AppError::internal(
                "refresh_generate",
                "local login failed",
                tlab::Error::Internal(e.into()),
            )
        })?;
        refresh_token::insert(
            &mut tx.context(),
            &NewRefreshToken {
                session_id: sid,
                account_id: account.id,
                token_hash: refresh.hash(),
                session_created_at: now,
                session_expires_at: expires,
                created_at: now,
                expires_at: expires,
            },
        )
        .await
        .map_err(|e| AppError::internal("refresh_insert", "local login failed", e))?;
        let user = crate::auth::session_policy::user_claim(&account, &role_ids);
        let access = self
            .access_issuer
            .issue(&account.id.to_string(), sid, user, timestamp)
            .map_err(|e| {
                AppError::internal(
                    "access_sign",
                    "local login failed",
                    tlab::Error::Internal(e.into()),
                )
            })?;
        let tokens = LoginTokens {
            account,
            access,
            refresh,
        };
        tx.commit()
            .await
            .map_err(|e| AppError::internal("login_commit", "local login failed", e))?;
        Ok(tokens)
    }

    pub async fn execute(
        &self,
        database: &Database,
        command: Command,
    ) -> Result<Account, AppError> {
        let email = command.email.trim();
        if EmailAddress::parse_with_options(email, Options::default().without_display_text())
            .is_err()
        {
            return Err(AppError::new(
                AppErrorCode::InvalidEmail,
                "email must be a single email address",
                "validate_input",
            ));
        }
        if command.password.is_empty() {
            return Err(AppError::new(
                AppErrorCode::PasswordRequired,
                "password must not be empty",
                "validate_input",
            ));
        }
        let identity = {
            let mut conn = database
                .conn()
                .await
                .map_err(|e| AppError::internal("identity_connection", "local login failed", e))?;
            identity::find_local_by_email(&mut conn.context(), email)
                .await
                .map_err(|e| AppError::internal("find_identity", "local login failed", e))?
        };
        let encoded_hash = match identity.as_ref() {
            Some(identity) => identity
                .password_hash
                .as_deref()
                .ok_or_else(|| {
                    AppError::internal(
                        "password_hash",
                        "local login failed",
                        tlab::Error::IllegalState("LOCAL identity has no password hash".into()),
                    )
                })?
                .into(),
            None => self.dummy_password_hash.clone(),
        };
        let verified = verify_password(
            self.hasher.clone(),
            self.slots.clone(),
            command.password,
            encoded_hash,
        )
        .await?;
        let identity = identity.filter(|_| verified).ok_or(AppError::new(
            AppErrorCode::InvalidCredentials,
            "email or password is incorrect",
            "verify_credentials",
        ))?;
        let account = {
            let mut conn = database
                .conn()
                .await
                .map_err(|e| AppError::internal("account_connection", "local login failed", e))?;
            account::find_by_id(&mut conn.context(), identity.account_id)
                .await
                .map_err(|e| AppError::internal("find_account", "local login failed", e))?
        }
        .ok_or_else(|| {
            AppError::internal(
                "find_account",
                "local login failed",
                tlab::Error::IllegalState("identity account is missing".into()),
            )
        })?;
        if crate::auth::session_policy::allowed(&account) {
            Ok(account)
        } else {
            Err(AppError::new(
                AppErrorCode::AccountUnavailable,
                "account is unavailable",
                "account_status",
            ))
        }
    }
}

pub(super) async fn verify_password(
    hasher: Arc<dyn PasswordHasher>,
    slots: Arc<Semaphore>,
    password: String,
    encoded_hash: Arc<str>,
) -> Result<bool, AppError> {
    let permit = slots.try_acquire_owned().map_err(|error| match error {
        TryAcquireError::NoPermits => AppError::new(
            AppErrorCode::Busy,
            "password verification is busy",
            "hash_capacity",
        ),
        TryAcquireError::Closed => AppError::internal(
            "hash_capacity",
            "local login failed",
            tlab::Error::Internal(error.into()),
        ),
    })?;
    tokio::task::spawn_blocking(move || {
        // Cancellation must not release capacity while blocking work still runs.
        let _permit = permit;
        hasher.verify(&password, &encoded_hash)
    })
    .await
    .map_err(|e| {
        AppError::internal(
            "verify_task",
            "local login failed",
            tlab::Error::Internal(e.into()),
        )
    })?
    .map_err(|e| AppError::internal("password_verify", "local login failed", e))
}
