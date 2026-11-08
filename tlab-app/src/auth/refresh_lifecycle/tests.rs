use super::*;
use crate::auth::{
    entity::{AccountStatus, NewAccount},
    repository::account,
    token::RefreshToken as Secret,
};
use std::{future::Future, time::Duration};
use uuid::Uuid;

type Database = crate::app::db::Database;

// Every scenario gets its own schema, including when tests run concurrently.
async fn isolated<F, Fut>(scenario: F)
where
    F: FnOnce(Database) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let url = std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL");
    let admin = Database::new(&tlab::sqlxdb::Config {
        url: url.clone(),
        max_connections: 1,
    })
    .await
    .unwrap();
    let schema = format!("tlab_refresh_test_{}", Uuid::new_v4().simple());
    let mut conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let scoped_url = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let result = tokio::spawn(async move {
        let db = Database::new(&tlab::sqlxdb::Config {
            url: scoped_url,
            max_connections: 4,
        })
        .await
        .unwrap();
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/0001_create_tlab_account.sql"
        ))
        .execute(conn.context().backend())
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/0002_create_tlab_auth_refresh_token.sql"
        ))
        .execute(conn.context().backend())
        .await
        .unwrap();
        drop(conn);
        scenario(db).await;
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}

async fn setup(db: &Database) -> (i64, Secret, uuid::Uuid) {
    use crate::auth::entity::NewRefreshToken;
    let mut conn = db.conn().await.unwrap();
    let account = account::insert(
        &mut conn.context(),
        &NewAccount {
            nickname: "original".into(),
            status: AccountStatus::Active,
            withdrawn_at: None,
        },
    )
    .await
    .unwrap();
    let secret = Secret::generate().unwrap();
    let sid = uuid::Uuid::new_v4();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    refresh_token::insert(
        &mut conn.context(),
        &NewRefreshToken {
            session_id: sid,
            account_id: account.id,
            token_hash: secret.hash(),
            session_created_at: now,
            session_expires_at: now + Duration::from_secs(604800),
            created_at: now,
            expires_at: now + Duration::from_secs(604800),
        },
    )
    .await
    .unwrap();
    (account.id, secret, sid)
}

