use std::sync::Arc;

use email_address::{EmailAddress, Options};
use serde::Deserialize;
use tlab::password::PasswordHasher;
use tokio::sync::{Semaphore, TryAcquireError};

use crate::{
    app::db::Database,
    auth::{
        entity::{Account, AccountStatus},
        repository::{account, identity},
    },
};

// No Debug: the command owns a plaintext password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub email: String,
    pub password: String,
}

#[derive(Debug)]
pub enum Error {
    InvalidInput {
        field: &'static str,
        code: &'static str,
        message: &'static str,
    },
    InvalidCredentials,
    AccountUnavailable,
    Busy,
    Internal(InternalFailure),
}

#[derive(Debug)]
pub struct InternalFailure {
    pub stage: &'static str,
    pub cause: Box<dyn std::error::Error + Send + Sync>,
}

impl Error {
    fn internal(
        stage: &'static str,
        cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Internal(InternalFailure {
            stage,
            cause: Box::new(cause),
        })
    }
}

pub struct LocalLogin {
    hasher: Arc<dyn PasswordHasher>,
    slots: Arc<Semaphore>,
    dummy_password_hash: Arc<str>,
}

impl LocalLogin {
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
            return Err(Error::InvalidInput {
                field: "email",
                code: "INVALID_EMAIL",
                message: "email must be a single email address",
            });
        }
        if command.password.is_empty() {
            return Err(Error::InvalidInput {
                field: "password",
                code: "PASSWORD_REQUIRED",
                message: "password must not be empty",
            });
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
        TryAcquireError::Closed => Error::internal("hash_capacity", error),
    })?;
    tokio::task::spawn_blocking(move || {
        // Cancellation must not release capacity while blocking work still runs.
        let _permit = permit;
        hasher.verify(&password, &encoded_hash)
    })
    .await
    .map_err(|e| Error::internal("verify_task", e))?
    .map_err(|e| Error::internal("password_verify", e))
}
