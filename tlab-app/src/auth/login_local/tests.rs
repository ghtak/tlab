use std::{
    sync::{Arc, Condvar, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::IntoResponse,
};
use serde_json::{Value, json};
use tlab::password::{PasswordHasher, argon2id};
use tokio::{
    sync::{Semaphore, mpsc},
    time::{Duration, timeout},
};
use tower::ServiceExt;

use super::usecase::{self, Error, verify_password};
use crate::{
    app::{self, Container},
    auth::passwd_policy::NonEmptyPasswordPolicy,
};

struct Gate {
    released: Mutex<bool>,
    wake: Condvar,
    started: mpsc::UnboundedSender<()>,
}

impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

struct GatedVerifier(Arc<Gate>);
impl PasswordHasher for GatedVerifier {
    fn hash(&self, _: &str) -> tlab::Result<String> {
        unreachable!()
    }
    fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
        self.0.started.send(()).unwrap();
        let mut released = self.0.released.lock().unwrap();
        while !*released {
            released = self.0.wake.wait(released).unwrap();
        }
        Ok(false)
    }
}

struct ReleaseOnDrop(Arc<Gate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn cancellation_keeps_verification_capacity_until_blocking_work_finishes() {
    let (started, mut events) = mpsc::unbounded_channel();
    let gate = Arc::new(Gate {
        released: Mutex::new(false),
        wake: Condvar::new(),
        started,
    });
    let _cleanup = ReleaseOnDrop(gate.clone());
    let hasher: Arc<dyn PasswordHasher> = Arc::new(GatedVerifier(gate.clone()));
    let slots = Arc::new(Semaphore::new(1));
    let task = tokio::spawn(verify_password(
        hasher.clone(),
        slots.clone(),
        "secret".into(),
        "hash".into(),
    ));
    timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(slots.available_permits(), 0);
    assert!(matches!(
        verify_password(
            hasher.clone(),
            slots.clone(),
            "secret".into(),
            "hash".into()
        )
        .await,
        Err(Error::Busy)
    ));
    gate.release();
    let permit = timeout(Duration::from_secs(5), slots.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    slots.close();
    assert!(matches!(
        verify_password(hasher, slots, "secret".into(), "hash".into()).await,
        Err(Error::Internal(_))
    ));
}

struct BrokenVerifier(bool);
impl PasswordHasher for BrokenVerifier {
    fn hash(&self, _: &str) -> tlab::Result<String> {
        unreachable!()
    }
    fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
        assert!(!self.0, "synthetic verification panic");
        Err(tlab::Error::IllegalState(
            "synthetic verification failure".into(),
        ))
    }
}

#[tokio::test]
async fn verifier_errors_and_panics_are_internal_and_release_capacity() {
    for panic in [false, true] {
        let slots = Arc::new(Semaphore::new(1));
        let error = verify_password(
            Arc::new(BrokenVerifier(panic)),
            slots.clone(),
            "secret".into(),
            "hash".into(),
        )
        .await
        .unwrap_err();
        let Error::Internal(failure) = &error else {
            panic!("expected internal failure: {error:?}");
        };
        if panic {
            assert_eq!(failure.stage, "verify_task");
            assert!(matches!(failure.cause, tlab::Error::Internal(_)));
        } else {
            assert_eq!(failure.stage, "password_verify");
            assert!(matches!(failure.cause, tlab::Error::IllegalState(_)));
        }
        assert_eq!(slots.available_permits(), 1);
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic"));
    }
}

async fn request(container: Arc<Container>, path: &str, payload: String) -> (StatusCode, Value) {
    let response = crate::auth::router()
        .with_state(container)
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    if path == "/api/v1/auth/local" {
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(!response.headers().contains_key("set-cookie"));
    }
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!body.to_string().contains("$argon2"));
    (status, body)
}

async fn login(container: Arc<Container>, email: &str, password: &str) -> (StatusCode, Value) {
    request(
        container,
        "/api/v1/auth/local",
        json!({"email": email, "password": password}).to_string(),
    )
    .await
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn local_login_http_contracts() {
    let url = std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL");
    let admin = app::db::Database::new(&tlab::sqlxdb::Config {
        url: url.clone(),
        max_connections: 1,
    })
    .await
    .unwrap();
    let schema = format!(
        "tlab_login_test_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let result = tokio::spawn(check_contracts(format!(
        "{url}{separator}options=-csearch_path%3D{schema}"
    )))
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}

