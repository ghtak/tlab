use crate::{
    app::{
        db::Database,
        error::{AppError, AppResult},
    },
    auth::{
        entity::AccountStatus, hash_local_account, insert_prepared_local_account,
        passwd_policy::PasswordPolicy, prepare_local_account,
    },
};
use serde::Serialize;
use std::sync::Arc;
use tlab::password::PasswordHasher;
use tokio::sync::Semaphore;

pub(crate) use crate::auth::LocalAccountInput as Command;

#[derive(Debug, Serialize)]
pub struct CreatedLocalUser {
    pub account_id: i64,
    pub status: AccountStatus,
}

pub async fn execute(
    database: &Database,
    policy: &dyn PasswordPolicy,
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
    command: Command,
) -> AppResult<CreatedLocalUser> {
    let command = {
        let mut conn = database
            .conn()
            .await
            .map_err(|error| AppError::internal("conn", "user creation failed", error))?;
        prepare_local_account(&mut conn.context(), policy, command).await?
    };
    let prepared = hash_local_account(command, hasher, hash_slots).await?;
    let mut tx = database
        .tx()
        .await
        .map_err(|error| AppError::internal("begin_transaction", "user creation failed", error))?;
    let account = insert_prepared_local_account(&mut tx.context(), prepared).await?;
    tx.commit()
        .await
        .map_err(|error| AppError::internal("commit_transaction", "user creation failed", error))?;
    Ok(CreatedLocalUser {
        account_id: account.id,
        status: account.status,
    })
}
