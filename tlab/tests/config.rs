use std::error::Error as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use tempfile::TempDir;
use tlab::{Error, config};

mod server {
    #[derive(Debug, PartialEq, serde::Deserialize)]
    pub struct Config {
        pub host: String,
        pub port: u16,
    }
}

mod worker {
    #[derive(Debug, PartialEq, serde::Deserialize)]
    pub struct Config {
        pub enabled: bool,
        #[serde(default)]
        pub retries: u32,
        pub label: Option<String>,
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfig {
    pub server_config: server::Config,
    pub worker_config: worker::Config,
}

const YAML: &str =
    "server_config:\n  host: localhost\n  port: 8080\nworker_config:\n  enabled: true\n";
const TOML: &str =
    "[server_config]\nhost = 'localhost'\nport = 8080\n[worker_config]\nenabled = true\n";
const JSON: &str =
    r#"{"server_config":{"host":"localhost","port":8080},"worker_config":{"enabled":true}}"#;

fn fixture(name: &str, contents: &str) -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, contents).unwrap();
    (dir, path)
}

fn expected() -> AppConfig {
    AppConfig {
        server_config: server::Config {
            host: "localhost".into(),
            port: 8080,
        },
        worker_config: worker::Config {
            enabled: true,
            retries: 0,
            label: None,
        },
    }
}

#[test]
fn loads_module_configs_defaults_and_options_from_supported_files() {
    for (name, contents) in [
        ("app.yaml", YAML),
        ("app.yml", YAML),
        ("app.toml", TOML),
        ("app.json", JSON),
    ] {
        let (_dir, path) = fixture(name, contents);
        let actual: AppConfig = config::load(path, "TLAB_FILE_TEST").unwrap();
        assert_eq!(actual, expected(), "{name}");
    }
}

#[test]
fn delegates_basename_discovery_and_prefers_the_named_file() {
    let (dir, _path) = fixture("app.toml", TOML);
    let actual: AppConfig = config::load(dir.path().join("app"), "TLAB_FILE_TEST").unwrap();
    assert_eq!(actual, expected());

    let yaml_path = dir.path().join("app.yaml");
    fs::write(&yaml_path, YAML.replace("8080", "9090")).unwrap();
    let actual: AppConfig = config::load(yaml_path, "TLAB_FILE_TEST").unwrap();
    assert_eq!(actual.server_config.port, 9090);
}

#[test]
fn rejects_an_empty_prefix_as_invalid_input() {
    let (_dir, path) = fixture("app.yaml", YAML);
    let error = config::load::<AppConfig>(path, "").unwrap_err();
    assert!(matches!(&error, Error::InvalidInput(message) if message.contains("prefix")));
    assert!(error.source().is_none());
}

#[test]
fn preserves_configuration_and_file_io_causes() {
    let dir = tempfile::tempdir().unwrap();
    let error =
        config::load::<AppConfig>(dir.path().join("missing.yaml"), "TLAB_FILE_TEST").unwrap_err();
    assert!(matches!(&error, Error::Config(_)));
    let source = error.source().unwrap();
    assert!(source.is::<::config::ConfigError>());
    let ::config::ConfigError::Foreign(cause) = source.downcast_ref().unwrap() else {
        panic!("expected a stored file access error");
    };
    let io_error = cause.downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound);

    for (name, contents) in [
        ("invalid.yaml", "server_config: [unterminated"),
        ("invalid.toml", "[server_config"),
    ] {
        let (_dir, path) = fixture(name, contents);
        let error = config::load::<AppConfig>(path, "TLAB_FILE_TEST").unwrap_err();
        let Error::Config(::config::ConfigError::FileParse { uri, cause }) = error else {
            panic!("expected a stored parsing error");
        };
        assert!(uri.unwrap().contains(name));
        assert!(!cause.to_string().is_empty());
    }
}

#[test]
fn rejects_unrecognized_formats_and_invalid_config_values() {
    for (name, contents, message) in [
        ("app.unsupported", YAML.to_owned(), "supported file format"),
        ("app.yaml", "server_config: {}".to_owned(), "host"),
        ("app.yaml", YAML.replace("8080", "not-a-port"), "port"),
        ("app.yaml", YAML.replace("8080", "70000"), "port"),
        (
            "app.yaml",
            format!("{YAML}unexpected: true\n"),
            "unknown field",
        ),
    ] {
        let (_dir, path) = fixture(name, &contents);
        let error = config::load::<AppConfig>(path, "TLAB_FILE_TEST").unwrap_err();
        assert!(matches!(&error, Error::Config(_)), "{name}: {error}");
        assert!(error.to_string().contains(message), "{name}: {error}");
    }
}

fn run_environment_scenario(path: &Path, scenario: &str, variables: &[(&str, &str)]) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "environment_scenario",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("TLAB_CONFIG_PATH", path)
        .env("TLAB_CONFIG_SCENARIO", scenario)
        .envs(variables.iter().copied())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scenario {scenario}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn environment_overrides_nested_values_and_preserves_strings() {
    let (_dir, path) = fixture("app.yaml", YAML);
    run_environment_scenario(
        &path,
        "overrides",
        &[
            ("APP_SERVER_CONFIG__PORT", "9090"),
            ("APP_WORKER_CONFIG__ENABLED", "false"),
            ("APP_WORKER_CONFIG__LABEL", "00123"),
            ("OTHER_SERVER_CONFIG__HOST", "unrelated"),
        ],
    );
    run_environment_scenario(&path, "empty", &[("APP_SERVER_CONFIG__HOST", "")]);
}

#[test]
fn environment_can_fill_missing_fields_but_cannot_bypass_invalid_inputs() {
    let (_dir, path) = fixture("app.yaml", &YAML.replace("  port: 8080\n", ""));
    run_environment_scenario(&path, "supplement", &[("APP_SERVER_CONFIG__PORT", "8080")]);

    let (_dir, path) = fixture("app.toml", TOML);
    for variable in ["APP_SERVER_CONFIG__PORT", "APP_WORKER_CONFIG__ENABLED"] {
        run_environment_scenario(&path, "invalid", &[(variable, "not-a-value")]);
    }

    let variables = [
        ("APP_SERVER_CONFIG__HOST", "localhost"),
        ("APP_SERVER_CONFIG__PORT", "8080"),
        ("APP_WORKER_CONFIG__ENABLED", "true"),
    ];
    run_environment_scenario(&path.with_file_name("missing.toml"), "invalid", &variables);
    let (_dir, path) = fixture("malformed.yaml", "server_config: [unterminated");
    run_environment_scenario(&path, "invalid", &variables);
}

#[test]
#[ignore = "executed in isolated child processes by the environment tests"]
fn environment_scenario() {
    let path = std::env::var_os("TLAB_CONFIG_PATH").unwrap();
    let scenario = std::env::var("TLAB_CONFIG_SCENARIO").unwrap();
    let result = config::load::<AppConfig>(Path::new(&path), "APP");
    if scenario == "invalid" {
        let error = result.unwrap_err();
        assert!(matches!(error, Error::Config(_)));
        return;
    }

    let actual = result.unwrap();
    let mut expected = expected();
    match scenario.as_str() {
        "overrides" => {
            expected.server_config.port = 9090;
            expected.worker_config.enabled = false;
            expected.worker_config.label = Some("00123".into());
        }
        "empty" => expected.server_config.host.clear(),
        "supplement" => {}
        _ => panic!("unknown scenario: {scenario}"),
    }
    assert_eq!(actual, expected);
}