async fn check_contracts(url: String) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
    let mut config: app::Config = tlab::config::load(path, "TLAB_LOGIN_TEST").unwrap();
    config.database.url = url;
    config.database.max_connections = 1;
    config.server.http.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https = config.server.http.clone();
    let hasher: Arc<dyn PasswordHasher> =
        Arc::new(argon2id::Hasher::new(&config.passwd_hasher).unwrap());
    let slots = Arc::new(Semaphore::new(2));
    let local_login = usecase::LocalLogin::new(hasher.clone(), slots.clone())
        .await
        .unwrap();
    let container = Arc::new(Container {
        http: tlab::http::Server::new(&config.server.http).await.unwrap(),
        https: tlab::http::Server::new(&config.server.https).await.unwrap(),
        database: app::db::Database::new(&config.database).await.unwrap(),
        passwd_hasher: hasher,
        passwd_policy: Arc::new(NonEmptyPasswordPolicy),
        passwd_hash_slots: slots,
        local_login,
        config,
    });
    let mut conn = container.database.conn().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/0001_create_tlab_account.sql"
    ))
    .execute(conn.context().backend())
    .await
    .unwrap();
    drop(conn);
    let (status, created) = request(
        container.clone(),
        "/api/v1/accounts/local",
        json!({
            "nickname": "member", "email": "User@example.com", "password": " secret "
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["account_id"].as_i64().unwrap();
    let snapshot = snapshot(&container).await;
    let (status, body) = login(container.clone(), " user@EXAMPLE.com ", " secret ").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, created);
    let account = container
        .local_login
        .execute(
            &container.database,
            usecase::Command {
                email: "User@example.com".into(),
                password: " secret ".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(account.id, id);
    assert_eq!(account.nickname, "member");
    assert_eq!(
        account.status,
        crate::auth::entity::AccountStatus::Registered
    );
    assert!(account.withdrawn_at.is_none());
    assert_eq!(snapshot, self::snapshot(&container).await);
    let wrong = login(container.clone(), "User@example.com", "secret").await;
    let missing = login(
        container.clone(),
        "missing@example.com",
        "local-login-dummy-password",
    )
    .await;
    assert_eq!(wrong, missing);
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.1["code"], "INVALID_CREDENTIALS");

    // The same process-wide permits are consumed by both request handlers.
    let permits = container
        .passwd_hash_slots
        .clone()
        .acquire_many_owned(2)
        .await
        .unwrap();
    assert_eq!(
        login(container.clone(), "User@example.com", " secret ")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        login(container.clone(), "missing@example.com", "secret")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(
            container.clone(),
            "/api/v1/accounts/local",
            json!({"nickname":"m", "email":"busy@example.com", "password":"secret"}).to_string()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for (payload, code) in [
        (
            json!({"email":"bad", "password":"secret"}).to_string(),
            "INVALID_EMAIL",
        ),
        (
            json!({"email":"User@example.com", "password":""}).to_string(),
            "PASSWORD_REQUIRED",
        ),
        (
            json!({"email":"User@example.com", "password":"secret", "status":"ACTIVE"}).to_string(),
            "INVALID_REQUEST",
        ),
        ("{}".into(), "INVALID_REQUEST"),
        (
            "{\"password\":\"sensitive-input\"".into(),
            "INVALID_REQUEST",
        ),
    ] {
        let (status, body) = request(container.clone(), "/api/v1/auth/local", payload).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], code);
        assert!(!body.to_string().contains("sensitive-input"));
    }
    let (status, body) = request(
        container.clone(),
        "/api/v1/auth/local",
        json!({"email":"User@example.com", "password":"x".repeat(2 * 1024 * 1024)}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "INVALID_REQUEST");
    drop(permits);

    for (state, expected) in [
        ("ACTIVE", StatusCode::OK),
        ("WITHDRAWN", StatusCode::FORBIDDEN),
    ] {
        let mut conn = container.database.conn().await.unwrap();
        sqlx::query("UPDATE tlab_account SET status = $1, withdrawn_at = CASE WHEN $1 = 'WITHDRAWN' THEN CURRENT_TIMESTAMP ELSE NULL END WHERE id = $2")
            .bind(state).bind(id).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let before = self::snapshot(&container).await;
        let (status, body) = login(container.clone(), "User@example.com", " secret ").await;
        assert_eq!(status, expected);
        if status == StatusCode::OK {
            assert_eq!(body["status"], state);
        } else {
            assert_eq!(body["code"], "ACCOUNT_UNAVAILABLE");
        }
        assert_eq!(
            login(container.clone(), "User@example.com", "wrong").await,
            wrong
        );
        assert_eq!(before, self::snapshot(&container).await);
    }
    for hash in [
        None,
        Some("broken-phc"),
        Some("$pbkdf2-sha256$i=1,l=32$c2FsdA$aGFzaA"),
    ] {
        let mut conn = container.database.conn().await.unwrap();
        sqlx::query("UPDATE tlab_account_identity SET password_hash = $1 WHERE account_id = $2")
            .bind(hash)
            .bind(id)
            .execute(conn.context().backend())
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            login(container.clone(), "User@example.com", " secret ")
                .await
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    let mut conn = container.database.conn().await.unwrap();
    sqlx::query("UPDATE tlab_account_identity SET provider = 'GOOGLE', provider_subject = 'external-user' WHERE account_id = $1")
        .bind(id).execute(conn.context().backend()).await.unwrap();
    drop(conn);
    assert_eq!(
        login(container.clone(), "User@example.com", " secret ").await,
        wrong
    );

    // A missing table is a storage failure, never an authentication failure.
    let mut conn = container.database.conn().await.unwrap();
    sqlx::query("ALTER TABLE tlab_account_identity RENAME TO unavailable_identity")
        .execute(conn.context().backend())
        .await
        .unwrap();
    drop(conn);
    assert_eq!(
        login(container, "User@example.com", " secret ").await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

async fn snapshot(container: &Container) -> String {
    let mut conn = container.database.conn().await.unwrap();
    sqlx::query_scalar("SELECT row_to_json(a)::text || row_to_json(i)::text FROM tlab_account a JOIN tlab_account_identity i ON i.account_id = a.id")
        .fetch_one(conn.context().backend()).await.unwrap()
}
