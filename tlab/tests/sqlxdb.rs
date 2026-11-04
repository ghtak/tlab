use std::error::Error as _;
use std::time::Duration;

use sqlx::Postgres;
use tlab::sqlxdb::{Config, Context, Database};
use tlab::{Error, Result};

type Db = Database<Postgres>;
type Ctx<'a> = Context<'a, Postgres>;

async fn insert(ctx: &mut Ctx<'_>, value: i32) -> Result<()> {
    sqlx::query("INSERT INTO tlab_test_values VALUES ($1)")
        .bind(value)
        .execute(ctx.backend())
        .await?;
    Ok(())
}

async fn values(ctx: &mut Ctx<'_>) -> Result<Vec<i32>> {
    Ok(
        sqlx::query_scalar("SELECT value FROM tlab_test_values ORDER BY value")
            .fetch_all(ctx.backend())
            .await?,
    )
}

async fn database(max_connections: u32) -> Result<Db> {
    let url = std::env::var("TLAB_TEST_DATABASE_URL")
        .expect("set TLAB_TEST_DATABASE_URL for the PostgreSQL integration tests");
    Db::new(&Config {
        url,
        max_connections,
    })
    .await
}

#[test]
fn config_composes_with_existing_loader() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct AppConfig {
        database: Config,
    }
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.toml");
    std::fs::write(
        &path,
        "[database]\nurl = 'postgres://localhost/example'\nmax_connections = 3\n",
    )?;
    let config: AppConfig = tlab::config::load(&path, "TLAB_SQLX_CONFIG_TEST")?;
    assert_eq!(config.database.url, "postgres://localhost/example");
    assert_eq!(config.database.max_connections, 3);
    Ok(())
}

