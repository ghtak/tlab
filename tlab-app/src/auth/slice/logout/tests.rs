use crate::app::error::{AppError, AppErrorCode};
use crate::auth::{
    entity::RevokeReason,
    repository::{account, refresh_token},
    slice::{logout, refresh_access_token},
    test_support::{isolated, session_setup as setup, token_keys as keys},
    token::RefreshToken as Secret,
};
use sqlx::types::time::OffsetDateTime;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn logout_and_account_revocation_preserve_first_result() {
    isolated(|db| async move {
        let (id, first, _) = setup(&db).await;
        let (_, other, _) = setup(&db).await;
        let second = Secret::generate().unwrap();
        let mut conn = db.conn().await.unwrap();
        sqlx::query("INSERT INTO tlab_auth_refresh_token (session_id,account_id,token_hash,session_created_at,session_expires_at,created_at,expires_at) SELECT $1, account_id,$2,session_created_at,session_expires_at,created_at,expires_at FROM tlab_auth_refresh_token WHERE account_id=$3").bind(uuid::Uuid::new_v4()).bind(second.hash().as_bytes().as_slice()).bind(id).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        logout::execute(&db, first.expose_secret()).await.unwrap();
        let mut conn = db.conn().await.unwrap();
        let before = refresh_token::find_by_hash(&mut conn.context(), &first.hash()).await.unwrap().unwrap().revoked_at;
        assert!(refresh_token::find_by_hash(&mut conn.context(), &second.hash()).await.unwrap().unwrap().revoked_at.is_none());
        drop(conn);
        logout::execute(&db, first.expose_secret()).await.unwrap();
        let mut tx = db.tx().await.unwrap();
        account::lock_by_id(&mut tx.context(), id).await.unwrap().unwrap();
        assert_eq!(refresh_token::revoke_account(&mut tx.context(), id, OffsetDateTime::now_utc(), RevokeReason::AccountWithdrawn).await.unwrap(), 1);
        tx.commit().await.unwrap();
        let mut conn = db.conn().await.unwrap();
        let row = refresh_token::find_by_hash(&mut conn.context(), &first.hash()).await.unwrap().unwrap();
        assert_eq!(row.revoked_at, before); assert!(matches!(row.revoke_reason, Some(RevokeReason::Logout)));
        assert!(refresh_token::find_by_hash(&mut conn.context(), &second.hash()).await.unwrap().unwrap().revoked_at.is_some());
        assert!(refresh_token::find_by_hash(&mut conn.context(), &other.hash()).await.unwrap().unwrap().revoked_at.is_none());
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn logout_commit_failure_rolls_back() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'commit failure'; END $$; CREATE CONSTRAINT TRIGGER reject_commit AFTER UPDATE ON tlab_auth_refresh_token DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_commit();").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert!(matches!(logout::execute(&db, secret.expose_secret()).await, Err(AppError { code: AppErrorCode::InternalError, .. })));
        let mut conn = db.conn().await.unwrap();
        assert!(refresh_token::find_by_hash(&mut conn.context(), &secret.hash()).await.unwrap().unwrap().revoked_at.is_none());
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn logout_does_not_wait_for_account_lock() {
    isolated(|db| async move {
        let (id, secret, _) = setup(&db).await;
        let mut holder = db.tx().await.unwrap();
        account::lock_by_id(&mut holder.context(), id)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            logout::execute(&db, secret.expose_secret()),
        )
        .await
        .unwrap()
        .unwrap();
        holder.rollback().await.unwrap();
        let (issuer, _) = keys();
        assert!(matches!(
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await,
            Err(AppError {
                code: AppErrorCode::InvalidRefreshToken,
                ..
            })
        ));
    })
    .await;
}
