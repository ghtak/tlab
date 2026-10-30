use std::{
    collections::BTreeMap,
    error::Error as _,
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use serde::Deserialize;
use tlab::{Error, config::Loader};

struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    fn new(extension: &str, contents: Option<&str>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tlab-config-{}-{timestamp}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let file = Self {
            path: directory.join(format!("settings.{extension}")),
        };
        if let Some(contents) = contents {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file.path)
                .unwrap()
                .write_all(contents.as_bytes())
                .unwrap();
        }
        file
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(self.path.parent().unwrap()) {
            if !std::thread::panicking() {
                panic!("failed to remove test config: {error}");
            }
            eprintln!("failed to remove test config during panic: {error}");
        }
    }
}

#[derive(Debug, Deserialize)]
struct Settings {
    server: Server,
}

#[derive(Debug, Deserialize)]
struct Server {
    name: String,
    port: u16,
    enabled: bool,
    directory: PathBuf,
}

#[test]
fn loads_values_from_file() {
    let file = ConfigFile::new("toml", Some("name = 'tlab'\n"));

    let config = Loader::from_file(file.path.clone()).load().unwrap();

    assert_eq!(config.get_string("name").unwrap(), "tlab");
    assert_eq!(fs::read_to_string(&file.path).unwrap(), "name = 'tlab'\n");
}

#[test]
fn missing_file_is_an_error_and_is_not_created() {
    let file = ConfigFile::new("toml", None);

    let error = Loader::from_file(file.path.clone()).load().unwrap_err();

    assert!(matches!(error, Error::Internal(_)));
    assert!(error.source().is_some());
    assert!(!file.path.exists());
}

#[test]
fn malformed_file_preserves_error_source() {
    let file = ConfigFile::new("toml", Some("name = ["));

    let error = Loader::from_file(file.path.clone()).load().unwrap_err();

    assert!(matches!(error, Error::Internal(_)));
    assert!(error.source().is_some());
}

#[test]
fn deserializes_yaml_into_caller_type() {
    let file = ConfigFile::new(
        "yaml",
        Some("server:\n  name: tlab\n  port: 8080\n  enabled: true\n  directory: logs\n"),
    );

    let settings: Settings = Loader::from_file(file.path.clone())
        .try_deserialize()
        .unwrap();

    assert_eq!(settings.server.name, "tlab");
    assert_eq!(settings.server.port, 8080);
    assert!(settings.server.enabled);
    assert_eq!(settings.server.directory, PathBuf::from("logs"));
}

#[test]
fn deserialization_failure_preserves_context_and_cause() {
    let file = ConfigFile::new(
        "toml",
        Some("[server]\nname = 'tlab'\nport = 'invalid'\nenabled = true\ndirectory = 'logs'\n"),
    );

    let error = Loader::from_file(file.path.clone())
        .try_deserialize::<Settings>()
        .unwrap_err();

    assert!(matches!(error, Error::Internal(_)));
    assert_eq!(
        error.to_string(),
        "internal error: failed to deserialize config"
    );
    let source = error.source().unwrap();
    assert_eq!(source.to_string(), "failed to deserialize config");
    assert!(source.source().is_some());
}

const PREFIX: &str = "TLAB_LOADER_TEST";
const CHILD_MARKER: &str = "TLAB_CONFIG_CHILD_OK";

#[test]
fn environment_contract_in_child_process() {
    let inherited: BTreeMap<_, _> = std::env::vars_os()
        .filter(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with(PREFIX)
        })
        .collect();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "environment_child",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    for key in inherited.keys() {
        command.env_remove(key);
    }
    let output = command
        .env("TLAB_LOADER_TEST_RUN_CHILD", "1")
        .env("TLAB_LOADER_TEST_SERVER__NAME", "environment")
        .env("TLAB_LOADER_TEST_SERVER__PORT", "9090")
        .env("TLAB_LOADER_TEST_SERVER__ENABLED", "true")
        .env("TLAB_LOADER_TEST_SERVER__DIRECTORY", "env-logs")
        .env("TLAB_LOADER_TESTOTHER_SECRET", "excluded")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "child failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(CHILD_MARKER), "child did not run: {stdout}");
    let after: BTreeMap<_, _> = std::env::vars_os()
        .filter(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with(PREFIX)
        })
        .collect();
    assert_eq!(inherited, after);
}

#[test]
#[ignore = "executed with isolated environment by environment_contract_in_child_process"]
fn environment_child() {
    assert_eq!(std::env::var("TLAB_LOADER_TEST_RUN_CHILD").unwrap(), "1");

    let config = Loader::from_env(PREFIX).load().unwrap();
    assert_eq!(config.get_string("server.name").unwrap(), "environment");
    assert!(config.get_string("other_secret").is_err());
    let settings: Settings = Loader::from_env(PREFIX).try_deserialize().unwrap();
    assert_eq!(settings.server.port, 9090);
    assert!(settings.server.enabled);
    assert_eq!(settings.server.directory, PathBuf::from("env-logs"));

    let file = ConfigFile::new(
        "toml",
        Some("file_only = 'retained'\n[server]\nname = 'file'\nport = 8080\nenabled = false\n"),
    );
    let config = Loader::from_file(file.path.clone())
        .env_prefix("unused")
        .env_prefix(PREFIX)
        .load()
        .unwrap();
    assert_eq!(config.get_string("server.name").unwrap(), "environment");
    assert_eq!(config.get_int("server.port").unwrap(), 9090);
    assert!(config.get_bool("server.enabled").unwrap());
    assert_eq!(config.get_string("file_only").unwrap(), "retained");

    let missing = ConfigFile::new("toml", None);
    assert!(matches!(
        Loader::from_file(missing.path.clone())
            .env_prefix(PREFIX)
            .load(),
        Err(Error::Internal(_))
    ));
    assert!(!missing.path.exists());
    let malformed = ConfigFile::new("toml", Some("name = ["));
    assert!(matches!(
        Loader::from_file(malformed.path.clone())
            .env_prefix(PREFIX)
            .try_deserialize::<Settings>(),
        Err(Error::Internal(_))
    ));
    println!("{CHILD_MARKER}");
}