#[tokio::test]
async fn invalid_config_returns_errors_without_a_database() {
    for (url, limit) in [("", 1), (" \t", 1), ("postgres://localhost/example", 0)] {
        let result = Db::new(&Config {
            url: url.into(),
            max_connections: limit,
        })
        .await;
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }
    let result = Db::new(&Config {
        url: "not a database URL".into(),
        max_connections: 1,
    })
    .await;
    assert!(matches!(
        result,
        Err(Error::DbError(sqlx::Error::Configuration(_)))
    ));
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn connection_transactions_and_savepoints_preserve_boundaries() -> Result<()> {
    let db = database(2).await?;
    let mut conn = db.conn().await?;
    sqlx::query("CREATE TEMP TABLE tlab_test_values (value integer NOT NULL)")
        .execute(conn.context().backend())
        .await?;
    insert(&mut conn.context(), 1).await?;
    {
        let mut tx = conn.begin().await?;
        insert(&mut tx.context(), 2).await?;
        {
            let mut child = tx.begin().await?;
            insert(&mut child.context(), 3).await?;
            child.rollback().await?;
        }
        assert_eq!(values(&mut tx.context()).await?, [1, 2]);
        {
            let mut child = tx.begin().await?;
            insert(&mut child.context(), 4).await?;
            child.commit().await?;
        }
        {
            let mut child = tx.begin().await?;
            insert(&mut child.context(), 5).await?;
            // SQLx must roll back this unfinished savepoint before the next query.
        }
        assert_eq!(values(&mut tx.context()).await?, [1, 2, 4]);
        {
            let mut child = tx.begin().await?;
            let error = sqlx::query("SELECT 1 / 0")
                .execute(child.context().backend())
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("22012")
            );
            child.rollback().await?;
        }
        insert(&mut tx.context(), 6).await?;
        tx.commit().await?;
    }
    assert_eq!(values(&mut conn.context()).await?, [1, 2, 4, 6]);
    {
        let mut tx = conn.begin().await?;
        let mut child = tx.begin().await?;
        insert(&mut child.context(), 7).await?;
        child.commit().await?;
        tx.rollback().await?;
    }
    {
        let mut tx = conn.begin().await?;
        insert(&mut tx.context(), 8).await?;
        // Drop a root transaction, then query the same connection.
    }
    assert_eq!(values(&mut conn.context()).await?, [1, 2, 4, 6]);
    Ok(())
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn pool_transactions_commit_rollback_and_drop() -> Result<()> {
    let db = database(2).await?;
    // A unique temporary schema isolates persistent observations across connections.
    // Identifiers contain only a fixed prefix and numeric process/backend IDs.
    let mut conn = db.conn().await?;
    let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(conn.context().backend())
        .await?;
    let schema = format!("tlab_sqlxdb_{}_{}", std::process::id(), backend);
    sqlx::QueryBuilder::<Postgres>::new("CREATE SCHEMA ")
        .push(&schema)
        .build()
        .execute(conn.context().backend())
        .await?;
    let result: Result<Vec<i32>> = async {
        sqlx::QueryBuilder::<Postgres>::new("CREATE TABLE ")
            .push(&schema)
            .push(".test_values (value integer)")
            .build()
            .execute(conn.context().backend())
            .await?;
        for (value, commit) in [(1, true), (2, false)] {
            let mut tx = db.tx().await?;
            sqlx::QueryBuilder::<Postgres>::new("INSERT INTO ")
                .push(&schema)
                .push(".test_values VALUES (")
                .push_bind(value)
                .push(")")
                .build()
                .execute(tx.context().backend())
                .await?;
            if commit {
                tx.commit().await?;
            } else {
                tx.rollback().await?;
            }
        }
        {
            let mut tx = db.tx().await?;
            sqlx::QueryBuilder::<Postgres>::new("INSERT INTO ")
                .push(&schema)
                .push(".test_values VALUES (")
                .push_bind(3_i32)
                .push(")")
                .build()
                .execute(tx.context().backend())
                .await?;
        }
        // Acquiring another transaction proves the returned pool connection is usable.
        let mut tx = db.tx().await?;
        sqlx::QueryBuilder::<Postgres>::new("INSERT INTO ")
            .push(&schema)
            .push(".test_values VALUES (")
            .push_bind(4_i32)
            .push(")")
            .build()
            .execute(tx.context().backend())
            .await?;
        tx.commit().await?;
        let actual: Vec<i32> = sqlx::QueryBuilder::<Postgres>::new("SELECT value FROM ")
            .push(&schema)
            .push(".test_values ORDER BY value")
            .build_query_scalar()
            .fetch_all(conn.context().backend())
            .await?;
        Ok(actual)
    }
    .await;
    let cleanup = sqlx::QueryBuilder::<Postgres>::new("DROP SCHEMA ")
        .push(&schema)
        .push(" CASCADE")
        .build()
        .execute(conn.context().backend())
        .await;
    let actual = result?;
    cleanup?;
    assert_eq!(actual, [1, 4]);
    Ok(())
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn query_errors_preserve_database_diagnostics() -> Result<()> {
    async fn fail(ctx: &mut Ctx<'_>) -> Result<()> {
        sqlx::query("SELECT 1 / 0").execute(ctx.backend()).await?;
        Ok(())
    }
    let db = database(1).await?;
    let mut conn = db.conn().await?;
    let error = fail(&mut conn.context()).await.unwrap_err();
    let original = error
        .source()
        .unwrap()
        .downcast_ref::<sqlx::Error>()
        .unwrap();
    assert_eq!(
        original.as_database_error().unwrap().code().as_deref(),
        Some("22012")
    );
    assert!(matches!(error, Error::DbError(_)));
    let value: i32 = sqlx::query_scalar("SELECT 42::integer")
        .fetch_one(conn.context().backend())
        .await?;
    assert_eq!(value, 42);
    Ok(())
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn status_is_available_when_pool_is_exhausted_and_recovers() -> Result<()> {
    let db = database(1).await?;
    let conn = db.conn().await?;
    let status = db.pool_status();
    assert_eq!(status.max_connections, 1);
    assert_eq!(status.size, 1);
    assert_eq!(status.idle, 0);
    let json = serde_json::to_value(status).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"max_connections": 1, "size": 1, "idle": 0})
    );
    drop(conn);
    tokio::time::timeout(Duration::from_secs(5), async {
        while db.pool_status().idle != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection was not returned to the pool");
    assert_eq!(db.pool_status().size, 1);
    Ok(())
}
