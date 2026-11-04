use std::{fs, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use axum::{Router, extract::State, middleware, routing::get};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, ServerName};
use tlab::{
    Error,
    http::{Config, Server, StaticFilesConfig},
    keyfiles::TlsCertFile,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};
use tokio_rustls::{TlsConnector, client::TlsStream};

const WAIT: Duration = Duration::from_secs(5);

fn config() -> Config {
    Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        tls: None,
        shutdown_timeout_secs: 1,
    }
}

// The caller prepares TLS for every enabled instance before starting runs.
async fn prepare(configs: &[Config], router: Router) -> tlab::Result<Vec<TestServer>> {
    if configs.is_empty() {
        return Err(Error::InvalidInput("no server instances enabled".into()));
    }
    let mut servers = Vec::new();
    for config in configs {
        servers.push(TestServer::new(config, router.clone()).await?);
    }
    Ok(servers)
}

struct Identity {
    cert: CertificateDer<'static>,
    pem: String,
    key: String,
}

impl Identity {
    fn new() -> Self {
        let generated = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        Self {
            cert: generated.cert.der().clone(),
            pem: generated.cert.pem(),
            key: generated.signing_key.serialize_pem(),
        }
    }

    fn write(&self, files: &TlsCertFile) {
        fs::write(&files.certificate_chain, &self.pem).unwrap();
        fs::write(&files.private_key, &self.key).unwrap();
    }
}

fn files(root: &Path) -> TlsCertFile {
    TlsCertFile {
        certificate_chain: root.join("chain.pem"),
        private_key: root.join("key.pem"),
        generate_if_missing: false,
        generation: None,
    }
}

