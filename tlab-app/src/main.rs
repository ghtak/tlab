mod app;
mod auth;
mod http;
mod migrations;
mod options;

use std::path::PathBuf;

use tlab::Error;

#[tokio::main]
async fn main() -> tlab::Result<()> {
    let config = load_config()?;
    let _tracing_guard = tlab::tracing::initialize(&config.tracing)?;

    let options = options::Options::parse(std::env::args_os().skip(1))?;
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
