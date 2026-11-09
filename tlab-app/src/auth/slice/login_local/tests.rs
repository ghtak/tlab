use crate::app::error::{AppError, AppErrorCode};
use std::sync::{Arc, Condvar, Mutex};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::IntoResponse,
};
use jsonwebtoken::EncodingKey;
use serde_json::{Value, json};
use tlab::password::{PasswordHasher, argon2id};
use tokio::{
    sync::{Semaphore, mpsc},
    time::{Duration, timeout},
};
use tower::ServiceExt;

use super::usecase::{self, verify_password};
use crate::{
    app::{self, Container},
    auth::{LocalLogin, passwd_policy::NonEmptyPasswordPolicy, token::AccessIssuer},
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
        Err(AppError {
            code: AppErrorCode::Busy,
            ..
        })
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
        Err(AppError {
            code: AppErrorCode::InternalError,
            ..
        })
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
        let failure @ AppError {
            code: AppErrorCode::InternalError,
            ..
        } = &error
        else {
            panic!("expected internal failure: {error:?}");
        };
        if panic {
            assert_eq!(failure.stage, "verify_task");
            assert!(matches!(failure.cause, Some(tlab::Error::Internal(_))));
        } else {
            assert_eq!(failure.stage, "password_verify");
            assert!(matches!(failure.cause, Some(tlab::Error::IllegalState(_))));
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

async fn isolated<F, Fut>(scenario: F)
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let url = std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL");
    let admin = app::db::Database::new(&tlab::sqlxdb::Config {
        url: url.clone(),
        max_connections: 1,
    })
    .await
    .unwrap();
    let schema = format!("tlab_login_test_{}", uuid::Uuid::new_v4().simple());
    let mut conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let result = tokio::spawn(scenario(format!(
        "{url}{separator}options=-csearch_path%3D{schema}"
    )))
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}

async fn fixture(url: String) -> (Arc<Container>, Value, i64) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
    let mut config: app::Config = tlab::config::load(path, "TLAB_LOGIN_TEST").unwrap();
    config.database.url = url;
    config.database.max_connections = 4;
    config.server.http.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https = config.server.http.clone();
    let hasher: Arc<dyn PasswordHasher> =
        Arc::new(argon2id::Hasher::new(&config.passwd_hasher).unwrap());
    let slots = Arc::new(Semaphore::new(2));
    let key_dir = tempfile::tempdir().unwrap();
    config.auth.jwt.private_key = key_dir.path().join("private.pem");
    config.auth.jwt.public_key = key_dir.path().join("public.pem");
    config.auth.jwt.generate_if_missing = true;
    let (access_issuer, access_verifier) = config.auth.jwt.prepare().unwrap();
    let local_login =
        usecase::LocalLogin::new(hasher.clone(), slots.clone(), access_issuer.clone())
            .await
            .unwrap();
    let database = Arc::new(app::db::Database::new(&config.database).await.unwrap());
    let role_permission_resolver =
        Arc::new(crate::rbac::DbRolePermissionResolver::new(database.clone()));
    let container = Arc::new(Container {
        http: tlab::http::Server::new(&config.server.http).await.unwrap(),
        https: tlab::http::Server::new(&config.server.https).await.unwrap(),
        database,
        role_permission_resolver,
        passwd_hasher: hasher,
        passwd_policy: Arc::new(NonEmptyPasswordPolicy),
        passwd_hash_slots: slots,
        local_login,
        access_issuer,
        access_verifier,
        config,
    });
    let mut conn = container.database.conn().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0001_create_tlab_account.sql"
    ))
    .execute(conn.context().backend())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0002_create_tlab_auth_refresh_token.sql"
    ))
    .execute(conn.context().backend())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0003_create_tlab_rbac.sql"
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
    (container, created, id)
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn login_refresh_logout_http_flow() {
    isolated(|url| async move {
        let (container, created, id) = fixture(url).await;
        let (status, body) = login(container.clone(), " user@EXAMPLE.com ", " secret ").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["account_id"], created["account_id"]);
        assert_eq!(body["status"], created["status"]);
        assert!(body["access_token"].is_string());
        assert!(body["refresh_token"].is_string());
        let access = body["access_token"].as_str().unwrap();
        let refresh = body["refresh_token"].as_str().unwrap();
        async fn protected(claims: crate::auth::token::AccessClaims) -> axum::Json<Value> {
            axum::Json(json!({"sid": claims.sid, "sub": claims.sub}))
        }
        let router = crate::auth::router()
            .route("/protected", axum::routing::get(protected))
            .with_state(container.clone());
        let protect = |token: &str| {
            let router = router.clone();
            let token = token.to_owned();
            async move {
                let response = router
                    .oneshot(
                        Request::get("/protected")
                            .header("authorization", format!("Bearer {token}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                serde_json::from_slice::<Value>(
                    &to_bytes(response.into_body(), 4096).await.unwrap(),
                )
                .unwrap()
            }
        };
        let first_claims = protect(access).await;
        assert_eq!(first_claims["sub"], id.to_string());
        let (status, renewed) = request(
            container.clone(),
            "/api/v1/auth/refresh",
            json!({"refresh_token":refresh}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renewed.as_object().unwrap().len(), 1);
        assert_eq!(
            protect(renewed["access_token"].as_str().unwrap()).await["sid"],
            first_claims["sid"]
        );
        let (status, _) = request(
            container.clone(),
            "/api/v1/auth/logout",
            json!({"refresh_token":refresh}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(protect(access).await, first_claims);
        let (status, _) = request(
            container.clone(),
            "/api/v1/auth/refresh",
            json!({"refresh_token":refresh}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn login_returns_account_without_mutating_it() {
    isolated(|url| async move {
        let (container, _created, id) = fixture(url).await;
        let snapshot = snapshot(&container).await;
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
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn login_issues_independent_sessions() {
    isolated(|url| async move {
        let (container, _created, id) = fixture(url).await;
        let dir = tempfile::tempdir().unwrap();
        let material = tlab::keyfiles::Ed25519KeyFile {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: true,
        }
        .load()
        .unwrap();
        let issuer =
            crate::auth::token::AccessIssuer::new(&material, "test-issuer", "test-audience")
                .unwrap();
        let verifier = crate::auth::token::AccessVerifier::new(
            material.public_key_pem(),
            "test-issuer",
            "test-audience",
        )
        .unwrap();
        let mut sessions = Vec::new();
        let local_login = LocalLogin::new(
            container.passwd_hasher.clone(),
            container.passwd_hash_slots.clone(),
            Arc::new(issuer),
        )
        .await
        .unwrap();
        let mut expected_roles = json!([]);
        for iteration in 0..2 {
            let result = local_login
                .execute_with_tokens(
                    &container.database,
                    usecase::Command {
                        email: "User@example.com".into(),
                        password: " secret ".into(),
                    },
                )
                .await
                .unwrap();
            let now = chrono::Utc::now().timestamp() as u64;
            let claims = verifier.verify(result.access.expose_secret(), now).unwrap();
            assert_eq!(claims.sub, id.to_string());
            assert_eq!(claims.user["nickname"], "member");
            assert_eq!(claims.user["role_ids"], expected_roles);
            assert!(claims.user.get("roles").is_none());
            assert_eq!(claims.exp - claims.iat, 600);
            let mut conn = container.database.conn().await.unwrap();
            let row = crate::auth::repository::refresh_token::find_by_hash(
                &mut conn.context(),
                &result.refresh.hash(),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(row.session_id, claims.sid);
            assert_eq!(row.token_hash, result.refresh.hash().as_bytes());
            assert_ne!(row.token_hash, result.refresh.expose_secret().as_bytes());
            assert_eq!(row.created_at.timestamp(), claims.iat as i64);
            assert_eq!(row.session_created_at, row.created_at);
            assert_eq!((row.expires_at - row.created_at).num_seconds(), 168 * 3600);
            assert_eq!(row.session_expires_at, row.expires_at);
            sessions.push((claims.sid, result.refresh.expose_secret().to_owned()));
            if iteration == 0 {
                let ids: Vec<i64> = sqlx::query_scalar("INSERT INTO tlab_rbac_role (name, description) VALUES ('a', ''), ('b', '') RETURNING id").fetch_all(conn.context().backend()).await.unwrap();
                for role in &ids {
                    sqlx::query("INSERT INTO tlab_rbac_account_role (account_id, role_id) VALUES ($1, $2)").bind(id).bind(role).execute(conn.context().backend()).await.unwrap();
                }
                expected_roles = json!(ids);
            }
        }
        assert_ne!(sessions[0].0, sessions[1].0);
        assert_ne!(sessions[0].1, sessions[1].1);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn login_signing_insert_and_commit_failures() {
    isolated(|url| async move {
        let (container, _created, _id) = fixture(url).await;
        let dir = tempfile::tempdir().unwrap();
        let material = tlab::keyfiles::Ed25519KeyFile {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: true,
        }
        .load()
        .unwrap();
        let valid_issuer =
            Arc::new(AccessIssuer::new(&material, "test-issuer", "test-audience").unwrap());
        let invalid_issuer = Arc::new(AccessIssuer{
            key: EncodingKey::from_ed_der(b"invalid key"),
            issuer: "test-issuer".into(),
            audience: "test-audience".into(),
        });
        let _verifier = crate::auth::token::AccessVerifier::new(
            material.public_key_pem(),
            "test-issuer",
            "test-audience",
        )
        .unwrap();
        let command = || usecase::Command {
            email: "User@example.com".into(),
            password: " secret ".into(),
        };

        let mut conn = container.database.conn().await.unwrap();
        sqlx::query("ALTER TABLE tlab_rbac_account_role RENAME TO unavailable_roles").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let local_login = LocalLogin::new(container.passwd_hasher.clone(), container.passwd_hash_slots.clone(), valid_issuer.clone()).await.unwrap();
        let failed = local_login.execute_with_tokens(&container.database, command()).await;
        assert!(matches!(failed, Err(e @ AppError { code: AppErrorCode::InternalError, .. }) if e.stage == "list_role_ids"));
        let mut conn = container.database.conn().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_auth_refresh_token").fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(count, 0);
        sqlx::query("ALTER TABLE unavailable_roles RENAME TO tlab_rbac_account_role").execute(conn.context().backend()).await.unwrap();
        drop(conn);

        let local_login = LocalLogin::new(container.passwd_hasher.clone(), container.passwd_hash_slots.clone(), invalid_issuer.clone()).await.unwrap();
        let failed = local_login
            .execute_with_tokens(
                &container.database,
                command(),
            )
            .await;
        assert!(matches!(failed, Err(e @ AppError { code: AppErrorCode::InternalError, .. }) if e.stage == "access_sign"));
        for (trigger, stage) in [
            (
                "CREATE TRIGGER fail_login BEFORE INSERT ON tlab_auth_refresh_token FOR EACH ROW EXECUTE FUNCTION fail_login()",
                "refresh_insert",
            ),
            (
                "CREATE CONSTRAINT TRIGGER fail_login AFTER INSERT ON tlab_auth_refresh_token DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_login()",
                "login_commit",
            ),
        ] {
            let mut conn = container.database.conn().await.unwrap();
            sqlx::raw_sql("CREATE OR REPLACE FUNCTION fail_login() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected failure'; END $$")
                .execute(conn.context().backend()).await.unwrap();
            sqlx::raw_sql(trigger)
                .execute(conn.context().backend())
                .await
                .unwrap();
            drop(conn);

            let local_login = LocalLogin::new(container.passwd_hasher.clone(), container.passwd_hash_slots.clone(), valid_issuer.clone()).await.unwrap();
            let failed = local_login
                .execute_with_tokens(&container.database, command())
                .await;
            assert!(matches!(failed, Err(e @ AppError { code: AppErrorCode::InternalError, .. }) if e.stage == stage));
            let mut conn = container.database.conn().await.unwrap();
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_auth_refresh_token")
                .fetch_one(conn.context().backend())
                .await
                .unwrap();
            assert_eq!(count, 0);
            sqlx::raw_sql("DROP TRIGGER fail_login ON tlab_auth_refresh_token")
                .execute(conn.context().backend())
                .await
                .unwrap();
        }
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn login_rechecks_account_after_withdrawal_lock() {
    isolated(|url| async move {
        let (container, _created, id) = fixture(url).await;
        let dir = tempfile::tempdir().unwrap();
        tlab::keyfiles::Ed25519KeyFile {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: true,
        }
        .load()
        .unwrap();
        // A withdrawal owns the account lock while login has already verified the password.
        let mut withdrawal = container.database.tx().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(withdrawal.context().backend())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=CURRENT_TIMESTAMP WHERE id=$1",
        )
        .bind(id)
        .execute(withdrawal.context().backend())
        .await
        .unwrap();
        let login_container = container.clone();
        let login_material = tlab::keyfiles::Ed25519KeyFile {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: false,
        }
        .load()
        .unwrap();
        let waiting = tokio::spawn(async move {
            let issuer =
                crate::auth::token::AccessIssuer::new(&login_material, "test-issuer", "test-audience")
                    .unwrap();

            let local_login = LocalLogin::new(login_container.passwd_hasher.clone(), login_container.passwd_hash_slots.clone() , Arc::new(issuer)).await.unwrap();
            local_login
                .execute_with_tokens(
                    &login_container.database,
                    usecase::Command {
                        email: "User@example.com".into(),
                        password: " secret ".into(),
                    },
                )
                .await
        });
        timeout(Duration::from_secs(10), async {
            loop {
                let mut conn = container.database.conn().await.unwrap();
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)) AND query LIKE '%FOR UPDATE%')")
                    .bind(pid).fetch_one(conn.context().backend()).await.unwrap();
                if blocked { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        withdrawal.commit().await.unwrap();
        assert!(matches!(
            waiting.await.unwrap(),
            Err(AppError { code: AppErrorCode::AccountUnavailable, .. })
        ));
        let mut conn = container.database.conn().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_auth_refresh_token")
            .fetch_one(conn.context().backend())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn wrong_password_and_missing_account_are_indistinguishable() {
    isolated(|url| async move {
        let (container, _created, _id) = fixture(url).await;
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
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn busy_and_invalid_http_requests() {
    isolated(|url| async move {
        let (container, _created, _id) = fixture(url).await;
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
                json!({"nickname":"m", "email":"busy@example.com", "password":"secret"})
                    .to_string()
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
                json!({"email":"User@example.com", "password":"secret", "status":"ACTIVE"})
                    .to_string(),
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
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn account_state_and_wrong_password_contract() {
    isolated(|url| async move {
        let (container, _created, id) = fixture(url).await;
        let wrong = login(container.clone(), "User@example.com", "wrong").await;
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
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn invalid_password_hash_and_nonlocal_identity() {
    isolated(|url| async move {
        let (container, _created, id) = fixture(url).await;
        let wrong = login(container.clone(), "User@example.com", "wrong").await;
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

    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn storage_failure_is_internal() {
    isolated(|url| async move {
        let (container, _created, _id) = fixture(url).await;
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
    })
    .await;
}

async fn snapshot(container: &Container) -> String {
    let mut conn = container.database.conn().await.unwrap();
    sqlx::query_scalar("SELECT row_to_json(a)::text || row_to_json(i)::text FROM tlab_account a JOIN tlab_account_identity i ON i.account_id = a.id")
            .fetch_one(conn.context().backend()).await.unwrap()
}
