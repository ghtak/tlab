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
