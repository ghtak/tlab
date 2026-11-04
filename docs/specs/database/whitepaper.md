- sqlx 기반
- sqlxdb.rs
- 다음 구성에 기반하게 작업하려는 목적
```
/// A reusable SQLx connection pool.
#[derive(Debug)]
pub struct Database<DB: sqlx::Database> {
    inner: sqlx::Pool<DB>,
}

/// Current state of one SQLx pool.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PoolStatus {
    pub max_connections: u32,
    pub size: u32,
    pub idle: usize,
}

impl<DB: sqlx::Database> Database<DB> {
    /// Read connection counts without acquiring a connection.
    pub fn pool_status(&self) -> PoolStatus {
        let size = self.inner.size();
        PoolStatus {
            max_connections: self.inner.options().get_max_connections(),
            size,
            idle: self.inner.num_idle().min(size as usize),
        }
    }

    /// Connect to the database and create a pool from `config`.
    pub async fn new(config: &sqlxdb::Config) -> crate::Result<Self> {
        let inner = sqlx::pool::PoolOptions::<DB>::new()
            .max_connections(config.max_connections)
            .connect(&config.url)
            .await
            .map_err(map_error)?;
        Ok(Self { inner })
    }

    /// Check out one connection from the pool.
    pub async fn conn(&self) -> crate::Result<Conn<DB>> {
        let inner = self.inner.acquire().await.map_err(map_error)?;
        Ok(Conn { inner })
    }

    /// Start a root transaction on a pooled connection.
    pub async fn tx(&self) -> crate::Result<Tx<'_, DB>> {
        let tx = self.inner.begin().await.map_err(map_error)?;
        Ok(Tx { inner: tx })
    }
}

/// A checked-out pooled connection.
///
/// Use [`Self::context`] to execute a query without a transaction, or
/// [`Self::begin`] to start one on this connection.
#[derive(Debug)]
pub struct Conn<DB: sqlx::Database> {
    inner: sqlx::pool::PoolConnection<DB>,
}

impl<DB: sqlx::Database> Conn<DB> {
    /// Start a transaction on this connection.
    pub async fn begin(&mut self) -> crate::Result<Tx<'_, DB>> {
        let inner = self.inner.begin().await.map_err(sqlxdb::map_error)?;
        Ok(Tx { inner })
    }
    /// Borrow the SQLx backend for query execution.
    pub fn context(&mut self) -> Context<'_, DB> {
        Context {
            inner: self.inner.as_mut(),
        }
    }
}

/// A transaction that can be committed, rolled back, or nested with a savepoint.
#[derive(Debug)]
pub struct Tx<'c, DB: sqlx::Database> {
    inner: sqlx::Transaction<'c, DB>,
}

impl<'c, DB: sqlx::Database> Tx<'c, DB> {
    /// Start a nested transaction/savepoint.
    pub async fn begin(&mut self) -> crate::Result<Tx<'_, DB>> {
        let inner = self.inner.begin().await.map_err(sqlxdb::map_error)?;
        Ok(Tx { inner })
    }

    /// Commit this transaction.
    pub async fn commit(self) -> crate::Result<()> {
        self.inner.commit().await.map_err(sqlxdb::map_error)?;
        Ok(())
    }

    /// Roll back this transaction.
    pub async fn rollback(self) -> crate::Result<()> {
        self.inner.rollback().await.map_err(sqlxdb::map_error)?;
        Ok(())
    }

    /// Borrow the transaction's SQLx backend for query execution.
    pub fn context(&mut self) -> Context<'_, DB> {
        Context {
            inner: &mut *self.inner,
        }
    }
}

/// A short-lived wrapper that exposes the active SQLx executor.
#[derive(Debug)]
pub struct Context<'c, DB: sqlx::Database> {
    inner: &'c mut DB::Connection,
}

impl<DB: sqlx::Database> Context<'_, DB> {
    /// Return the SQLx connection to pass to `execute`, `fetch_one`, and similar APIs.
    pub fn backend(&mut self) -> &mut DB::Connection {
        self.inner
    }
}

-- main.rs

pub type AppDB = tlab::sqlxdb::Database<sqlx::Postgres>;
pub type AppDBCtx<'a> = tlab::sqlxdb::Context<'a, sqlx::Postgres>;
pub type AppQueryBuilder = sqlx::QueryBuilder<sqlx::Postgres>;

-- repository
pub async fn delete_by_id(context: &mut AppDBCtx<'_>, id: i64) -> tlab::Result<bool> {
    let result = sqlx::query("DELETE FROM tlab_permission WHERE id = $1")
        .bind(id)
        .execute(context.backend())
        .await
        .map_err(sqlxdb::postgres::map_error)?;
    Ok(result.rows_affected() > 0)
}

```
- 즉 Context 로 얇은 레이어를 추가하여 일반화된 접근을 제공하려는 목적, 호출되는 쪽에선 tx 는 conn 이든
- 위안은 초안이고 방향성의 정의하는 목적이니 저것을 꼭 쫒아 가야 하는것은 아님
- 다음과 같이 Oracledb 등 추가 레이어 연계를 위한 구조라고 보면됨

