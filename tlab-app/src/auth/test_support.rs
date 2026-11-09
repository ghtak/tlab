use chrono::Timelike;
use std::future::Future;
use uuid::Uuid;
type Database = crate::app::db::Database;

// Every scenario gets its own schema, including when tests run concurrently.
pub(super) async fn isolated<F, Fut>(scenario: F)
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
            "../../migrations/0001_create_tlab_account.sql"
        ))
        .execute(conn.context().backend())
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/0002_create_tlab_auth_refresh_token.sql"
        ))
        .execute(conn.context().backend())
        .await
        .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0003_create_tlab_rbac.sql"))
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

use super::{
    entity::{AccountStatus, NewAccount},
    repository::{account, refresh_token},
    token::{AccessIssuer, RefreshToken as Secret},
};
use chrono::Utc;
use std::time::Duration;

pub(super) async fn session_setup(db: &Database) -> (i64, Secret, uuid::Uuid) {
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
    let now = Utc::now().with_nanosecond(0).unwrap();
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

pub(super) fn token_keys() -> (AccessIssuer, crate::auth::token::AccessVerifier) {
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
