use std::sync::Arc;

use email_address::{EmailAddress, Options};
use serde::Deserialize;
use tlab::password::PasswordHasher;
use tokio::sync::{Semaphore, TryAcquireError};

use crate::{
    app::{
        db::Context,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::{
        entity::{Account, AccountStatus, NewAccount, NewIdentity, Provider},
        passwd_policy::PasswordPolicy,
        repository::{account, identity},
    },
};

// Deliberately no Debug: this value contains a plaintext password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAccountInput {
    pub nickname: String,
    pub email: String,
    pub password: String,
}

// No Debug or Serialize: prepared values contain a password hash.
pub(crate) struct PreparedLocalAccount {
    nickname: String,
    email: String,
    encoded_hash: String,
}

/// Normalize and validate using the caller's context. The early email check
/// preserves signup's retry contract; the unique index remains authoritative.
/// Release the connection before hashing the returned input.
pub(crate) async fn prepare_local_account(
    ctx: &mut Context<'_>,
    policy: &dyn PasswordPolicy,
    command: LocalAccountInput,
) -> AppResult<LocalAccountInput> {
    let nickname = command.nickname.trim();
    let email = command.email.trim();
    if nickname.is_empty() {
        return Err(AppError::new(
            AppErrorCode::NicknameRequired,
            "nickname must not be empty",
            "validate_input",
        ));
    }
    if nickname.contains('\0') {
        return Err(AppError::new(
            AppErrorCode::InvalidNickname,
            "nickname must not contain NUL characters",
            "validate_input",
        ));
    }
    if EmailAddress::parse_with_options(email, Options::default().without_display_text()).is_err() {
        return Err(AppError::new(
            AppErrorCode::InvalidEmail,
            "email must be a single email address",
            "validate_input",
        ));
    }
    policy
        .validate(&command.password)
        .map_err(|violation| AppError::new(violation.code, violation.message, "validate_input"))?;

    if identity::find_local_by_email(ctx, email)
        .await
        .map_err(|error| AppError::internal("find_local_by_email", "user creation failed", error))?
        .is_some()
    {
        return Err(AppError::new(
            AppErrorCode::EmailAlreadyExists,
            "local email already exists",
            "find_local_by_email",
        ));
    }
    Ok(LocalAccountInput {
        nickname: nickname.to_owned(),
        email: email.to_owned(),
        password: command.password,
    })
}

/// Hash already prepared input without retaining a database connection.
pub(crate) async fn hash_local_account(
    command: LocalAccountInput,
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
) -> AppResult<PreparedLocalAccount> {
    let encoded_hash = hash_password(hasher, hash_slots, command.password).await?;
    Ok(PreparedLocalAccount {
        nickname: command.nickname,
        email: command.email,
        encoded_hash,
    })
}

/// Store auth-owned rows in the caller's transaction without committing it.
pub(crate) async fn insert_prepared_local_account(
    ctx: &mut Context<'_>,
    prepared: PreparedLocalAccount,
) -> AppResult<Account> {
    let account = account::insert(
        ctx,
        &NewAccount {
            nickname: prepared.nickname,
            status: AccountStatus::Registered,
            withdrawn_at: None,
        },
    )
    .await
    .map_err(|error| AppError::internal("insert_account", "user creation failed", error))?;
    identity::insert(
        ctx,
        &NewIdentity {
            account_id: account.id,
            priority: 0,
            provider: Provider::Local,
            provider_subject: None,
            email: prepared.email,
            email_verified_at: None,
            password_hash: Some(prepared.encoded_hash),
        },
    )
    .await
    .map_err(|error| AppError::internal("insert_identity", "user creation failed", error))?;

    Ok(account)
}

async fn hash_password(
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
    password: String,
) -> Result<String, AppError> {
    let permit = hash_slots
        .try_acquire_owned()
        .map_err(|error| match error {
            TryAcquireError::NoPermits => AppError::new(
                AppErrorCode::Busy,
                "password hashing is busy",
                "hash_capacity",
            ),
            TryAcquireError::Closed => AppError::internal(
                "hash_capacity",
                "user creation failed",
                tlab::Error::Internal(error.into()),
            ),
        })?;
    tokio::task::spawn_blocking(move || {
        // Keep the slot until hashing actually ends, even when the caller is cancelled.
        let _permit = permit;
        hasher.hash(&password)
    })
    .await
    .map_err(|error| {
        AppError::internal(
            "hash_task",
            "user creation failed",
            tlab::Error::Internal(error.into()),
        )
    })?
    .map_err(|error| AppError::internal("password_hash", "user creation failed", error))
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};

    use tokio::{
        sync::mpsc,
        time::{Duration, timeout},
    };

    use super::*;

    struct Gate {
        released: Mutex<bool>,
        wake: Condvar,
        started: mpsc::UnboundedSender<()>,
    }

    impl Gate {
        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.wake.notify_all();
        }
    }

    struct ReleaseOnDrop(Arc<Gate>);

    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    struct GatedHasher(Arc<Gate>);

    impl PasswordHasher for GatedHasher {
        fn hash(&self, _: &str) -> tlab::Result<String> {
            self.0.started.send(()).unwrap();
            let mut released = self.0.released.lock().unwrap();
            while !*released {
                released = self.0.wake.wait(released).unwrap();
            }
            Ok("hash".into())
        }

        fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn independent_hashers_share_two_slots_and_cancellation_keeps_slot() {
        let (started, mut events) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate {
            released: Mutex::new(false),
            wake: Condvar::new(),
            started,
        });
        let _release_on_failure = ReleaseOnDrop(gate.clone());
        let slots = Arc::new(Semaphore::new(2));
        let hasher_a: Arc<dyn PasswordHasher> = Arc::new(GatedHasher(gate.clone()));
        let hasher_b: Arc<dyn PasswordHasher> = Arc::new(GatedHasher(gate.clone()));
        let first = tokio::spawn(hash_password(hasher_a.clone(), slots.clone(), "one".into()));
        timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        let second = tokio::spawn(hash_password(hasher_b.clone(), slots.clone(), "two".into()));
        timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            hash_password(hasher_a.clone(), slots.clone(), "three".into()).await,
            Err(AppError {
                code: AppErrorCode::Busy,
                ..
            })
        ));
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert_eq!(slots.available_permits(), 0);
        assert!(matches!(
            hash_password(hasher_b, slots.clone(), "four".into()).await,
            Err(AppError {
                code: AppErrorCode::Busy,
                ..
            })
        ));
        gate.release();
        assert_eq!(second.await.unwrap().unwrap(), "hash");
        let permits = timeout(Duration::from_secs(5), slots.acquire_many(2))
            .await
            .unwrap()
            .unwrap();
        drop(permits);
        assert_eq!(slots.available_permits(), 2);
    }

    struct FailingHasher;

    impl PasswordHasher for FailingHasher {
        fn hash(&self, _: &str) -> tlab::Result<String> {
            Err(tlab::Error::IllegalState("synthetic hash failure".into()))
        }
        fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn hashing_failure_releases_capacity_and_remains_an_internal_error() {
        let slots = Arc::new(Semaphore::new(2));
        assert!(matches!(
            hash_password(Arc::new(FailingHasher), slots.clone(), "secret".into()).await,
            Err(AppError {
                code: AppErrorCode::InternalError,
                ..
            })
        ));
        assert_eq!(slots.available_permits(), 2);
    }
}