```
//! Oracle connection-pool wrapper for the synchronous `oracledb` driver.
//!
//! Use [`Database::conn`] or [`Database::tx`] from async code. Execute driver
//! calls through [`Context::run_blocking`] so they do not block the Tokio runtime.

use oracledb::{Connection, Pool};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
/// Oracle connection-pool settings passed to [`Database::new`].
#[derive(Debug, serde::Deserialize)]
pub struct Config {
    /// Oracle listener host name.
    pub host: String,
    /// Oracle listener port.
    pub port: u16,
    /// Oracle service name, such as `FREEPDB1`.
    pub service: String,
    /// Database user name.
    pub username: String,
    /// Database user password.
    pub password: String,
    /// Maximum number of checked-out connections.
    pub max_connections: usize,
}
/// A thread-safe Oracle pool with an async checkout limit.
#[derive(Clone)]
pub struct Database {
    pool: Arc<Pool>,
    permits: Arc<Semaphore>,
}
impl Database {
    /// Create an Oracle pool from the supplied connection settings.
    pub fn new(config: Config) -> crate::Result<Self> {
        let connect_string = format!("{}:{}/{}", config.host, config.port, config.service);
        let pool_config = oracledb::PoolConfig::default()
            .set_credentials(&config.username, &config.password)
            .set_connect_string(&connect_string)
            .map_err(map_error)?
            .set_min_connections(0)
            .set_max_connections(config.max_connections);
        let pool = oracledb::create_pool(pool_config).map_err(map_error)?;
        Ok(Self {
            pool: Arc::new(pool),
            permits: Arc::new(Semaphore::new(config.max_connections)),
        })
    }
    /// Check out one connection without blocking the async runtime.
    pub async fn conn(&self) -> crate::Result<Conn> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(map_semaphore_error)?;
        let pool = Arc::clone(&self.pool);
        let connection = tokio::task::spawn_blocking(move || pool.acquire())
            .await
            .map_err(map_join_error)?
            .map_err(map_error)?;
        Ok(Conn {
            lease: Arc::new(Lease {
                connection,
                _permit: permit,
            }),
        })
    }
    /// Check out one connection and use it as a root transaction.
    pub async fn tx(&self) -> crate::Result<Tx> {
        Ok(Tx::root(self.conn().await?.lease))
    }
}
struct Lease {
    connection: Connection,
    _permit: OwnedSemaphorePermit,
}
/// A checked-out Oracle connection.
pub struct Conn {
    lease: Arc<Lease>,
}
impl Conn {
    /// Clone a context for running statements on this connection.
    pub fn context(&self) -> Context {
        Context(Arc::clone(&self.lease))
    }
    /// Treat this connection as a root transaction.
    pub fn begin(&self) -> Tx {
        Tx::root(Arc::clone(&self.lease))
    }
}
/// A root transaction or nested savepoint.
pub struct Tx {
    lease: Arc<Lease>,
    savepoint: Option<String>,
    depth: u32,
}
impl Tx {
    fn root(lease: Arc<Lease>) -> Self {
        Self {
            lease,
            savepoint: None,
            depth: 0,
        }
    }
    /// Clone a context for running statements in this transaction.
    pub fn context(&self) -> Context {
        Context(Arc::clone(&self.lease))
    }
    /// Create a nested transaction backed by an Oracle savepoint.
    pub async fn begin(&self) -> crate::Result<Self> {
        let depth = self.depth + 1;
        let savepoint = format!("tlab_sp_{depth}");
        let lease = Arc::clone(&self.lease);
        let savepoint_sql = format!("SAVEPOINT {savepoint}");
        tokio::task::spawn_blocking(move || lease.connection.execute(&savepoint_sql, &[]))
            .await
            .map_err(map_join_error)?
            .map_err(map_error)?;
        Ok(Self {
            lease: Arc::clone(&self.lease),
            savepoint: Some(savepoint),
            depth,
        })
    }
    /// Commit a root transaction. Nested transactions are released implicitly.
    pub async fn commit(self) -> crate::Result<()> {
        if self.savepoint.is_none() {
            tokio::task::spawn_blocking(move || self.connection().commit())
                .await
                .map_err(map_join_error)?
                .map_err(map_error)?;
        }
        Ok(())
    }
    /// Roll back a root transaction or roll back to a nested savepoint.
    pub async fn rollback(self) -> crate::Result<()> {
        if let Some(savepoint) = &self.savepoint {
            let lease = Arc::clone(&self.lease);
            let rollback_sql = format!("ROLLBACK TO SAVEPOINT {savepoint}");
            tokio::task::spawn_blocking(move || lease.connection.execute(&rollback_sql, &[]))
                .await
                .map_err(map_join_error)?
                .map_err(map_error)?;
        } else {
            tokio::task::spawn_blocking(move || self.connection().rollback())
                .await
                .map_err(map_join_error)?
                .map_err(map_error)?;
        }
        Ok(())
    }
    fn connection(&self) -> &Connection {
        &self.lease.connection
    }
}
/// A cloneable view of the checked-out synchronous Oracle connection.
pub struct Context(Arc<Lease>);
impl Context {
    /// Return the synchronous driver connection.
    ///
    /// Prefer [`Self::run_blocking`] from async code.
    pub fn backend(&self) -> &Connection {
        &self.0.connection
    }

    /// Run a synchronous driver operation on Tokio's blocking thread pool.
    ///
    /// The closure receives the checked-out `oracledb::Connection` and its result
    /// should be mapped with this module's database error handling.
    pub async fn run_blocking<T, F>(&self, f: F) -> crate::Result<T>
    where
        F: FnOnce(&Connection) -> crate::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let lease = Arc::clone(&self.0);
        let span = tracing::Span::current();
        tokio::task::spawn_blocking(move || span.in_scope(|| f(&lease.connection)))
            .await
            .map_err(map_join_error)?
    }
}
```