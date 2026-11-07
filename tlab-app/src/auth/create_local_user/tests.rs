use std::{
    future::Future,
    io::Write,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tlab::password::argon2id;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

use crate::app::{self, Container};
use crate::auth::{
    create_local_user::usecase::{self as create_local_user, Command, Error, InternalFailure},
    entity::{AccountStatus, NewAccount, NewIdentity, Provider},
    passwd_policy::{NonEmptyPasswordPolicy, PasswordPolicy, PasswordPolicyViolation},
    repository::{account, identity},
};

fn command(email: &str) -> Command {
    Command {
        nickname: " member ".into(),
        email: email.into(),
        password: " password with spaces ".into(),
    }
}

struct RejectPassword;

impl PasswordPolicy for RejectPassword {
    fn validate(&self, _: &str) -> Result<(), PasswordPolicyViolation> {
        Err(PasswordPolicyViolation {
            code: "PASSWORD_REJECTED",
            message: "password rejected by policy",
        })
    }
}

#[tokio::test]
async fn passwd_policy_preserves_whitespace_and_can_be_replaced() {
    let policy = NonEmptyPasswordPolicy;
    assert!(policy.validate("").is_err());
    assert!(policy.validate("  ").is_ok());
    assert!(RejectPassword.validate("some password").is_err());
}

async fn request(container: Arc<Container>, payload: &str) -> (StatusCode, Value) {
    let response = crate::auth::router()
        .with_state(container)
        .oneshot(
            Request::post("/api/accounts/local")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    response_json(response).await
}

async fn response_json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn with_logs<T>(future: impl Future<Output = T>) -> (T, String) {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let output = future.with_subscriber(subscriber).await;
    let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    (output, logs)
}

#[tokio::test]
async fn internal_error_logs_cause_without_exposing_it_in_the_response() {
    let (response, logs) = with_logs(async {
        Error::Internal(InternalFailure {
            stage: "password_hash",
            cause: Box::new(std::io::Error::other("synthetic hash failure")),
        })
        .into_response()
    })
    .await;
    let (status, body) = response_json(response).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "INTERNAL_ERROR");
    assert!(logs.contains("operation=\"create_local_user\""));
    assert!(logs.contains("stage=\"password_hash\""));
    assert!(logs.contains("cause="));
    assert!(logs.contains("synthetic hash failure"));
    assert!(!body.to_string().contains("synthetic hash failure"));

    let (response, logs) =
        with_logs(async { app::response::internal_error("user creation failed") }).await;
    let (status, body) = response_json(response).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({"code": "INTERNAL_ERROR", "message": "user creation failed"})
    );
    assert!(
        logs.is_empty(),
        "response construction must not emit logs: {logs}"
    );
}

