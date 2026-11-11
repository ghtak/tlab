use std::{future::Future, path::Path};
type Database = tlab::sqlxdb::Database<sqlx::Postgres>;

pub async fn isolated_with_url<F, Fut>(scenario: F)
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
    let schema = format!("tlab_rbac_test_{}", uuid::Uuid::new_v4().simple());
    let mut conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let scoped = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let result = tokio::spawn(async move {
        let db = Database::new(&tlab::sqlxdb::Config {
            url: scoped.clone(),
            max_connections: 4,
        })
        .await
        .unwrap();
        tlab::migration::run(
            &db,
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")),
            &[],
        )
        .await
        .unwrap();
        scenario(db, scoped).await;
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}
