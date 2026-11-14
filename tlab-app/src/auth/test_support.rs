use chrono::Timelike;
use std::future::Future;
use uuid::Uuid;
type Database = crate::app::db::Database;

/// Authentication tests prepare users through the signup operation; HTTP
/// account.create authorization is covered by the application integration tests.
pub(crate) async fn create_local_user(
    container: &crate::app::Container,
    input: super::LocalAccountInput,
) -> serde_json::Value {
    serde_json::to_value(
        super::slice::create_local_user::execute(
            &container.database,
            container.passwd_policy.as_ref(),
            container.passwd_hasher.clone(),
            container.passwd_hash_slots.clone(),
            input,
        )
        .await
        .unwrap(),
    )
    .unwrap()
}

// Every scenario gets its own schema, including when tests run concurrently.
pub(crate) async fn isolated<F, Fut>(scenario: F)
where
    F: FnOnce(Database) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    isolated_with_url(|database, _url| scenario(database)).await;
}

pub(crate) async fn isolated_with_url<F, Fut>(scenario: F)
where
    F: FnOnce(Database, String) -> Fut + Send + 'static,
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
            url: scoped_url.clone(),
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
        scenario(db, scoped_url).await;
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
    jwt::AccessIssuer,
    repository::{account, refresh_token},
    token::RefreshToken as Secret,
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

pub(super) fn token_keys() -> (AccessIssuer, crate::auth::jwt::AccessVerifier) {
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
        crate::auth::jwt::AccessVerifier::new(material.public_key_pem(), "issuer", "audience")
            .unwrap(),
    )
}