#[tokio::test]
async fn route_error_logs_are_explicit_and_linked_to_the_request_span() {
    for (status, code) in [
        (400, "INVALID_EMAIL"),
        (409, "EMAIL_ALREADY_EXISTS"),
        (503, "BUSY"),
        (500, "INTERNAL_ERROR"),
    ] {
        let (response, logs) = with_logs(async {
            let router = crate::http::trace_requests(axum::Router::new().route(
                "/errors",
                axum::routing::post(move || async move {
                    match status {
                        400 => Error::InvalidInput {
                            field: "email",
                            code: "INVALID_EMAIL",
                            message: "email must be a single email address",
                        },
                        409 => Error::EmailAlreadyExists,
                        503 => Error::Busy,
                        _ => Error::Internal(InternalFailure {
                            stage: "password_hash",
                            cause: Box::new(std::io::Error::other("synthetic hash failure")),
                        }),
                    }
                    .into_response()
                }),
            ));
            router
                .oneshot(
                    Request::post("/errors")
                        .header(
                            "traceparent",
                            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        })
        .await;
        let (actual_status, body) = response_json(response).await;
        assert_eq!(actual_status.as_u16(), status);
        assert_eq!(body["code"], code);
        assert_eq!(
            logs.lines().filter(|line| line.contains("ERROR")).count(),
            usize::from(status == 500),
            "{logs}"
        );
        assert_eq!(
            logs.lines().filter(|line| line.contains("WARN")).count(),
            usize::from(status == 503),
            "{logs}"
        );
        assert!(logs.contains("started processing request"), "{logs}");
        assert!(logs.contains(&format!("status={status}")), "{logs}");
        for line in logs.lines() {
            assert!(
                line.contains("trace_id=0af7651916cd43dd8448eb211c80319c"),
                "{line}"
            );
        }
        if status == 500 {
            assert!(logs.contains("stage=\"password_hash\""), "{logs}");
            assert!(logs.contains("operation=\"create_local_user\""), "{logs}");
            assert!(logs.contains("cause="), "{logs}");
            assert!(logs.contains("synthetic hash failure"), "{logs}");
        }
        assert!(!body.to_string().contains("synthetic hash failure"));
    }
}

async fn container(url: String) -> Container {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
    let mut config: app::Config = tlab::config::load(path, "TLAB_AUTH_TEST").unwrap();
    config.database.url = url;
    // No listeners or TLS material are needed for in-process HTTP tests.
    config.server.http.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https.tls = None;
    let passwd_hasher: Arc<dyn tlab::password::PasswordHasher> =
        Arc::new(argon2id::Hasher::new(&config.passwd_hasher).unwrap());
    let passwd_hash_slots = Arc::new(Semaphore::new(2));
    let local_login =
        crate::auth::LocalLogin::new(passwd_hasher.clone(), passwd_hash_slots.clone())
            .await
            .unwrap();
    Container {
        http: tlab::http::Server::new(&config.server.http).await.unwrap(),
        https: tlab::http::Server::new(&config.server.https).await.unwrap(),
        database: app::db::Database::new(&config.database).await.unwrap(),
        passwd_hasher,
        passwd_policy: Arc::new(NonEmptyPasswordPolicy),
        passwd_hash_slots,
        local_login,
        config,
    }
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn local_user_http_and_service_contracts() {
    let url = std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL");
    let admin = app::db::Database::new(&tlab::sqlxdb::Config {
        url: url.clone(),
        max_connections: 1,
    })
    .await
    .unwrap();
    let schema = format!(
        "tlab_auth_test_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut admin_conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(admin_conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let test_url = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let result = tokio::spawn(check_local_user_contracts(test_url)).await;

    // Always remove the owned schema, including after a panic in the contract checks.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(admin_conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}

async fn check_local_user_contracts(test_url: String) {
    let container = Arc::new(container(test_url).await);
    let mut conn = container.database.conn().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/0001_create_tlab_account.sql"
    ))
    .execute(conn.context().backend())
    .await
    .unwrap();
    drop(conn);
    check_enum_mappings(&container.database).await;
    let payload = json!({"nickname":" member ", "email":" User@example.com ", "password":" password with spaces "}).to_string();
    let (status, body) = request(container.clone(), &payload).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["status"], "REGISTERED");
    assert_eq!(body.as_object().unwrap().len(), 2);
    let id = body["account_id"].as_i64().unwrap();
    let mut conn = container.database.conn().await.unwrap();
    let row: (String, String, String, i32, Option<String>, bool, String) = sqlx::query_as(
        "SELECT a.nickname, a.status, i.provider, i.priority, i.provider_subject, \
             i.email_verified_at IS NULL, i.password_hash FROM tlab_account a \
             JOIN tlab_account_identity i ON i.account_id = a.id WHERE a.id = $1",
    )
    .bind(id)
    .fetch_one(conn.context().backend())
    .await
    .unwrap();
    assert_eq!(
        (&*row.0, &*row.1, &*row.2, row.3, row.4, row.5),
        ("member", "REGISTERED", "LOCAL", 0, None, true)
    );
    assert!(row.6.starts_with("$argon2id$"));
    assert!(
        container
            .passwd_hasher
            .verify(" password with spaces ", &row.6)
            .unwrap()
    );
    assert!(
        !container
            .passwd_hasher
            .verify("password with spaces", &row.6)
            .unwrap()
    );
    let stored_email: String =
        sqlx::query_scalar("SELECT email FROM tlab_account_identity WHERE account_id = $1")
            .bind(id)
            .fetch_one(conn.context().backend())
            .await
            .unwrap();
    assert_eq!(stored_email, "User@example.com");
    drop(conn);

    let (status, body) = request(
        container.clone(),
        &payload.replace("User@example.com", "user@EXAMPLE.com"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "EMAIL_ALREADY_EXISTS");
    for payload in [
        "{\"password\":\"sensitive-input\"",
        "{}",
        "{\"nickname\":\"m\",\"email\":\"bad\",\"password\":\"secret\"}",
        "{\"nickname\":\"m\",\"email\":\"empty@example.com\",\"password\":\"\"}",
        "{\"nickname\":\"m\",\"email\":\"extra@example.com\",\"password\":\"secret\",\"status\":\"ACTIVE\"}",
    ] {
        let (status, body) = request(container.clone(), payload).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!body.to_string().contains("sensitive-input"));
        assert!(!body.to_string().contains("secret"));
    }
    for email in [
        "member@example.com,other@example.com",
        "Member <member@example.com>",
    ] {
        let result = create_local_user::execute(
            &container.database,
            container.passwd_policy.as_ref(),
            container.passwd_hasher.clone(),
            container.passwd_hash_slots.clone(),
            command(email),
        )
        .await;
        assert!(matches!(
            result,
            Err(Error::InvalidInput { field: "email", .. })
        ));
    }
    let slots = container
        .passwd_hash_slots
        .clone()
        .acquire_many_owned(2)
        .await
        .unwrap();
    let (status, body) = request(container.clone(), &payload).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "BUSY");
    // Even with no hashing capacity, unsupported text must fail before hashing.
    let nul_payload =
        json!({"nickname":"member\0name", "email":"nul@example.com", "password":"secret"})
            .to_string();
    let (status, body) = request(container.clone(), &nul_payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["field"], "nickname");
    assert_eq!(body["code"], "INVALID_NICKNAME");
    drop(slots);
    let result = create_local_user::execute(
        &container.database,
        &RejectPassword,
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        command("policy@example.com"),
    )
    .await;
    assert!(matches!(
        result,
        Err(Error::InvalidInput {
            field: "password",
            code: "PASSWORD_REJECTED",
            ..
        })
    ));

    // Two simultaneous creations of one email must leave only one account.
    let a = create_local_user::execute(
        &container.database,
        container.passwd_policy.as_ref(),
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        command("race@example.com"),
    );
    let b = create_local_user::execute(
        &container.database,
        container.passwd_policy.as_ref(),
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        command("RACE@example.com"),
    );
    let (a, b) = tokio::join!(a, b);
    assert!(matches!(
        (&a, &b),
        (Ok(_), Err(Error::EmailAlreadyExists)) | (Err(Error::EmailAlreadyExists), Ok(_))
    ));

    // Repository CRUD accepts provider values; the service owns the LOCAL intent.
    let mut tx = container.database.tx().await.unwrap();
    let mut ctx = tx.context();
    let sns_account = account::insert(
        &mut ctx,
        &NewAccount {
            nickname: "sns".into(),
            status: AccountStatus::Active,
            withdrawn_at: None,
        },
    )
    .await
    .unwrap();
    identity::insert(
        &mut ctx,
        &NewIdentity {
            account_id: sns_account.id,
            priority: 0,
            provider: Provider::Google,
            provider_subject: Some("subject".into()),
            email: "sns@example.com".into(),
            email_verified_at: None,
            password_hash: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    create_local_user::execute(
        &container.database,
        container.passwd_policy.as_ref(),
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        command("sns@example.com"),
    )
    .await
    .unwrap();

    // An unrelated unique violation is INTERNAL, with account insertion rolled back.
    let mut conn = container.database.conn().await.unwrap();
    sqlx::query("INSERT INTO tlab_account_identity (account_id, provider, provider_subject, email) VALUES ($1, 'NEW_PROVIDER', 'failure-subject', 'failure@example.com')").bind(sns_account.id).execute(conn.context().backend()).await.unwrap();
    sqlx::query("CREATE UNIQUE INDEX test_identity_email_key ON tlab_account_identity (email) WHERE email = 'failure@example.com'").execute(conn.context().backend()).await.unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_account")
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
    drop(conn);
    let ((status, body), logs) = with_logs(request(
        container.clone(),
        &payload.replace("User@example.com", "failure@example.com"),
    ))
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "INTERNAL_ERROR");
    assert!(logs.contains("stage=\"insert_identity\""));
    assert!(logs.contains("operation=\"create_local_user\""));
    assert!(logs.contains("cause="));
    assert!(logs.contains("23505"));
    assert!(!body.to_string().contains("failure@example.com"));
    assert!(!logs.contains("password with spaces"));
    assert!(!logs.contains("$argon2id$"));
    let mut conn = container.database.conn().await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_account")
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
    assert_eq!(before, after);
    let orphans: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_account a WHERE NOT EXISTS (SELECT FROM tlab_account_identity i WHERE i.account_id = a.id)").fetch_one(conn.context().backend()).await.unwrap();
    assert_eq!(orphans, 0);
}

async fn check_enum_mappings(database: &app::db::Database) {
    let mut conn = database.conn().await.unwrap();
    let mut ctx = conn.context();
    for (value, code) in [
        (AccountStatus::Registered, "REGISTERED"),
        (AccountStatus::Active, "ACTIVE"),
        (AccountStatus::Withdrawn, "WITHDRAWN"),
    ] {
        let encoded: String = sqlx::query_scalar("SELECT $1::text")
            .bind(value)
            .fetch_one(ctx.backend())
            .await
            .unwrap();
        assert_eq!(encoded, code);
        let decoded: AccountStatus = sqlx::query_scalar("SELECT $1::text")
            .bind(code)
            .fetch_one(ctx.backend())
            .await
            .unwrap();
        assert_eq!(decoded, value);
    }
    for (value, code) in [
        (Provider::Local, "LOCAL"),
        (Provider::Google, "GOOGLE"),
        (Provider::Kakao, "KAKAO"),
    ] {
        let encoded: String = sqlx::query_scalar("SELECT $1::text")
            .bind(value)
            .fetch_one(ctx.backend())
            .await
            .unwrap();
        assert_eq!(encoded, code);
        let decoded: Provider = sqlx::query_scalar("SELECT $1::text")
            .bind(code)
            .fetch_one(ctx.backend())
            .await
            .unwrap();
        assert_eq!(decoded, value);
    }
    let unsupported = sqlx::query_scalar::<_, Provider>("SELECT 'NEW_PROVIDER'::text")
        .fetch_one(ctx.backend())
        .await;
    assert!(matches!(unsupported, Err(sqlx::Error::ColumnDecode { .. })));
    let unsupported = sqlx::query_scalar::<_, AccountStatus>("SELECT 'UNKNOWN'::text")
        .fetch_one(ctx.backend())
        .await;
    assert!(matches!(unsupported, Err(sqlx::Error::ColumnDecode { .. })));
}
