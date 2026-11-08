//! Storage only. Callers own transactions, locking and policy.
#![allow(
    dead_code,
    reason = "Refresh storage is connected by later authentication slices"
)]

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    app::db::Context,
    auth::{
        entity::{NewRefreshToken, RefreshToken, RevokeReason},
        token::RefreshHash,
    },
};

pub async fn insert(ctx: &mut Context<'_>, value: &NewRefreshToken) -> tlab::Result<RefreshToken> {
    Ok(sqlx::query_as("INSERT INTO tlab_auth_refresh_token (session_id, account_id, token_hash, session_created_at, session_expires_at, created_at, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *")
        .bind(value.session_id).bind(value.account_id).bind(value.token_hash.as_bytes().as_slice())
        .bind(value.session_created_at).bind(value.session_expires_at).bind(value.created_at).bind(value.expires_at)
        .fetch_one(ctx.backend()).await?)
}

/// Unfiltered lookup; callers own expiry, revocation and concurrency policy.
/// Workflows requiring locks re-read with `lock_by_hash` after the account lock.
pub async fn find_by_hash(
    ctx: &mut Context<'_>,
    hash: &RefreshHash,
) -> tlab::Result<Option<RefreshToken>> {
    Ok(
        sqlx::query_as("SELECT * FROM tlab_auth_refresh_token WHERE token_hash = $1")
            .bind(hash.as_bytes().as_slice())
            .fetch_optional(ctx.backend())
            .await?,
    )
}

/// Call only after locking the account in the same transaction.
pub async fn lock_by_hash(
    ctx: &mut Context<'_>,
    hash: &RefreshHash,
) -> tlab::Result<Option<RefreshToken>> {
    Ok(
        sqlx::query_as("SELECT * FROM tlab_auth_refresh_token WHERE token_hash = $1 FOR UPDATE")
            .bind(hash.as_bytes().as_slice())
            .fetch_optional(ctx.backend())
            .await?,
    )
}

/// Returns the number newly revoked; already revoked rows remain unchanged.
pub async fn revoke_session(
    ctx: &mut Context<'_>,
    account_id: i64,
    session_id: Uuid,
    at: OffsetDateTime,
    reason: RevokeReason,
) -> tlab::Result<u64> {
    Ok(sqlx::query("UPDATE tlab_auth_refresh_token SET revoked_at=$3, revoke_reason=$4 WHERE account_id=$1 AND session_id=$2 AND revoked_at IS NULL")
        .bind(account_id).bind(session_id).bind(at).bind(reason).execute(ctx.backend()).await?.rows_affected())
}

