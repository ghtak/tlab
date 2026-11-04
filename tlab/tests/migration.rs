use std::{future::Future, pin::Pin};

use sqlx::Postgres;
use tlab::{
    Result,
    migration::{self, CodeMigration},
    sqlxdb::{Config, Context, Database},
};

fn code<'a, 'c>(
    ctx: &'a mut Context<'c, Postgres>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        sqlx::query("INSERT INTO migration_test VALUES (2)")
            .execute(ctx.backend())
            .await?;
        Ok(())
    })
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn mixed_steps_repeat_and_failure_stops_execution() -> Result<()> {
    let db = Database::<Postgres>::new(&Config {
        url: std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL"),
        max_connections: 1,
    })
    .await?;
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("0001_setup.sql"),
        "CREATE TEMP TABLE IF NOT EXISTS migration_test (value integer); INSERT INTO migration_test VALUES (1);",
    )?;
    std::fs::write(
        dir.path().join("0003_last.sql"),
        "INSERT INTO migration_test VALUES (3);",
    )?;
    let code = [CodeMigration {
        sequence: 2,
        name: "insert",
        run: code,
    }];
    migration::run(&db, dir.path(), &code).await?;
    migration::run(&db, dir.path(), &code).await?;
    let mut conn = db.conn().await?;
    let values: Vec<i32> = sqlx::query_scalar("SELECT value FROM migration_test")
        .fetch_all(conn.context().backend())
        .await?;
    assert_eq!(values, [1, 2, 3, 1, 2, 3]);
    drop(conn);
    std::fs::write(
        dir.path().join("0003_last.sql"),
        "SELECT * FROM missing_migration_table",
    )?;
    std::fs::write(
        dir.path().join("0004_after.sql"),
        "INSERT INTO migration_test VALUES (4)",
    )?;
    assert!(migration::run(&db, dir.path(), &code).await.is_err());
    let mut conn = db.conn().await?;
    let values: Vec<i32> = sqlx::query_scalar("SELECT value FROM migration_test")
        .fetch_all(conn.context().backend())
        .await?;
    assert_eq!(values, [1, 2, 3, 1, 2, 3, 1, 2]);
    Ok(())
}
