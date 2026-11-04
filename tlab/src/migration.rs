//! Repeatable, explicitly ordered PostgreSQL SQL and code migrations.
//!
//! No history, locking, retry, or rollback is provided. Callers must serialize
//! runs and ensure every operation is safe to repeat.

use std::{future::Future, path::Path, pin::Pin};

use crate::{
    Error, Result,
    sqlxdb::{Context, Database},
};
use sqlx::Postgres;

/// A code migration borrowing the current database context until it completes.
pub type MigrationFn = for<'a, 'c> fn(
    &'a mut Context<'c, Postgres>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// An explicitly numbered code migration.
pub struct CodeMigration {
    pub sequence: u64,
    pub name: &'static str,
    pub run: MigrationFn,
}

enum Action {
    Sql(String),
    Code(MigrationFn),
}

struct Step {
    sequence: u64,
    name: String,
    action: Action,
}

fn collect(path: &Path, code: &[CodeMigration]) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "sql")
        {
            continue;
        }
        let filename = entry.file_name();
        let filename = filename
            .to_str()
            .ok_or_else(|| Error::InvalidInput("migration filename must be UTF-8".into()))?;
        let (number, name) = filename
            .strip_suffix(".sql")
            .unwrap()
            .split_once('_')
            .ok_or_else(|| {
                Error::InvalidInput(format!("invalid migration filename: {filename}").into())
            })?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || name.is_empty() {
            return Err(Error::InvalidInput(
                format!("invalid migration filename: {filename}").into(),
            ));
        }
        let sequence = number.parse::<u64>().map_err(|_| {
            Error::InvalidInput(format!("invalid migration sequence: {filename}").into())
        })?;
        steps.push(Step {
            sequence,
            name: filename.to_owned(),
            action: Action::Sql(std::fs::read_to_string(entry.path())?),
        });
    }
    for item in code {
        if item.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "code migration name must not be blank".into(),
            ));
        }
        steps.push(Step {
            sequence: item.sequence,
            name: item.name.to_owned(),
            action: Action::Code(item.run),
        });
    }
    steps.sort_by_key(|step| step.sequence);
    for pair in steps.windows(2) {
        if pair[0].sequence == pair[1].sequence {
            return Err(Error::InvalidInput(
                format!("duplicate migration sequence: {}", pair[0].sequence).into(),
            ));
        }
    }
    Ok(steps)
}

/// Run every SQL file and registered code migration in ascending sequence.
///
/// Files are read and all sequences validated before any step is executed.
/// A failed step stops execution; prior effects may have already committed.
pub async fn run(db: &Database<Postgres>, path: &Path, code: &[CodeMigration]) -> Result<()> {
    let steps = collect(path, code)?;
    let mut conn = db.conn().await?;
    for step in steps {
        tracing::info!(sequence = step.sequence, name = %step.name, "migration starting");
        let result = {
            let mut context = conn.context();
            match step.action {
                // Migration SQL is intentionally executable input supplied by the operator.
                Action::Sql(sql) => sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(context.backend())
                    .await
                    .map(|_| ())
                    .map_err(Error::from),
                Action::Code(run) => run(&mut context).await,
            }
        };
        if let Err(error) = result {
            tracing::error!(sequence = step.sequence, name = %step.name, "migration failed");
            return Err(error);
        }
        tracing::info!(sequence = step.sequence, name = %step.name, "migration completed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop<'a, 'c>(
        _: &'a mut Context<'c, Postgres>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    #[test]
    fn validates_and_orders_all_sources_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("0010_last.sql"), "SELECT 1").unwrap();
        std::fs::write(dir.path().join("0001_first.sql"), "SELECT 1").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "").unwrap();
        let code = [CodeMigration {
            sequence: 2,
            name: "code",
            run: noop,
        }];
        assert_eq!(
            collect(dir.path(), &code)
                .unwrap()
                .iter()
                .map(|s| s.sequence)
                .collect::<Vec<_>>(),
            [1, 2, 10]
        );
        std::fs::write(dir.path().join("2_duplicate.sql"), "SELECT 1").unwrap();
        assert!(collect(dir.path(), &code).is_err());
        std::fs::remove_file(dir.path().join("2_duplicate.sql")).unwrap();
        std::fs::write(dir.path().join("bad.sql"), "SELECT 1").unwrap();
        assert!(collect(dir.path(), &code).is_err());
    }
}
