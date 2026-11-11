use std::{
    io::{BufRead, IsTerminal},
    sync::Arc,
};

use crate::rbac::slice::bootstrap_admin::usecase;
use crate::{
    app::{
        Config,
        db::Database,
        error::{AppError, AppResult},
    },
    auth::{LocalAccountInput, passwd_policy::NonEmptyPasswordPolicy},
    options::BootstrapAdmin,
};
use tokio::sync::Semaphore;

/// Stdin is the only secret input channel. Refuse echoing terminal input.
pub(crate) async fn run(
    config: &Config,
    options: BootstrapAdmin,
) -> AppResult<usecase::CreatedAdmin> {
    let input_error = |error| {
        AppError::internal(
            "bootstrap_input",
            "could not read password from stdin",
            error,
        )
    };
    if std::io::stdin().is_terminal() {
        return Err(input_error(std::io::Error::other(
            "pipe the password via stdin",
        )));
    }
    let mut password = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut password)
        .map_err(input_error)?;
    // Remove the line delimiter only; password whitespace remains significant.
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    let hasher = tlab::password::argon2id::Hasher::new(&config.passwd_hasher).map_err(|error| {
        AppError::internal("bootstrap_hasher", "administrator creation failed", error)
    })?;
    let database = Database::new(&config.database).await.map_err(|error| {
        AppError::internal("bootstrap_database", "administrator creation failed", error)
    })?;
    usecase::execute(
        &database,
        &NonEmptyPasswordPolicy,
        Arc::new(hasher),
        Arc::new(Semaphore::new(1)),
        LocalAccountInput {
            nickname: options.nickname,
            email: options.email,
            password,
        },
    )
    .await
}
