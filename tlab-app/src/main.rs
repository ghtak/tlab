mod app;
mod auth;
#[path = "rbac/slice/bootstrap_admin/cli.rs"]
mod bootstrap_admin_cli;
mod http;
mod migrations;
mod options;
#[allow(
    dead_code,
    unused_imports,
    reason = "RBAC contracts are connected incrementally"
)]
mod rbac;

use std::path::PathBuf;

use tlab::Error;

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let options = options::Options::parse(std::env::args_os().skip(1))?;
    let config = load_config()?;
    if let Some(options) = options.bootstrap_admin {
        // No tracing subscriber, JWT/TLS preparation, migration or listeners.
        match bootstrap_admin_cli::run(&config, options).await {
            Ok(created) => println!(
                "{}",
                serde_json::json!({"account_id": created.account_id, "role_id": created.role_id})
            ),
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({"code": error.code, "message": error.message})
                );
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    if let Some(path) = options.migrate {
        let database = app::db::Database::new(&config.database).await?;
        tlab::migration::run(&database, &path, migrations::CODE).await?;
        return Ok(());
    }

    http::run(app::Container::new(config).await?).await
}

fn load_config() -> tlab::Result<app::Config> {
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

    tlab::config::load(&config_path, "TLAB_APP")
}