fn keys() -> (AccessIssuer, crate::auth::token::AccessVerifier) {
    let dir = tempfile::tempdir().unwrap();
    let material = tlab::keyfiles::Ed25519KeyFile {
        private_key: dir.path().join("private.pem"),
        public_key: dir.path().join("public.pem"),
        generate_if_missing: true,
    }
    .load()
    .unwrap();
    (
        AccessIssuer::new(&material, "issuer", "audience").unwrap(),
        crate::auth::token::AccessVerifier::new(material.public_key_pem(), "issuer", "audience")
            .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn repeated_refresh_preserves_session_and_uses_latest_account() {
    isolated(|db| async move {
        let (id, secret, sid) = setup(&db).await;
        let mut conn = db.conn().await.unwrap();
        let before: String =
            sqlx::query_scalar("SELECT row_to_json(t)::text FROM tlab_auth_refresh_token t")
                .fetch_one(conn.context().backend())
                .await
                .unwrap();
        sqlx::query("UPDATE tlab_account SET nickname='latest' WHERE id=$1")
            .bind(id)
            .execute(conn.context().backend())
            .await
            .unwrap();
        drop(conn);
        let (issuer, verifier) = keys();
        let (a, b) = tokio::join!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer),
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer)
        );
        for token in [a.unwrap(), b.unwrap()] {
            let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
            let claims = verifier.verify(token.expose_secret(), now).unwrap();
            assert_eq!(claims.sid, sid);
            assert_eq!(claims.sub, id.to_string());
            assert_eq!(claims.user["nickname"], "latest");
            assert_eq!(claims.exp - claims.iat, 600);
        }
        let mut conn = db.conn().await.unwrap();
        let after: String =
            sqlx::query_scalar("SELECT row_to_json(t)::text FROM tlab_auth_refresh_token t")
                .fetch_one(conn.context().backend())
                .await
                .unwrap();
        assert_eq!(before, after);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn expiry_rejection_and_invalid_states() {
    isolated(|db| async move {
        let (id, secret, _) = setup(&db).await;
        let (issuer, _) = keys();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, "missing", &issuer).await,
            Err(Error::InvalidRefresh)
        ));
        let mut conn = db.conn().await.unwrap();
        // Permit deliberately independent expiry values to exercise both guards.
        sqlx::raw_sql("ALTER TABLE tlab_auth_refresh_token DROP CONSTRAINT tlab_auth_refresh_token_expiry_check, DROP CONSTRAINT tlab_auth_refresh_token_session_expiry_check")
            .execute(conn.context().backend()).await.unwrap();
        for column in ["expires_at", "session_expires_at"] {
            let sql =
                format!("UPDATE tlab_auth_refresh_token SET {column}=now() - interval '1 second'");
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .execute(conn.context().backend())
                .await
                .unwrap();
            assert!(matches!(
                RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
                Err(Error::InvalidRefresh)
            ));
            let sql =
                format!("UPDATE tlab_auth_refresh_token SET {column}=now() + interval '1 day'");
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .execute(conn.context().backend())
                .await
                .unwrap();
        }
        sqlx::query("UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=now() WHERE id=$1")
            .bind(id)
            .execute(conn.context().backend())
            .await
            .unwrap();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
        RefreshLifecycle::logout(&db, secret.expose_secret())
            .await
            .unwrap();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
    })
    .await;
}

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
        RefreshLifecycle::logout(&db, first.expose_secret()).await.unwrap();
        let mut conn = db.conn().await.unwrap();
        let before = refresh_token::find_by_hash(&mut conn.context(), &first.hash()).await.unwrap().unwrap().revoked_at;
        assert!(refresh_token::find_by_hash(&mut conn.context(), &second.hash()).await.unwrap().unwrap().revoked_at.is_none());
        drop(conn);
        RefreshLifecycle::logout(&db, first.expose_secret()).await.unwrap();
        let mut tx = db.tx().await.unwrap();
        account::lock_by_id(&mut tx.context(), id).await.unwrap().unwrap();
        assert_eq!(revoke_all_locked(&mut tx.context(), id, OffsetDateTime::now_utc(), RevokeReason::AccountWithdrawn).await.unwrap(), 1);
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
async fn refresh_does_not_wait_for_uncommitted_logout() {
    isolated(|db| async move {
        let (id, secret, sid) = setup(&db).await;
        let (issuer, verifier) = keys();
        let mut holder = db.tx().await.unwrap();
        account::lock_by_id(&mut holder.context(), id)
            .await
            .unwrap()
            .unwrap();
        refresh_token::revoke_session(
            &mut holder.context(),
            id,
            sid,
            OffsetDateTime::now_utc(),
            RevokeReason::Logout,
        )
        .await
        .unwrap();
        let access = tokio::time::timeout(
            Duration::from_secs(3),
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer),
        )
        .await
        .unwrap()
        .unwrap();
        holder.commit().await.unwrap();
        verifier
            .verify(
                access.expose_secret(),
                OffsetDateTime::now_utc().unix_timestamp() as u64,
            )
            .unwrap();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn signing_and_revocation_failure_do_not_return_success_or_persist() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let issuer = AccessIssuer::with_invalid_signing_key();
        assert!(matches!(RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await, Err(Error::Internal(_))));
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_revoke() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'failure'; END $$; CREATE TRIGGER reject_revoke BEFORE UPDATE ON tlab_auth_refresh_token FOR EACH ROW EXECUTE FUNCTION reject_revoke();").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert!(matches!(RefreshLifecycle::logout(&db, secret.expose_secret()).await, Err(Error::Internal(_))));
        let mut conn = db.conn().await.unwrap();
        assert!(refresh_token::find_by_hash(&mut conn.context(), &secret.hash()).await.unwrap().unwrap().revoked_at.is_none());
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn seven_day_session_boundary() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, _) = keys();
        let mut conn = db.conn().await.unwrap();
        sqlx::query("UPDATE tlab_auth_refresh_token SET session_created_at=now()-interval '167 hours 59 minutes', created_at=now()-interval '167 hours 59 minutes', session_expires_at=now()+interval '1 minute', expires_at=now()+interval '1 minute'").execute(conn.context().backend()).await.unwrap();
        assert!(RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await.is_ok());
        sqlx::query("UPDATE tlab_auth_refresh_token SET session_created_at=now()-interval '168 hours 1 minute', created_at=now()-interval '168 hours 1 minute', session_expires_at=now()-interval '1 minute', expires_at=now()-interval '1 minute'").execute(conn.context().backend()).await.unwrap();
        assert!(matches!(RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await, Err(Error::InvalidRefresh)));
        RefreshLifecycle::logout(&db, secret.expose_secret()).await.unwrap();
        RefreshLifecycle::logout(&db, secret.expose_secret()).await.unwrap();
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn concurrent_logout_and_refresh_leave_session_revoked() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, verifier) = keys();
        let (refresh, logout) = tokio::join!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer),
            RefreshLifecycle::logout(&db, secret.expose_secret())
        );
        logout.unwrap();
        match refresh {
            Ok(access) => {
                verifier
                    .verify(
                        access.expose_secret(),
                        OffsetDateTime::now_utc().unix_timestamp() as u64,
                    )
                    .unwrap();
            }
            Err(Error::InvalidRefresh) => {}
            Err(Error::Internal(error)) => panic!("unexpected refresh failure: {error:?}"),
        }
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn logout_commit_failure_rolls_back() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'commit failure'; END $$; CREATE CONSTRAINT TRIGGER reject_commit AFTER UPDATE ON tlab_auth_refresh_token DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_commit();").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert!(matches!(RefreshLifecycle::logout(&db, secret.expose_secret()).await, Err(Error::Internal(_))));
        let mut conn = db.conn().await.unwrap();
        assert!(refresh_token::find_by_hash(&mut conn.context(), &secret.hash()).await.unwrap().unwrap().revoked_at.is_none());
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn revoked_refresh_is_rejected_for_active_account() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, _) = keys();
        RefreshLifecycle::logout(&db, secret.expose_secret())
            .await
            .unwrap();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn account_state_and_revoke_all_rollback_together() {
    isolated(|db| async move {
        let (id, secret, _) = setup(&db).await;
        let mut tx = db.tx().await.unwrap();
        account::lock_by_id(&mut tx.context(), id)
            .await
            .unwrap()
            .unwrap();
        sqlx::query("UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=now() WHERE id=$1")
            .bind(id)
            .execute(tx.context().backend())
            .await
            .unwrap();
        revoke_all_locked(
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
            refresh_token::find_by_hash(&mut conn.context(), &secret.hash())
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
            RefreshLifecycle::logout(&db, secret.expose_secret()),
        )
        .await
        .unwrap()
        .unwrap();
        holder.rollback().await.unwrap();
        let (issuer, _) = keys();
        assert!(matches!(
            RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await,
            Err(Error::InvalidRefresh)
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn refresh_read_before_logout_can_finish_after_logout_commits() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, verifier) = keys();
        let mut holder = db.tx().await.unwrap();
        sqlx::query("LOCK TABLE tlab_account IN ACCESS EXCLUSIVE MODE")
            .execute(holder.context().backend()).await.unwrap();
        let pending = RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer);
        tokio::pin!(pending);
        assert!(tokio::time::timeout(Duration::from_millis(200), &mut pending).await.is_err());
        let mut observer = db.conn().await.unwrap();
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND query = 'SELECT * FROM tlab_account WHERE id = $1' AND cardinality(pg_blocking_pids(pid)) > 0)")
            .fetch_one(observer.context().backend()).await.unwrap();
        assert!(blocked, "refresh must have reached the blocked account read");
        drop(observer);
        RefreshLifecycle::logout(&db, secret.expose_secret()).await.unwrap();
        holder.commit().await.unwrap();
        let access = tokio::time::timeout(Duration::from_secs(3), &mut pending).await.unwrap().unwrap();
        verifier.verify(access.expose_secret(), OffsetDateTime::now_utc().unix_timestamp() as u64).unwrap();
        assert!(matches!(RefreshLifecycle::refresh(&db, secret.expose_secret(), &issuer).await, Err(Error::InvalidRefresh)));
    }).await;
}
