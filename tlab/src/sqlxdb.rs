//! SQLx pools and borrowed query contexts for connections and transactions.
//!
//! PostgreSQL on Tokio is the initially supported backend. Queries and row
//! models belong to the caller; pass [`Context::backend`] to SQLx query APIs.
//! Use SQLx 0.9 in consuming applications so the exposed types match.

use sqlx::{Acquire, Connection};

use crate::{Error, Result};

/// Required pool settings, composable with [`crate::config::load`].
///
/// No `Debug` implementation is provided because URLs may contain credentials.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct Config {
    pub url: String,
    pub max_connections: u32,
}

/// Observed connection counts, not an atomic snapshot of concurrent activity.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PoolStatus {
    pub max_connections: u32,
    pub size: u32,
    pub idle: usize,
}

/// An owned SQLx connection pool.
pub struct Database<DB: sqlx::Database> {
    inner: sqlx::Pool<DB>,
}

impl<DB: sqlx::Database> Database<DB> {
    /// Connect to the database, opening at least one connection.
    ///
    /// Requires a Tokio runtime. Blank URLs and zero connection limits return
    /// [`Error::InvalidInput`]; SQLx failures return [`Error::DbError`]. Options
    /// other than the connection limit, including timeouts, follow SQLx defaults.
    pub async fn new(config: &Config) -> Result<Self> {
        if config.url.trim().is_empty() {
            return Err(Error::InvalidInput("database URL must not be blank".into()));
        }
        if config.max_connections == 0 {
            return Err(Error::InvalidInput(
                "max_connections must be greater than zero".into(),
            ));
        }
        let inner = sqlx::pool::PoolOptions::<DB>::new()
            .max_connections(config.max_connections)
            .connect(&config.url)
            .await?;
        Ok(Self { inner })
    }

    /// Read pool counts without acquiring a connection.
    pub fn pool_status(&self) -> PoolStatus {
        let size = self.inner.size();
        PoolStatus {
            max_connections: self.inner.options().get_max_connections(),
            size,
            idle: self.inner.num_idle().min(size as usize),
        }
    }

    /// Check out one connection. Dropping it returns it to the SQLx pool.
    pub async fn conn(&self) -> Result<Conn<DB>> {
        Ok(Conn {
            inner: self.inner.acquire().await?,
        })
    }

    /// Start a root transaction owning a pooled connection.
    ///
    /// The transaction does not borrow this handle. Unfinished transactions
    /// start SQLx rollback on drop; use explicit rollback to await completion.
    pub async fn tx(&self) -> Result<Tx<'static, DB>> {
        Ok(Tx {
            inner: self.inner.begin().await?,
        })
    }
}

/// A checked-out connection, exclusively borrowed by its active context or tx.
pub struct Conn<DB: sqlx::Database> {
    inner: sqlx::pool::PoolConnection<DB>,
}

impl<DB: sqlx::Database> Conn<DB> {
    /// Begin a transaction borrowing this connection.
    pub async fn begin(&mut self) -> Result<Tx<'_, DB>> {
        Ok(Tx {
            inner: self.inner.as_mut().begin().await?,
        })
    }

    /// Borrow the connection for SQLx queries.
    pub fn context(&mut self) -> Context<'_, DB> {
        Context {
            inner: self.inner.as_mut(),
        }
    }
}

/// A root transaction or nested savepoint managed by SQLx.
///
/// Dropping an unfinished transaction starts rollback through SQLx, without
/// awaiting it or reporting its errors. Explicitly roll back when that result
/// matters. Commit errors or cancellation may leave the server outcome unknown;
/// this wrapper does not retry or guarantee that the operation was unapplied.
/// Do not issue manual transaction-control SQL through the exposed connection.
///
/// An active child prevents completion of its parent:
///
/// ```compile_fail,E0505
/// # async fn example(mut tx: tlab::sqlxdb::Tx<'_, sqlx::Postgres>) -> tlab::Result<()> {
/// let child = tx.begin().await?;
/// tx.commit().await?;
/// child.rollback().await?;
/// # Ok(())
/// # }
/// ```
///
/// Completed handles cannot be reused:
///
/// ```compile_fail,E0382
/// # async fn example(tx: tlab::sqlxdb::Tx<'_, sqlx::Postgres>) -> tlab::Result<()> {
/// tx.commit().await?;
/// tx.rollback().await?;
/// # Ok(())
/// # }
/// ```
pub struct Tx<'c, DB: sqlx::Database> {
    inner: sqlx::Transaction<'c, DB>,
}

impl<DB: sqlx::Database> Tx<'_, DB> {
    /// Start a savepoint borrowing this transaction.
    pub async fn begin(&mut self) -> Result<Tx<'_, DB>> {
        Ok(Tx {
            inner: Acquire::begin(&mut self.inner).await?,
        })
    }

    /// Commit the root transaction or release this savepoint.
    pub async fn commit(self) -> Result<()> {
        self.inner.commit().await?;
        Ok(())
    }

    /// Roll back the root transaction or this savepoint.
    pub async fn rollback(self) -> Result<()> {
        self.inner.rollback().await?;
        Ok(())
    }

    /// Borrow this transaction's connection for SQLx queries.
    pub fn context(&mut self) -> Context<'_, DB> {
        Context {
            inner: &mut self.inner,
        }
    }
}

/// A short mutable borrow of the active connection for caller-owned queries.
///
/// ```no_run
/// async fn read(ctx: &mut tlab::sqlxdb::Context<'_, sqlx::Postgres>) -> tlab::Result<i32> {
///     Ok(sqlx::query_scalar("SELECT 42::integer")
///         .fetch_one(ctx.backend()).await?)
/// }
/// ```
///
/// A context cannot outlive its connection:
///
/// ```compile_fail,E0597
/// # async fn example(db: &tlab::sqlxdb::Database<sqlx::Postgres>) -> tlab::Result<()> {
/// let mut ctx = {
///     let mut conn = db.conn().await?;
///     conn.context()
/// };
/// sqlx::query("SELECT 1").execute(ctx.backend()).await?;
/// # Ok(())
/// # }
/// ```
pub struct Context<'c, DB: sqlx::Database> {
    inner: &'c mut DB::Connection,
}

impl<DB: sqlx::Database> Context<'_, DB> {
    /// Reborrow the SQLx connection for `execute`, `fetch_one`, and similar APIs.
    pub fn backend(&mut self) -> &mut DB::Connection {
        self.inner
    }
}
