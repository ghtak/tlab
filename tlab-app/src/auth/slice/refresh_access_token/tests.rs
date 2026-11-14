use crate::app::error::{AppError, AppErrorCode};
use crate::auth::{
    entity::RevokeReason,
    jwt::AccessIssuer,
    repository::{account, refresh_token},
    slice::{logout, refresh_access_token},
    test_support::{isolated, session_setup as setup, token_keys as keys},
};
use chrono::Utc;
use jsonwebtoken::EncodingKey;
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn refresh_reads_current_roles_and_preserves_session_on_lookup_failure() {
    isolated(|db| async move {
        let (id, secret, _) = setup(&db).await;
        let (issuer, verifier) = keys();
        let read = |token: crate::auth::jwt::AccessToken| {
            verifier.verify(token.expose_secret(), Utc::now().timestamp() as u64).unwrap().user["role_ids"].clone()
        };
        assert_eq!(read(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await.unwrap()), serde_json::json!([]));
        let mut conn = db.conn().await.unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("INSERT INTO tlab_rbac_role (name, description) VALUES ('a', ''), ('b', '') RETURNING id").fetch_all(conn.context().backend()).await.unwrap();
        for role in &ids {
            sqlx::query("INSERT INTO tlab_rbac_account_role (account_id, role_id) VALUES ($1, $2)").bind(id).bind(role).execute(conn.context().backend()).await.unwrap();
        }
        drop(conn);
        assert_eq!(read(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await.unwrap()), serde_json::json!(ids));
        let mut conn = db.conn().await.unwrap();
        sqlx::query("DELETE FROM tlab_rbac_account_role WHERE account_id=$1 AND role_id=$2").bind(id).bind(ids[0]).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert_eq!(read(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await.unwrap()), serde_json::json!([ids[1]]));
        let mut conn = db.conn().await.unwrap();
        let before: String = sqlx::query_scalar("SELECT row_to_json(t)::text FROM tlab_auth_refresh_token t").fetch_one(conn.context().backend()).await.unwrap();
        sqlx::query("ALTER TABLE tlab_rbac_account_role RENAME TO unavailable_roles").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert!(matches!(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await, Err(e @ AppError { code: AppErrorCode::InternalError, .. }) if e.stage == "list_role_ids"));
        let mut conn = db.conn().await.unwrap();
        let after: String = sqlx::query_scalar("SELECT row_to_json(t)::text FROM tlab_auth_refresh_token t").fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(before, after);
    }).await;
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
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer),
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer)
        );
        for token in [a.unwrap(), b.unwrap()] {
            let now = Utc::now().timestamp() as u64;
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
            refresh_access_token::execute(&db, "missing", &issuer).await,
            Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })
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
                refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await,
                Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })
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
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await,
            Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })
        ));
        logout::execute(&db, secret.expose_secret())
            .await
            .unwrap();
        assert!(matches!(
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await,
            Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })
        ));
    })
    .await;
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
            Utc::now(),
            RevokeReason::Logout,
        )
        .await
        .unwrap();
        let access = tokio::time::timeout(
            Duration::from_secs(3),
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer),
        )
        .await
        .unwrap()
        .unwrap();
        holder.commit().await.unwrap();
        verifier
            .verify(access.expose_secret(), Utc::now().timestamp() as u64)
            .unwrap();
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

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn signing_and_revocation_failure_do_not_return_success_or_persist() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let invalid_issuer = Arc::new(AccessIssuer{
            key: EncodingKey::from_ed_der(b"invalid key"),
            issuer: "test-issuer".into(),
            audience: "test-audience".into(),
        });
        assert!(matches!(refresh_access_token::execute(&db, secret.expose_secret(), &invalid_issuer).await, Err(AppError { code: AppErrorCode::InternalError, .. })));
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_revoke() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'failure'; END $$; CREATE TRIGGER reject_revoke BEFORE UPDATE ON tlab_auth_refresh_token FOR EACH ROW EXECUTE FUNCTION reject_revoke();").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert!(matches!(logout::execute(&db, secret.expose_secret()).await, Err(AppError { code: AppErrorCode::InternalError, .. })));
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
        assert!(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await.is_ok());
        sqlx::query("UPDATE tlab_auth_refresh_token SET session_created_at=now()-interval '168 hours 1 minute', created_at=now()-interval '168 hours 1 minute', session_expires_at=now()-interval '1 minute', expires_at=now()-interval '1 minute'").execute(conn.context().backend()).await.unwrap();
        assert!(matches!(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await, Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })));
        logout::execute(&db, secret.expose_secret()).await.unwrap();
        logout::execute(&db, secret.expose_secret()).await.unwrap();
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn concurrent_logout_and_refresh_leave_session_revoked() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, verifier) = keys();
        let (refresh, logout) = tokio::join!(
            refresh_access_token::execute(&db, secret.expose_secret(), &issuer),
            logout::execute(&db, secret.expose_secret())
        );
        logout.unwrap();
        match refresh {
            Ok(access) => {
                verifier
                    .verify(access.expose_secret(), Utc::now().timestamp() as u64)
                    .unwrap();
            }
            Err(AppError {
                code: AppErrorCode::InvalidRefreshToken,
                ..
            }) => {}
            Err(error) => {
                panic!("unexpected refresh failure: {error:?}")
            }
        }
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

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn revoked_refresh_is_rejected_for_active_account() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, _) = keys();
        logout::execute(&db, secret.expose_secret()).await.unwrap();
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

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn refresh_read_before_logout_can_finish_after_logout_commits() {
    isolated(|db| async move {
        let (_, secret, _) = setup(&db).await;
        let (issuer, verifier) = keys();
        let mut holder = db.tx().await.unwrap();
        sqlx::query("LOCK TABLE tlab_account IN ACCESS EXCLUSIVE MODE")
            .execute(holder.context().backend()).await.unwrap();
        let pending = refresh_access_token::execute(&db, secret.expose_secret(), &issuer);
        tokio::pin!(pending);
        assert!(tokio::time::timeout(Duration::from_millis(200), &mut pending).await.is_err());
        let mut observer = db.conn().await.unwrap();
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND query = 'SELECT * FROM tlab_account WHERE id = $1' AND cardinality(pg_blocking_pids(pid)) > 0)")
            .fetch_one(observer.context().backend()).await.unwrap();
        assert!(blocked, "refresh must have reached the blocked account read");
        drop(observer);
        logout::execute(&db, secret.expose_secret()).await.unwrap();
        holder.commit().await.unwrap();
        let access = tokio::time::timeout(Duration::from_secs(3), &mut pending).await.unwrap().unwrap();
        verifier.verify(access.expose_secret(), Utc::now().timestamp() as u64).unwrap();
        assert!(matches!(refresh_access_token::execute(&db, secret.expose_secret(), &issuer).await, Err(AppError { code: AppErrorCode::InvalidRefreshToken, .. })));
    }).await;
}