/// Call after account lock in the same transaction; includes expired rows.
pub async fn revoke_account(
    ctx: &mut Context<'_>,
    account_id: i64,
    at: OffsetDateTime,
    reason: RevokeReason,
) -> tlab::Result<u64> {
    Ok(sqlx::query("UPDATE tlab_auth_refresh_token SET revoked_at=$2, revoke_reason=$3 WHERE account_id=$1 AND revoked_at IS NULL")
        .bind(account_id).bind(at).bind(reason).execute(ctx.backend()).await?.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{
        entity::{AccountStatus, NewAccount},
        repository::account,
        token::RefreshToken as Secret,
    };
    use sqlx::types::time::OffsetDateTime;
    use std::time::Duration;

    type Database = crate::app::db::Database;

    use crate::auth::test_support::isolated;

    async fn new_account(db: &Database, nickname: &str) -> i64 {
        let mut conn = db.conn().await.unwrap();
        account::insert(
            &mut conn.context(),
            &NewAccount {
                nickname: nickname.into(),
                status: AccountStatus::Active,
                withdrawn_at: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    fn new_refresh(account_id: i64, now: OffsetDateTime) -> NewRefreshToken {
        NewRefreshToken {
            session_id: Uuid::new_v4(),
            account_id,
            token_hash: Secret::generate().unwrap().hash(),
            session_created_at: now,
            session_expires_at: now + Duration::from_secs(168 * 3600),
            created_at: now,
            expires_at: now + Duration::from_secs(168 * 3600),
        }
    }
    fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc().replace_nanosecond(0).unwrap()
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn hash_lookup_and_storage_constraints() {
        isolated(|db| async move {
            let a = new_account(&db, "one").await;
            let b = new_account(&db, "two").await;
            let at = now();
            let secret = Secret::generate().unwrap();
            let mut first = new_refresh(a, at);
            first.token_hash = secret.hash();
            let mut conn = db.conn().await.unwrap();
            let row = insert(&mut conn.context(), &first).await.unwrap();
            assert_eq!(row.token_hash, secret.hash().as_bytes());
            assert_ne!(row.token_hash.as_slice(), secret.expose_secret().as_bytes());
            for _ in 0..2 {
                let found = find_by_hash(&mut conn.context(), &secret.hash())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(found.session_id, first.session_id);
                assert_eq!(
                    found.session_expires_at,
                    at + Duration::from_secs(168 * 3600)
                );
                assert_eq!(found.expires_at, found.session_expires_at);
                assert!(found.revoked_at.is_none());
            }
            let mut duplicate_hash = new_refresh(b, at);
            duplicate_hash.token_hash = secret.hash();
            assert!(insert(&mut conn.context(), &duplicate_hash).await.is_err());
            let mut duplicate_session = new_refresh(a, at);
            duplicate_session.session_id = first.session_id;
            assert!(
                insert(&mut conn.context(), &duplicate_session)
                    .await
                    .is_err()
            );
            let mut invalid_session = new_refresh(a, at);
            invalid_session.session_id = Uuid::nil();
            assert!(insert(&mut conn.context(), &invalid_session).await.is_err());
            let mut shorter = new_refresh(a, at);
            shorter.expires_at = at + Duration::from_secs(3600);
            let row = insert(&mut conn.context(), &shorter).await.unwrap();
            assert_ne!(row.expires_at, row.session_expires_at);
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn caller_rollback_undoes_insert_and_revocation() {
        isolated(|db| async move {
            let a = new_account(&db, "one").await;
            let at = now();
            let first = new_refresh(a, at);
            let rolled_back = new_refresh(a, at);
            let mut tx = db.tx().await.unwrap();
            account::lock_by_id(&mut tx.context(), a)
                .await
                .unwrap()
                .unwrap();
            insert(&mut tx.context(), &first).await.unwrap();
            tx.commit().await.unwrap();
            let mut tx = db.tx().await.unwrap();
            account::lock_by_id(&mut tx.context(), a)
                .await
                .unwrap()
                .unwrap();
            insert(&mut tx.context(), &rolled_back).await.unwrap();
            lock_by_hash(&mut tx.context(), &first.token_hash)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                revoke_session(
                    &mut tx.context(),
                    a,
                    first.session_id,
                    at + Duration::from_secs(1),
                    RevokeReason::Logout
                )
                .await
                .unwrap(),
                1
            );
            tx.rollback().await.unwrap();
            let mut conn = db.conn().await.unwrap();
            assert!(
                find_by_hash(&mut conn.context(), &rolled_back.token_hash)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                find_by_hash(&mut conn.context(), &first.token_hash)
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_none()
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn revocation_is_scoped_and_preserves_first_result() {
        isolated(|db| async move {
            let a = new_account(&db, "one").await;
            let b = new_account(&db, "two").await;
            let at = now();
            let first = new_refresh(a, at);
            let second = new_refresh(a, at);
            let other = new_refresh(b, at);
            let mut conn = db.conn().await.unwrap();
            for token in [&first, &second, &other] {
                insert(&mut conn.context(), token).await.unwrap();
            }
            drop(conn);
            let mut tx = db.tx().await.unwrap();
            account::lock_by_id(&mut tx.context(), a)
                .await
                .unwrap()
                .unwrap();
            lock_by_hash(&mut tx.context(), &first.token_hash)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                revoke_session(
                    &mut tx.context(),
                    a,
                    first.session_id,
                    at + Duration::from_secs(1),
                    RevokeReason::Logout
                )
                .await
                .unwrap(),
                1
            );
            assert!(
                find_by_hash(&mut tx.context(), &second.token_hash)
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_none()
            );
            assert_eq!(
                revoke_session(
                    &mut tx.context(),
                    a,
                    first.session_id,
                    at + Duration::from_secs(2),
                    RevokeReason::AccountWithdrawn
                )
                .await
                .unwrap(),
                0
            );
            assert_eq!(
                revoke_account(
                    &mut tx.context(),
                    a,
                    at + Duration::from_secs(3),
                    RevokeReason::AccountSuspended
                )
                .await
                .unwrap(),
                1
            );
            tx.commit().await.unwrap();
            let mut conn = db.conn().await.unwrap();
            let first_row = find_by_hash(&mut conn.context(), &first.token_hash)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(first_row.revoked_at, Some(at + Duration::from_secs(1)));
            assert!(matches!(
                first_row.revoke_reason,
                Some(RevokeReason::Logout)
            ));
            assert!(
                find_by_hash(&mut conn.context(), &second.token_hash)
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_some()
            );
            assert!(
                find_by_hash(&mut conn.context(), &other.token_hash)
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_none()
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn account_lock_serializes_and_refresh_is_rechecked() {
        isolated(|db| async move {
            let a = new_account(&db, "one").await;
            let at = now();
            let token = new_refresh(a, at);
            let mut holder = db.tx().await.unwrap();
            account::lock_by_id(&mut holder.context(), a)
                .await
                .unwrap()
                .unwrap();
            insert(&mut holder.context(), &token).await.unwrap();
            holder.commit().await.unwrap();
            let mut holder = db.tx().await.unwrap();
            account::lock_by_id(&mut holder.context(), a)
                .await
                .unwrap()
                .unwrap();
            let mut waiter = db.tx().await.unwrap();
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(200),
                    account::lock_by_id(&mut waiter.context(), a)
                )
                .await
                .is_err()
            );
            lock_by_hash(&mut holder.context(), &token.token_hash)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                revoke_account(
                    &mut holder.context(),
                    a,
                    at + Duration::from_secs(1),
                    RevokeReason::AccountWithdrawn
                )
                .await
                .unwrap(),
                1
            );
            holder.commit().await.unwrap();
            account::lock_by_id(&mut waiter.context(), a)
                .await
                .unwrap()
                .unwrap();
            assert!(
                lock_by_hash(&mut waiter.context(), &token.token_hash)
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_some()
            );
            waiter.rollback().await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn refresh_row_lock_waits_and_returns_committed_state() {
        isolated(|db| async move {
            let a = new_account(&db, "one").await;
            let at = now();
            let token = new_refresh(a, at);
            let mut holder = db.tx().await.unwrap();
            account::lock_by_id(&mut holder.context(), a)
                .await
                .unwrap()
                .unwrap();
            insert(&mut holder.context(), &token).await.unwrap();
            holder.commit().await.unwrap();
            let mut holder = db.tx().await.unwrap();
            account::lock_by_id(&mut holder.context(), a)
                .await
                .unwrap()
                .unwrap();
            lock_by_hash(&mut holder.context(), &token.token_hash)
                .await
                .unwrap()
                .unwrap();
            let mut waiter = db.tx().await.unwrap();
            sqlx::query("SET LOCAL statement_timeout = '5s'")
                .execute(waiter.context().backend())
                .await
                .unwrap();
            {
                // Deliberately omit the waiter's account lock to test the row lock itself.
                let mut context = waiter.context();
                let pending = lock_by_hash(&mut context, &token.token_hash);
                tokio::pin!(pending);
                assert!(
                    tokio::time::timeout(Duration::from_millis(200), &mut pending)
                        .await
                        .is_err()
                );
                assert_eq!(
                    revoke_session(
                        &mut holder.context(),
                        a,
                        token.session_id,
                        at + Duration::from_secs(1),
                        RevokeReason::Logout
                    )
                    .await
                    .unwrap(),
                    1
                );
                holder.commit().await.unwrap();
                let observed = tokio::time::timeout(Duration::from_secs(5), &mut pending)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(observed.revoked_at, Some(at + Duration::from_secs(1)));
                assert!(matches!(observed.revoke_reason, Some(RevokeReason::Logout)));
            }
            waiter.rollback().await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
    async fn account_state_and_revoke_all_rollback_together() {
        isolated(|db| async move {
            let (id, secret, _) = crate::auth::test_support::session_setup(&db).await;
            let mut tx = db.tx().await.unwrap();
            account::lock_by_id(&mut tx.context(), id)
                .await
                .unwrap()
                .unwrap();
            sqlx::query(
                "UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=now() WHERE id=$1",
            )
            .bind(id)
            .execute(tx.context().backend())
            .await
            .unwrap();
            revoke_account(
                &mut tx.context(),
                id,
                OffsetDateTime::now_utc(),
                RevokeReason::AccountWithdrawn,
            )
            .await
            .unwrap();
            tx.rollback().await.unwrap();
            let mut conn = db.conn().await.unwrap();
            assert_eq!(
                account::find_by_id(&mut conn.context(), id)
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                AccountStatus::Active
            );
            assert!(
                find_by_hash(&mut conn.context(), &secret.hash())
                    .await
                    .unwrap()
                    .unwrap()
                    .revoked_at
                    .is_none()
            );
        })
        .await;
    }
}
