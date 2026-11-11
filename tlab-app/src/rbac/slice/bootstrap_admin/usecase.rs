use std::sync::Arc;
use tlab::password::PasswordHasher;
use tokio::sync::Semaphore;

use crate::{
    app::{
        db::Database,
        error::{AppError, AppErrorCode, AppResult},
    },
    auth::{
        LocalAccountInput, hash_local_account, insert_prepared_local_account,
        passwd_policy::PasswordPolicy, prepare_local_account,
    },
    rbac::{
        NewAccountRole, NewRole, NewRolePermission, RoleId,
        repository::{account_role, permission, role, role_permission},
    },
};

#[derive(Debug)]
pub(crate) struct CreatedAdmin {
    pub account_id: i64,
    pub role_id: RoleId,
}

/// Run once by a trusted operator in an isolated environment.
pub(crate) async fn execute(
    db: &Database,
    policy: &dyn PasswordPolicy,
    hasher: Arc<dyn PasswordHasher>,
    hash_slots: Arc<Semaphore>,
    command: LocalAccountInput,
) -> AppResult<CreatedAdmin> {
    let command = {
        let mut conn = db
            .conn()
            .await
            .map_err(|error| AppError::internal("conn", "user creation failed", error))?;
        prepare_local_account(&mut conn.context(), policy, command).await?
    };
    let prepared = hash_local_account(command, hasher, hash_slots).await?;
    let mut tx = db.tx().await.map_err(|error| {
        AppError::internal(
            "bootstrap_transaction",
            "administrator creation failed",
            error,
        )
    })?;
    let result = {
        let mut ctx = tx.context();
        if !role::list_all(&mut ctx)
            .await
            .map_err(|error| {
                AppError::internal("bootstrap_roles", "administrator creation failed", error)
            })?
            .is_empty()
        {
            return Err(AppError::new(
                AppErrorCode::RbacConflict,
                "RBAC is already initialized",
                "bootstrap_roles",
            ));
        }
        let catalog = permission::list(&mut ctx).await.map_err(|error| {
            AppError::internal("bootstrap_catalog", "administrator creation failed", error)
        })?;
        let account = insert_prepared_local_account(&mut ctx, prepared).await.map_err(|error| {
            // Ordinary signup preserves its retry contract. Bootstrap treats a
            // concurrent email collision as an initialization conflict.
            if matches!(&error.cause, Some(tlab::Error::DbError(sqlx::Error::Database(db)))
                if db.is_unique_violation() && db.constraint() == Some("tlab_account_identity_local_email_key")) {
                AppError::new(AppErrorCode::EmailAlreadyExists, "local email already exists", "insert_identity")
            } else { error }
        })?;
        let admin_role = role::insert(
            &mut ctx,
            &NewRole {
                name: "bootstrap-admin".into(),
                description: "Initial administrator".into(),
            },
        )
        .await
        .map_err(|error| {
            AppError::internal("bootstrap_role", "administrator creation failed", error)
        })?;
        let permissions: Vec<_> = catalog
            .into_iter()
            .map(|entry| NewRolePermission {
                role_id: admin_role.id,
                permission_code: entry.code,
            })
            .collect();
        role_permission::insert_many(&mut ctx, &permissions)
            .await
            .map_err(|error| {
                AppError::internal(
                    "bootstrap_permissions",
                    "administrator creation failed",
                    error,
                )
            })?;
        account_role::insert(
            &mut ctx,
            &NewAccountRole {
                account_id: account.id,
                role_id: admin_role.id,
            },
        )
        .await
        .map_err(|error| {
            AppError::internal(
                "bootstrap_assignment",
                "administrator creation failed",
                error,
            )
        })?;
        CreatedAdmin {
            account_id: account.id,
            role_id: admin_role.id,
        }
    };
    tx.commit().await.map_err(|error| {
        AppError::internal("bootstrap_commit", "administrator creation failed", error)
    })?;
    Ok(result)
}
