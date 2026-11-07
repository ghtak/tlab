use std::sync::Arc;

use email_address::{EmailAddress, Options};
use serde::{Deserialize, Serialize};
use tlab::password::PasswordHasher;
use tokio::sync::{Semaphore, TryAcquireError};

use crate::{
    app::db::Database,
    auth::{
        entity::{AccountStatus, NewAccount, NewIdentity, Provider},
        passwd_policy::PasswordPolicy,
        repository::{account, identity},
    },
};

// Deliberately no Debug: this value contains a plaintext password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub nickname: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct CreatedLocalUser {
    pub account_id: i64,
    pub status: AccountStatus,
}

#[derive(Debug)]
pub enum Error {
    InvalidInput {
        field: &'static str,
        code: &'static str,
        message: &'static str,
    },
    EmailAlreadyExists,
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
        error: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Internal(InternalFailure {
            stage,
            cause: Box::new(error),
        })
    }
}

pub async fn execute(
    database: &Database,
    policy: &dyn PasswordPolicy,
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
    command: Command,
) -> Result<CreatedLocalUser, Error> {
    let nickname = command.nickname.trim();
    let email = command.email.trim();
    if nickname.is_empty() {
        return Err(Error::InvalidInput {
            field: "nickname",
            code: "NICKNAME_REQUIRED",
            message: "nickname must not be empty",
        });
    }
    if nickname.contains('\0') {
        return Err(Error::InvalidInput {
            field: "nickname",
            code: "INVALID_NICKNAME",
            message: "nickname must not contain NUL characters",
        });
    }
    if EmailAddress::parse_with_options(email, Options::default().without_display_text()).is_err() {
        return Err(Error::InvalidInput {
            field: "email",
            code: "INVALID_EMAIL",
            message: "email must be a single email address",
        });
    }
    policy
        .validate(&command.password)
        .map_err(|violation| Error::InvalidInput {
            field: "password",
            code: violation.code,
            message: violation.message,
        })?;

    let encoded_hash = hash_password(hasher, hash_slots, command.password).await?;
    let mut tx = database
        .tx()
        .await
        .map_err(|error| Error::internal("begin_transaction", error))?;
    let stored = async {
        let mut ctx = tx.context();
        let account = account::insert(
            &mut ctx,
            &NewAccount {
                nickname: nickname.to_owned(),
                status: AccountStatus::Registered,
                withdrawn_at: None,
            },
        )
        .await
        .map_err(|error| ("insert_account", error))?;
        identity::insert(
            &mut ctx,
            &NewIdentity {
                account_id: account.id,
                priority: 0,
                provider: Provider::Local,
                provider_subject: None,
                email: email.to_owned(),
                email_verified_at: None,
                password_hash: Some(encoded_hash),
            },
        )
        .await
        .map_err(|error| ("insert_identity", error))?;
        Ok::<_, (&'static str, tlab::Error)>((account.id, account.status))
    }
    .await;

    let (account_id, status) = match stored {
        Ok(id) => id,
        Err((stage, error)) => {
            // Translate this particular storage conflict at the scenario boundary.
            let duplicate_email = matches!(
                &error,
                tlab::Error::DbError(sqlx::Error::Database(db_error))
                    if db_error.is_unique_violation()
                        && db_error.constraint() == Some("tlab_account_identity_local_email_key")
            );
            tx.rollback()
                .await
                .map_err(|error| Error::internal("rollback_transaction", error))?;
            return Err(if duplicate_email {
                Error::EmailAlreadyExists
            } else {
                Error::internal(stage, error)
            });
        }
    };
    tx.commit()
        .await
        .map_err(|error| Error::internal("commit_transaction", error))?;
    Ok(CreatedLocalUser { account_id, status })
}

async fn hash_password(
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
    password: String,
) -> Result<String, Error> {
    let permit = hash_slots
        .try_acquire_owned()
        .map_err(|error| match error {
            TryAcquireError::NoPermits => Error::Busy,
            TryAcquireError::Closed => Error::internal("hash_capacity", error),
        })?;
    tokio::task::spawn_blocking(move || {
        // Keep the slot until hashing actually ends, even when the caller is cancelled.
        let _permit = permit;
        hasher.hash(&password)
    })
    .await
    .map_err(|error| Error::internal("hash_task", error))?
    .map_err(|error| Error::internal("password_hash", error))
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
            Err(Error::Busy)
        ));
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert_eq!(slots.available_permits(), 0);
        assert!(matches!(
            hash_password(hasher_b, slots.clone(), "four".into()).await,
            Err(Error::Busy)
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
            Err(Error::Internal(_))
        ));
        assert_eq!(slots.available_permits(), 2);
    }
}
