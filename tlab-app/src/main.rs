mod app;
mod migrations;

use std::{path::PathBuf, sync::Arc};

use axum::{Json, Router, extract::State, routing::get};
use tlab::Error;

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let migration_path = migration_argument(std::env::args_os().skip(1))?;
    match dotenvy::from_path(".env") {
        Ok(_) => {}
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::InvalidInput(
                format!("failed to load .env: {error}").into(),
            ));
        }
    }
    let config_path = std::env::var_os("TLAB_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"));

    let config: app::Config = tlab::config::load(&config_path, "TLAB_APP")?;
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    if let Some(path) = migration_path {
        let database = app::db::Database::new(&config.database).await?;
        tlab::migration::run(&database, &path, migrations::CODE).await?;
        return Ok(());
    }

    let container = Arc::new(app::Container::new(config).await?);

    let mut router: Router<Arc<app::Container>> = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/db_status", get(db_status));

    if let Some(files) = &container.config.server.static_files {
        router = router.merge(files.router()?);
    }

    let router: Router = router.with_state(container.clone());

    tracing::info!("application starting");
    tokio::try_join!(
        container.http.run(router.clone()),
        container.https.run(router),
        async {
            shutdown_signal().await?;
            tracing::info!("application stopping");
            for server in [&container.http, &container.https] {
                server.graceful_shutdown();
            }
            Ok::<(), Error>(())
        }
    )?;
    Ok(())
}

fn migration_argument(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> tlab::Result<Option<PathBuf>> {
    match args.next() {
        None => Ok(None),
        Some(option) if option == "--migrate" => {
            let path = args
                .next()
                .filter(|value| !value.is_empty() && !value.to_string_lossy().starts_with('-'))
                .ok_or_else(|| Error::InvalidInput("--migrate requires a directory".into()))?;
            if args.next().is_some() {
                return Err(Error::InvalidInput(
                    "unexpected command line argument".into(),
                ));
            }
            Ok(Some(path.into()))
        }
        Some(_) => Err(Error::InvalidInput("unknown command line argument".into())),
    }
}

async fn db_status(State(container): State<Arc<app::Container>>) -> Json<tlab::sqlxdb::PoolStatus> {
    Json(container.database.pool_status())
}

async fn shutdown_signal() -> tlab::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;

    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn migration_command_arguments() {
        let parse = |args: &[&str]| migration_argument(args.iter().map(std::ffi::OsString::from));
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(
            parse(&["--migrate", "migration"]).unwrap(),
            Some(PathBuf::from("migration"))
        );
        for args in [
            &["--migrate"][..],
            &["--unknown"],
            &["--migrate", "x", "extra"],
            &["--migrate", "--migrate"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
