use std::{fs, process::Command};

use tlab::tracing::{Config, ConsoleTraceConfig, FileTraceConfig, initialize};

#[test]
fn loads_minimal_settings_and_explicit_overrides() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracing.toml");
    let minimal = "[console]\n[file]\ndirectory = './logs'\nfilename_prefix = 'app.log'\n";
    fs::write(&path, minimal).unwrap();
    let config: Config = tlab::config::load(&path, "TLAB_TRACING_DEFAULTS_TEST").unwrap();
    let console = config.console.unwrap();
    let file = config.file.unwrap();
    assert_eq!(console.filter, "info");
    assert_eq!(file.filter, "info");
    assert_eq!(
        console.buffered_lines_limit,
        tracing_appender::non_blocking::DEFAULT_BUFFERED_LINES_LIMIT
    );
    assert_eq!(
        file.buffered_lines_limit,
        tracing_appender::non_blocking::DEFAULT_BUFFERED_LINES_LIMIT
    );
    assert!(console.lossy && file.lossy);
    assert!(!console.ansi);
    assert_eq!(file.directory, std::path::Path::new("./logs"));
    assert_eq!(file.filename_prefix, "app.log");

    fs::write(
        &path,
        r#"
[console]
filter = "debug"
buffered_lines_limit = 32
lossy = false
ansi = true

[file]
directory = "./other"
filename_prefix = "other.log"
filter = "warn"
buffered_lines_limit = 64
lossy = false
"#,
    )
    .unwrap();
    let config: Config = tlab::config::load(&path, "TLAB_TRACING_DEFAULTS_TEST").unwrap();
    let console = config.console.unwrap();
    let file = config.file.unwrap();
    assert_eq!(console.filter, "debug");
    assert_eq!(console.buffered_lines_limit, 32);
    assert!(!console.lossy);
    assert!(console.ansi);
    assert_eq!(file.filter, "warn");
    assert_eq!(file.buffered_lines_limit, 64);
    assert!(!file.lossy);

    fs::write(&path, "").unwrap();
    let config: Config = tlab::config::load(&path, "TLAB_TRACING_DEFAULTS_TEST").unwrap();
    assert!(config.console.is_none() && config.file.is_none());
}

#[test]
fn rejects_unknown_fields_and_missing_file_location() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracing.toml");
    for (contents, expected) in [
        ("consol = {}", "unknown field"),
        ("[console]\nfitler = 'debug'", "unknown field"),
        (
            "[file]\ndirectory = './logs'\nfilename_prefix = 'app.log'\nfitler = 'debug'",
            "unknown field",
        ),
        ("[file]\nfilename_prefix = 'app.log'", "file.directory"),
        ("[file]\ndirectory = './logs'", "file.filename_prefix"),
        (
            "[file]\ndirectory = './logs'\nfilename = 'app.log'",
            "unknown field",
        ),
    ] {
        fs::write(&path, contents).unwrap();
        let error =
            tlab::config::load::<Config>(&path, "TLAB_TRACING_VALIDATION_TEST").unwrap_err();
        assert!(error.to_string().contains(expected), "{contents}: {error}");
    }
}

#[test]
fn initialization_contract() {
    // Isolate the global subscriber and RUST_LOG from the test runner.
    let Ok(scenario) = std::env::var("TLAB_TRACING_TEST_SCENARIO") else {
        for scenario in ["configured", "override", "invalid_env"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "initialization_contract", "--nocapture"])
                .env("TLAB_TRACING_TEST_SCENARIO", scenario)
                .env_remove("RUST_LOG");
            match scenario {
                "override" => {
                    command.env("RUST_LOG", "warn");
                }
                "invalid_env" => {
                    command.env("RUST_LOG", "[invalid");
                }
                _ => {}
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{scenario}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };

    let directory = tempfile::tempdir().unwrap();
    let log_directory = directory.path().join("logs");
    let mut config = Config {
        console: Some(ConsoleTraceConfig {
            filter: "error".into(),
            buffered_lines_limit: 128,
            lossy: false,
            ansi: false,
        }),
        file: Some(FileTraceConfig {
            directory: log_directory.clone(),
            filename_prefix: "app.log".into(),
            filter: "info".into(),
            buffered_lines_limit: 128,
            lossy: false,
        }),
    };
    drop(initialize(&Config::default()).unwrap());

    if scenario == "invalid_env" {
        assert!(initialize(&config).is_err());
        assert!(!log_directory.exists());
        return;
    }
    if scenario == "configured" {
        config.file.as_mut().unwrap().filter = "[invalid".into();
        assert!(initialize(&config).is_err());
        assert!(!log_directory.exists());
        config.file.as_mut().unwrap().filter = "info".into();

        fs::write(&log_directory, "not a directory").unwrap();
        assert!(initialize(&config).is_err());
        fs::remove_file(&log_directory).unwrap();
    } else {
        // A valid environment filter overrides even invalid configured filters.
        config.console.as_mut().unwrap().filter = "[invalid".into();
        config.file.as_mut().unwrap().filter = "[invalid".into();
    }

    let guard = initialize(&config).unwrap();
    assert!(initialize(&config).is_err());
    tracing::debug!("excluded-debug-event");
    tracing::info!("info-event");
    tracing::warn!("warn-event");
    drop(guard);

    let logs: String = fs::read_dir(&log_directory)
        .unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert!(logs.contains("warn-event"));
    assert_eq!(logs.contains("info-event"), scenario == "configured");
    assert!(!logs.contains("excluded-debug-event"));
    assert!(!logs.contains('\u{1b}'));
}
