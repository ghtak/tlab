use std::sync::Arc;

use email_address::{EmailAddress, Options};
use serde::Deserialize;
use tlab::password::PasswordHasher;
use tokio::sync::{Semaphore, TryAcquireError};

use crate::{
    app::{
        db::Database,
        error::{InternalFailure, InvalidInput},
    },
    auth::{
        entity::{Account, AccountStatus, NewRefreshToken},
        repository::{account, identity, refresh_token},
        token::{AccessIssuer, AccessToken, RefreshToken},
    },
};

// No Debug: the command owns a plaintext password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub email: String,
    pub password: String,
}

/// Secrets are exposed only to the caller after the transaction commits.
#[allow(dead_code, reason = "Token response is connected in API step 6")]
pub struct LoginTokens {
    pub account: Account,
    pub access: AccessToken,
    pub refresh: RefreshToken,
}

#[derive(Debug)]
pub enum Error {
    InvalidInput(InvalidInput),
    InvalidCredentials,
    AccountUnavailable,
    Busy,
    Internal(InternalFailure),
}

impl Error {
    fn internal(stage: &'static str, cause: impl Into<tlab::Error>) -> Self {
        Self::Internal(InternalFailure {
            stage,
            cause: cause.into(),
        })
    }
}

pub struct LocalLogin {
    hasher: Arc<dyn PasswordHasher>,
    slots: Arc<Semaphore>,
    dummy_password_hash: Arc<str>,
}

impl LocalLogin {
    /// Authenticate outside the lock, then serialize issuance with account changes.
    #[allow(dead_code, reason = "Token response is connected in API step 6")]
    pub async fn execute_with_tokens(
        &self,
        database: &Database,
        command: Command,
        issuer: &AccessIssuer,
    ) -> Result<LoginTokens, Error> {
        let authenticated = self.execute(database, command).await?;
        let mut tx = database
            .tx()
            .await
            .map_err(|e| Error::internal("login_transaction", e))?;
        let account = account::lock_by_id(&mut tx.context(), authenticated.id)
            .await
            .map_err(|e| Error::internal("lock_account", e))?
            .ok_or_else(|| {
                Error::internal(
                    "lock_account",
                    tlab::Error::IllegalState("identity account is missing".into()),
                )
            })?;
        if account.status == AccountStatus::Withdrawn {
            return Err(Error::AccountUnavailable);
        }
        let now = sqlx::types::time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .map_err(|e| Error::internal("login_clock", tlab::Error::Internal(e.into())))?;
        let timestamp = u64::try_from(now.unix_timestamp())
            .map_err(|e| Error::internal("login_clock", tlab::Error::Internal(e.into())))?;
        let expires = now + std::time::Duration::from_secs(168 * 3600);
        let sid = uuid::Uuid::new_v4();
        let refresh = RefreshToken::generate()
            .map_err(|e| Error::internal("refresh_generate", tlab::Error::Internal(e.into())))?;
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
        .map_err(|e| Error::internal("refresh_insert", e))?;
        let user = serde_json::json!({"nickname": account.nickname, "status": account.status})
            .as_object()
            .expect("object literal")
            .clone();
        let access = issuer
            .issue(&account.id.to_string(), sid, user, timestamp)
            .map_err(|e| Error::internal("access_sign", tlab::Error::Internal(e.into())))?;
        tx.commit()
            .await
            .map_err(|e| Error::internal("login_commit", e))?;
        Ok(LoginTokens {
            account,
            access,
            refresh,
        })
    }
    pub async fn new(hasher: Arc<dyn PasswordHasher>, slots: Arc<Semaphore>) -> tlab::Result<Self> {
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
        })
    }

    pub async fn execute(&self, database: &Database, command: Command) -> Result<Account, Error> {
        let email = command.email.trim();
        if EmailAddress::parse_with_options(email, Options::default().without_display_text())
            .is_err()
        {
            return Err(Error::InvalidInput(InvalidInput {
                field: "email",
                code: "INVALID_EMAIL",
                message: "email must be a single email address",
            }));
        }
        if command.password.is_empty() {
            return Err(Error::InvalidInput(InvalidInput {
                field: "password",
                code: "PASSWORD_REQUIRED",
                message: "password must not be empty",
            }));
        }
        let identity = {
            let mut conn = database
                .conn()
                .await
                .map_err(|e| Error::internal("identity_connection", e))?;
            identity::find_local_by_email(&mut conn.context(), email)
                .await
                .map_err(|e| Error::internal("find_identity", e))?
        };
        let encoded_hash = match identity.as_ref() {
            Some(identity) => identity
                .password_hash
                .as_deref()
                .ok_or_else(|| {
                    Error::internal(
                        "password_hash",
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
        let identity = identity
            .filter(|_| verified)
            .ok_or(Error::InvalidCredentials)?;
        let account = {
            let mut conn = database
                .conn()
                .await
                .map_err(|e| Error::internal("account_connection", e))?;
            account::find_by_id(&mut conn.context(), identity.account_id)
                .await
                .map_err(|e| Error::internal("find_account", e))?
        }
        .ok_or_else(|| {
            Error::internal(
                "find_account",
                tlab::Error::IllegalState("identity account is missing".into()),
            )
        })?;
        match account.status {
            AccountStatus::Withdrawn => Err(Error::AccountUnavailable),
            AccountStatus::Registered | AccountStatus::Active => Ok(account),
        }
    }
}

pub(super) async fn verify_password(
    hasher: Arc<dyn PasswordHasher>,
    slots: Arc<Semaphore>,
    password: String,
    encoded_hash: Arc<str>,
) -> Result<bool, Error> {
    let permit = slots.try_acquire_owned().map_err(|error| match error {
        TryAcquireError::NoPermits => Error::Busy,
        TryAcquireError::Closed => {
            Error::internal("hash_capacity", tlab::Error::Internal(error.into()))
        }
    })?;
    tokio::task::spawn_blocking(move || {
        // Cancellation must not release capacity while blocking work still runs.
        let _permit = permit;
        hasher.verify(&password, &encoded_hash)
    })
    .await
    .map_err(|e| Error::internal("verify_task", tlab::Error::Internal(e.into())))?
    .map_err(|e| Error::internal("password_verify", e))
}