async fn connect_tls(addr: SocketAddr, trusted: &[&Identity]) -> TlsStream<TcpStream> {
    let mut roots = rustls::RootCertStore::empty();
    for identity in trusted {
        roots.add(identity.cert.clone()).unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    timeout(WAIT, async {
        let tcp = TcpStream::connect(addr).await.unwrap();
        TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap()
    })
    .await
    .unwrap()
}

async fn request<S>(mut stream: S, method: &str, path: &str) -> String
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    timeout(WAIT, async {
        stream
            .write_all(
                format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    })
    .await
    .unwrap()
}

// Reserve distinct test ports until startup. Production binds only in run().
struct TestServer {
    server: Arc<Server>,
    router: Router,
    reservation: std::net::TcpListener,
}

impl TestServer {
    async fn new(config: &Config, router: Router) -> tlab::Result<Self> {
        let reservation = std::net::TcpListener::bind(config.bind).map_err(anyhow::Error::from)?;
        let mut config = config.clone();
        config.bind = reservation.local_addr().unwrap();
        let server = Arc::new(Server::new(&config).await?);
        Ok(Self {
            server,
            router,
            reservation,
        })
    }

    fn local_addr(&self) -> SocketAddr {
        self.reservation.local_addr().unwrap()
    }
}

async fn start(server: TestServer) -> (oneshot::Sender<()>, JoinHandle<tlab::Result<()>>) {
    let addr = server.local_addr();
    let TestServer {
        server,
        router,
        reservation,
    } = server;
    let (stop, stopped) = oneshot::channel();
    drop(reservation);
    let task = tokio::spawn(async move {
        tokio::try_join!(server.run(router), async {
            let _ = stopped.await;
            server.graceful_shutdown();
            Ok::<(), Error>(())
        })?;
        Ok(())
    });
    // Wait for binding rather than relying on scheduler timing.
    timeout(WAIT, async {
        loop {
            if let Ok(connection) = TcpStream::connect(addr).await {
                drop(connection);
                break;
            }
            assert!(!task.is_finished(), "server exited before binding");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    (stop, task)
}

async fn finish(stop: oneshot::Sender<()>, task: JoinHandle<tlab::Result<()>>) {
    stop.send(()).unwrap();
    timeout(WAIT, task).await.unwrap().unwrap().unwrap();
}

#[test]
fn settings_defaults_and_unknown_fields() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AppServers {
        http: Option<Config>,
        https: Option<Config>,
        static_files: Option<StaticFilesConfig>,
    }
    let parsed: AppServers = serde_json::from_value(serde_json::json!({
        "http": { "bind": "127.0.0.1:0" },
        "static_files": { "directory": "public" }
    }))
    .unwrap();
    let http = parsed.http.unwrap();
    assert_eq!(http.shutdown_timeout_secs, 30);
    assert!(http.tls.is_none());
    assert_eq!(parsed.static_files.unwrap().prefix, "/static");
    assert!(parsed.https.is_none());
    for value in [
        serde_json::json!({}),
        serde_json::json!({ "bind": "localhost:80" }),
        serde_json::json!({ "bind": "127.0.0.1:0", "port": 5 }),
        serde_json::json!({ "bind": "127.0.0.1:0", "tls": { "certificate_chain": "cert.pem" } }),
        serde_json::json!({ "bind": "127.0.0.1:0", "http": {} }),
    ] {
        assert!(serde_json::from_value::<Config>(value).is_err());
    }
    assert!(
        serde_json::from_value::<StaticFilesConfig>(serde_json::json!({
            "directory": "public", "preix": "/assets"
        }))
        .is_err()
    );
}

#[tokio::test]
async fn reject_invalid_server_settings_and_missing_tls() {
    for settings in [
        Config {
            shutdown_timeout_secs: 0,
            ..config()
        },
        Config {
            shutdown_timeout_secs: u64::MAX,
            ..config()
        },
    ] {
        assert!(matches!(
            Server::new(&settings).await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(matches!(
        prepare(&[], Router::new()).await,
        Err(Error::InvalidInput(_))
    ));
    let directory = tempfile::tempdir().unwrap();
    let tls = files(directory.path());
    let settings = Config {
        tls: Some(tls.clone()),
        ..config()
    };
    assert!(matches!(
        Server::new(&settings).await,
        Err(Error::Internal(_))
    ));
    assert!(!tls.private_key.exists());
    assert!(!tls.certificate_chain.exists());
    let bad = Config {
        tls: Some(TlsCertFile {
            private_key: "".into(),
            ..tls
        }),
        ..config()
    };
    assert!(matches!(
        Server::new(&bad).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn construction_does_not_bind_and_run_reports_port_conflicts() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = occupied.local_addr().unwrap();
    let settings = Config {
        bind: addr,
        ..config()
    };
    // TLS/config preparation succeeds even while the address is occupied.
    let server = Server::new(&settings).await.unwrap();
    assert!(matches!(
        server.reload_tls_config().await,
        Err(Error::IllegalState(_))
    ));
    let error = server.run(Router::new()).await.unwrap_err();
    assert!(error.to_string().contains("failed to bind HTTP listener"));
    drop(occupied);
    // The same server can run after the conflicting listener is removed.
    server.graceful_shutdown();
    server.run(Router::new()).await.unwrap();
    drop(std::net::TcpListener::bind(addr).unwrap());
}

#[tokio::test]
async fn app_instances_share_state_layers_and_static_files() {
    let directory = tempfile::tempdir().unwrap();
    let tls = files(directory.path());
    let identity = Identity::new();
    identity.write(&tls);
    let public = directory.path().join("public");
    fs::create_dir(&public).unwrap();
    fs::write(public.join("index.html"), "index-body").unwrap();
    fs::write(public.join("hello.txt"), "file-body").unwrap();
    for (http, https) in [(true, false), (false, true), (true, true)] {
        let mut configs = Vec::new();
        if http {
            configs.push(config());
        }
        if https {
            configs.push(Config {
                tls: Some(tls.clone()),
                ..config()
            });
        }
        let static_files = StaticFilesConfig {
            prefix: "/static/".into(),
            directory: public.clone(),
        };
        let router = Router::new()
            .route(
                "/api",
                get(|State(value): State<String>| async move { value }),
            )
            .merge(static_files.router().unwrap())
            .with_state("state-body".to_owned())
            .layer(middleware::map_response(
                |mut response: axum::response::Response| async move {
                    response
                        .headers_mut()
                        .insert("x-common", "yes".parse().unwrap());
                    response
                },
            ));
        let servers = prepare(&configs, router).await.unwrap();
        let addresses: Vec<_> = servers
            .iter()
            .zip(&configs)
            .map(|(server, config)| (server.local_addr(), config.tls.is_some()))
            .collect();
        let mut controls = Vec::new();
        for server in servers {
            controls.push(start(server).await);
        }
        for (method, path, status, body) in [
            ("GET", "/api", "200", "state-body"),
            ("GET", "/static/hello.txt", "200", "file-body"),
            ("HEAD", "/static/hello.txt", "200", ""),
            ("GET", "/static/", "200", "index-body"),
            ("GET", "/static/missing", "404", ""),
            ("POST", "/static/hello.txt", "405", ""),
            ("GET", "/static/../key.pem", "404", ""),
            ("GET", "/static/%2e%2e/key.pem", "404", ""),
            ("GET", "/static/..%5ckey.pem", "404", ""),
        ] {
            for &(addr, tls) in &addresses {
                let response = if tls {
                    request(connect_tls(addr, &[&identity]).await, method, path).await
                } else {
                    request(TcpStream::connect(addr).await.unwrap(), method, path).await
                };
                assert!(
                    response.starts_with(&format!("HTTP/1.1 {status}")),
                    "{method} {path}: {response}"
                );
                assert!(response.contains("x-common: yes\r\n"));
                assert_eq!(response.split_once("\r\n\r\n").unwrap().1, body);
            }
        }
        for (stop, task) in controls {
            finish(stop, task).await;
        }
    }
}

#[test]
fn static_settings_fail_before_router_assembly() {
    let directory = tempfile::tempdir().unwrap();
    for prefix in [
        "",
        "/",
        "//",
        "relative",
        "/{id}",
        "/x/{*tail}",
        "/x?y",
        "/x#y",
        "/a/../b",
        "/a/./b",
        "/a//b",
        "/a\\b",
        "/:id",
        "/a\n",
        "/%2f",
    ] {
        assert!(
            matches!(
                StaticFilesConfig {
                    prefix: prefix.into(),
                    directory: directory.path().into()
                }
                .router::<()>(),
                Err(Error::InvalidInput(_))
            ),
            "{prefix:?}"
        );
    }
    let file = directory.path().join("file");
    fs::write(&file, "file").unwrap();
    for root in [file, directory.path().join("missing"), "".into()] {
        assert!(matches!(
            StaticFilesConfig {
                prefix: "/static".into(),
                directory: root
            }
            .router::<()>(),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[tokio::test]
async fn reload_changes_new_handshakes_and_preserves_existing_connections() {
    let directory = tempfile::tempdir().unwrap();
    let mut tls = files(directory.path());
    tls.generate_if_missing = true;
    let a = Identity::new();
    let b = Identity::new();
    a.write(&tls);
    let router = Router::new().route("/", get(|| async { "alive" }));
    let mut servers = prepare(
        &[
            config(),
            Config {
                tls: Some(tls.clone()),
                ..config()
            },
        ],
        router,
    )
    .await
    .unwrap();
    let https_server = servers.pop().unwrap();
    let http_server = servers.pop().unwrap();
    let http = http_server.local_addr();
    let https = https_server.local_addr();
    let reloader = https_server.server.clone();
    let (http_stop, http_task) = start(http_server).await;
    let (stop, task) = start(https_server).await;
    let existing = connect_tls(https, &[&a]).await;
    assert_eq!(existing.get_ref().1.peer_certificates().unwrap()[0], a.cert);
    b.write(&tls);
    let (one, two) = tokio::join!(reloader.reload_tls_config(), reloader.reload_tls_config());
    one.unwrap();
    two.unwrap();
    let updated = connect_tls(https, &[&b]).await;
    assert_eq!(updated.get_ref().1.peer_certificates().unwrap()[0], b.cert);
    assert!(request(existing, "GET", "/").await.ends_with("alive"));
    drop(updated);

    // Partial replacement and malformed secret input must not replace B.
    fs::write(&tls.private_key, &a.key).unwrap();
    assert!(reloader.reload_tls_config().await.is_err());
    fs::write(
        &tls.private_key,
        "-----BEGIN PRIVATE KEY-----\nsecret-invalid-key\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let error = reloader.reload_tls_config().await.unwrap_err();
    assert!(!format!("{error:?}").contains("secret-invalid-key"));
    fs::remove_file(&tls.private_key).unwrap();
    fs::remove_file(&tls.certificate_chain).unwrap();
    assert!(reloader.reload_tls_config().await.is_err());
    assert!(!tls.private_key.exists());
    assert!(!tls.certificate_chain.exists());
    let retained = connect_tls(https, &[&b]).await;
    assert_eq!(retained.get_ref().1.peer_certificates().unwrap()[0], b.cert);
    assert!(request(retained, "GET", "/").await.ends_with("alive"));
    assert!(
        request(TcpStream::connect(http).await.unwrap(), "GET", "/")
            .await
            .ends_with("alive")
    );
    finish(stop, task).await;
    finish(http_stop, http_task).await;
}

#[tokio::test]
async fn graceful_shutdown_waits_for_a_running_response() {
    let (entered, entering) = oneshot::channel();
    let entered = Arc::new(tokio::sync::Mutex::new(Some(entered)));
    let (release, released) = oneshot::channel();
    let released = Arc::new(tokio::sync::Mutex::new(Some(released)));
    let router = Router::new().route(
        "/",
        get(move || {
            let entered = entered.clone();
            let released = released.clone();
            async move {
                entered.lock().await.take().unwrap().send(()).unwrap();
                let _ = released.lock().await.take().unwrap().await;
                "finished"
            }
        }),
    );
    let server = TestServer::new(&config(), router).await.unwrap();
    let addr = server.local_addr();
    let (stop, task) = start(server).await;
    let response =
        tokio::spawn(
            async move { request(TcpStream::connect(addr).await.unwrap(), "GET", "/").await },
        );
    timeout(WAIT, entering).await.unwrap().unwrap();
    stop.send(()).unwrap();
    release.send(()).unwrap();
    assert!(
        timeout(WAIT, response)
            .await
            .unwrap()
            .unwrap()
            .ends_with("finished")
    );
    timeout(WAIT, task).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn deadline_stops_servers_with_incomplete_tls_and_closes_running_http() {
    let directory = tempfile::tempdir().unwrap();
    let tls = files(directory.path());
    Identity::new().write(&tls);
    let (entered, entering) = oneshot::channel();
    let entered = Arc::new(tokio::sync::Mutex::new(Some(entered)));
    let router = Router::new().route(
        "/",
        get(move || {
            let entered = entered.clone();
            async move {
                entered.lock().await.take().unwrap().send(()).unwrap();
                std::future::pending::<String>().await
            }
        }),
    );
    let mut servers = prepare(
        &[
            config(),
            Config {
                tls: Some(tls),
                ..config()
            },
        ],
        router,
    )
    .await
    .unwrap();
    let https_server = servers.pop().unwrap();
    let http_server = servers.pop().unwrap();
    let http = http_server.local_addr();
    let https = https_server.local_addr();
    let (http_stop, http_task) = start(http_server).await;
    let (stop, task) = start(https_server).await;
    let mut stalled = TcpStream::connect(https).await.unwrap();
    stalled.write_all(&[22, 3, 3, 0, 100, 1]).await.unwrap();
    let mut running = TcpStream::connect(http).await.unwrap();
    running
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    timeout(WAIT, entering).await.unwrap().unwrap();
    stop.send(()).unwrap();
    http_stop.send(()).unwrap();
    timeout(WAIT, task).await.unwrap().unwrap().unwrap();
    timeout(WAIT, http_task).await.unwrap().unwrap().unwrap();
    assert!(timeout(WAIT, running.read_u8()).await.unwrap().is_err());
    // An incomplete TLS handshake uses the backend timeout. The library only
    // guarantees bounded server shutdown, not immediate termination of this IO.
    drop(stalled);
    drop(std::net::TcpListener::bind(http).unwrap());
    drop(std::net::TcpListener::bind(https).unwrap());
}

#[tokio::test]
async fn cancelling_run_closes_listeners_and_connections() {
    let directory = tempfile::tempdir().unwrap();
    let tls = files(directory.path());
    let identity = Identity::new();
    identity.write(&tls);
    let settings = Config {
        tls: Some(tls),
        ..config()
    };
    let server = TestServer::new(&settings, Router::new()).await.unwrap();
    let https = server.local_addr();
    let (_stop, task) = start(server).await;
    // A completed TLS handshake ensures the backend has accepted connections.
    let mut connection = connect_tls(https, &[&identity]).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(timeout(WAIT, connection.read_u8()).await.unwrap().is_err());
    drop(std::net::TcpListener::bind(https).unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn static_symlinks_follow_filesystem_semantics() {
    let public = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("outside"), "linked-body").unwrap();
    std::os::unix::fs::symlink(external.path().join("outside"), public.path().join("link"))
        .unwrap();
    let router = StaticFilesConfig {
        prefix: "/static".into(),
        directory: public.path().into(),
    }
    .router()
    .unwrap();
    let server = TestServer::new(&config(), router).await.unwrap();
    let addr = server.local_addr();
    let (stop, task) = start(server).await;
    assert!(
        request(
            TcpStream::connect(addr).await.unwrap(),
            "GET",
            "/static/link"
        )
        .await
        .ends_with("linked-body")
    );
    finish(stop, task).await;
}

#[tokio::test]
async fn app_try_join_failure_drops_the_other_instance() {
    let (entered, entering) = oneshot::channel();
    let entered = Arc::new(tokio::sync::Mutex::new(Some(entered)));
    let router = Router::new().route(
        "/",
        get(move || {
            let entered = entered.clone();
            async move {
                entered.lock().await.take().unwrap().send(()).unwrap();
                std::future::pending::<String>().await
            }
        }),
    );
    let server = TestServer::new(&config(), router).await.unwrap();
    let addr = server.local_addr();
    let TestServer {
        server,
        router,
        reservation,
    } = server;
    drop(reservation);
    let task = tokio::spawn(async move {
        // Simulate another instance failing after this server has begun serving.
        tokio::try_join!(server.run(router), async {
            entering.await.unwrap();
            Err::<(), _>(Error::Internal(anyhow::anyhow!("other instance failed")))
        },)
    });
    let mut connection = timeout(WAIT, async {
        loop {
            if let Ok(connection) = TcpStream::connect(addr).await {
                break connection;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    connection
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let error = timeout(WAIT, task).await.unwrap().unwrap().unwrap_err();
    assert!(error.to_string().contains("other instance failed"));
    assert!(timeout(WAIT, connection.read_u8()).await.unwrap().is_err());
    drop(std::net::TcpListener::bind(addr).unwrap());
}
