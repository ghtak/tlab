//! Exercise the production Container and Router, not a test-only assembly.
use crate::{
    app::{Config, Container},
    auth::{LocalAccountInput, test_support::isolated_with_url},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

async fn setup(url: String) -> Arc<Container> {
    let dir = tempfile::tempdir().unwrap();
    let mut config: Config = tlab::config::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"),
        "TLAB_RBAC_HTTP_TEST",
    )
    .unwrap();
    config.database.url = url;
    config.server.http.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https.bind = "127.0.0.1:0".parse().unwrap();
    let tls = config.server.https.tls.as_mut().unwrap();
    tls.certificate_chain = dir.path().join("cert.pem");
    tls.private_key = dir.path().join("tls.pem");
    config.auth.jwt.private_key = dir.path().join("jwt-private.pem");
    config.auth.jwt.public_key = dir.path().join("jwt-public.pem");
    let container = Arc::new(Container::new(config).await.unwrap());
    super::slice::bootstrap_admin::usecase::execute(
        &container.database,
        container.passwd_policy.as_ref(),
        container.passwd_hasher.clone(),
        container.passwd_hash_slots.clone(),
        LocalAccountInput {
            nickname: "admin".into(),
            email: "admin@example.test".into(),
            password: "admin-secret".into(),
        },
    )
    .await
    .unwrap();
    container
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    payload: Value,
) -> Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(req.body(Body::from(payload.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}
fn signup(email: &str) -> Value {
    json!({"nickname":"member", "email":email, "password":"member-secret"})
}
async fn login(app: &Router, email: &str, password: &str) -> Value {
    body(
        send(
            app,
            "POST",
            "/api/v1/auth/local",
            None,
            json!({"email":email,"password":password}),
        )
        .await,
        StatusCode::OK,
    )
    .await
}
async fn refresh(app: &Router, tokens: &Value) -> String {
    body(
        send(
            app,
            "POST",
            "/api/v1/auth/refresh",
            None,
            json!({"refresh_token":tokens["refresh_token"]}),
        )
        .await,
        StatusCode::OK,
    )
    .await["access_token"]
        .as_str()
        .unwrap()
        .into()
}
async fn create_role(app: &Router, token: &str, name: &str, permissions: Value) -> i64 {
    let role = body(
        send(
            app,
            "POST",
            "/api/v1/rbac/roles",
            Some(token),
            json!({"name":name,"description":""}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = role["id"].as_i64().unwrap();
    body(
        send(
            app,
            "PUT",
            &format!("/api/v1/rbac/roles/{id}/permissions"),
            Some(token),
            json!({"version":1,"permissions":permissions}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    id
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn production_http_issuance_authorization_changes_and_refresh() {
    isolated_with_url(|_db, url| async move {
        let container = setup(url).await;
        let app = crate::http::router(container.clone()).unwrap();
        body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                None,
                signup("anonymous@example.test"),
            )
            .await,
            StatusCode::UNAUTHORIZED,
        )
        .await;
        body(
            send(&app, "GET", "/api/v1/rbac/roles", None, Value::Null).await,
            StatusCode::UNAUTHORIZED,
        )
        .await;
        let admin = login(&app, "admin@example.test", "admin-secret").await;
        let token = admin["access_token"].as_str().unwrap();
        assert!(!token.is_empty());
        let member = body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                Some(token),
                signup("member@example.test"),
            )
            .await,
            StatusCode::CREATED,
        )
        .await;
        let id = member["account_id"].as_i64().unwrap();
        let member_tokens = login(&app, "member@example.test", "member-secret").await;
        let old = member_tokens["access_token"].as_str().unwrap();
        let denied = body(
            send(&app, "POST", "/api/v1/accounts/local", Some(old), json!({})).await,
            StatusCode::FORBIDDEN,
        )
        .await;
        assert_eq!(denied["code"], "RBAC_FORBIDDEN");
        let role = create_role(&app, token, "creator", json!(["account.create"])).await;
        let assignment = format!("/api/v1/accounts/{id}/roles/{role}");
        body(
            send(&app, "PUT", &assignment, Some(token), Value::Null).await,
            StatusCode::NO_CONTENT,
        )
        .await;
        // Adding a role does not alter an already-issued token.
        body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                Some(old),
                signup("before-refresh@example.test"),
            )
            .await,
            StatusCode::FORBIDDEN,
        )
        .await;
        let creator = refresh(&app, &member_tokens).await;
        body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                Some(&creator),
                signup("created@example.test"),
            )
            .await,
            StatusCode::CREATED,
        )
        .await;
        body(
            send(&app, "DELETE", &assignment, Some(token), Value::Null).await,
            StatusCode::NO_CONTENT,
        )
        .await;
        assert_eq!(
            body(
                send(
                    &app,
                    "GET",
                    &format!("/api/v1/accounts/{id}/roles"),
                    Some(&creator),
                    Value::Null
                )
                .await,
                StatusCode::OK
            )
            .await,
            json!([])
        );
        assert_eq!(
            body(
                send(
                    &app,
                    "GET",
                    &format!("/api/v1/accounts/{id}/permissions"),
                    Some(&creator),
                    Value::Null
                )
                .await,
                StatusCode::OK
            )
            .await,
            json!([])
        );
        // Revoked assignment remains in Access; current role permissions still apply.
        body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                Some(&creator),
                signup("residual@example.test"),
            )
            .await,
            StatusCode::CREATED,
        )
        .await;
        body(
            send(
                &app,
                "PUT",
                &format!("/api/v1/rbac/roles/{role}/permissions"),
                Some(token),
                json!({"version":2,"permissions":[]}),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        body(
            send(
                &app,
                "POST",
                "/api/v1/accounts/local",
                Some(&creator),
                signup("removed-permission@example.test"),
            )
            .await,
            StatusCode::FORBIDDEN,
        )
        .await;
        let renewed = refresh(&app, &member_tokens).await;
        let claims = container
            .access_verifier
            .verify(&renewed, chrono::Utc::now().timestamp() as u64)
            .unwrap();
        assert_eq!(claims.user["role_ids"], json!([]));

        let manager = create_role(
            &app,
            token,
            "limited-manager",
            json!(["rbac.role.manage", "rbac.assignment.manage", "rbac.read"]),
        )
        .await;
        body(
            send(
                &app,
                "PUT",
                &format!("/api/v1/accounts/{id}/roles/{manager}"),
                Some(token),
                Value::Null,
            )
            .await,
            StatusCode::NO_CONTENT,
        )
        .await;
        let limited = refresh(&app, &member_tokens).await;
        let admin_claims = container
            .access_verifier
            .verify(token, chrono::Utc::now().timestamp() as u64)
            .unwrap();
        let admin_role = admin_claims.user["role_ids"][0].as_i64().unwrap();
        body(
            send(
                &app,
                "PUT",
                &format!("/api/v1/accounts/{id}/roles/{admin_role}"),
                Some(&limited),
                Value::Null,
            )
            .await,
            StatusCode::FORBIDDEN,
        )
        .await;
        body(
            send(
                &app,
                "PUT",
                &format!("/api/v1/rbac/roles/{role}/permissions"),
                Some(&limited),
                json!({"version":3,"permissions":["account.create"]}),
            )
            .await,
            StatusCode::FORBIDDEN,
        )
        .await;
        assert_eq!(
            body(
                send(
                    &app,
                    "GET",
                    &format!("/api/v1/rbac/roles/{role}"),
                    Some(token),
                    Value::Null
                )
                .await,
                StatusCode::OK
            )
            .await["role"]["version"],
            3
        );
        body(
            send(
                &app,
                "GET",
                "/api/v1/rbac/permissions",
                Some(token),
                Value::Null,
            )
            .await,
            StatusCode::OK,
        )
        .await;
        body(
            send(&app, "GET", "/api/v1/rbac/roles", Some(token), Value::Null).await,
            StatusCode::OK,
        )
        .await;
        body(
            send(
                &app,
                "GET",
                "/api/v1/accounts/9223372036854775807/permissions",
                Some(token),
                Value::Null,
            )
            .await,
            StatusCode::NOT_FOUND,
        )
        .await;
        body(
            send(
                &app,
                "PATCH",
                &format!("/api/v1/rbac/roles/{role}"),
                Some(token),
                json!({"version":3,"name":"renamed","description":"changed"}),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        body(
            send(
                &app,
                "DELETE",
                &format!("/api/v1/rbac/roles/{role}"),
                Some(token),
                json!({"version":3}),
            )
            .await,
            StatusCode::CONFLICT,
        )
        .await;
        body(
            send(
                &app,
                "DELETE",
                &format!("/api/v1/rbac/roles/{role}"),
                Some(token),
                json!({"version":4}),
            )
            .await,
            StatusCode::NO_CONTENT,
        )
        .await;

        let mut conn = container.database.conn().await.unwrap();
        sqlx::query("ALTER TABLE tlab_rbac_role_permission RENAME TO unavailable_permissions")
            .execute(conn.context().backend())
            .await
            .unwrap();
        drop(conn);
        for (method, path, payload) in [
            ("GET", "/api/v1/rbac/permissions", Value::Null),
            (
                "POST",
                "/api/v1/accounts/local",
                signup("db-failure@example.test"),
            ),
        ] {
            let error = body(
                send(&app, method, path, Some(token), payload).await,
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .await;
            assert_eq!(error["code"], "INTERNAL_ERROR");
            assert_eq!(error.as_object().unwrap().len(), 2);
            assert!(!error.to_string().contains("unavailable_permissions"));
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn production_web_rbac_cookie_csrf_and_automatic_refresh() {
    isolated_with_url(|_db, url| async move {
        let container = setup(url).await;
        let app = crate::http::router(container.clone()).unwrap();
        let origin = container.config.auth.web.origin.clone();
        let web_request =
            |method: &str, path: &str, cookie: &str, origin: Option<&str>, payload: Value| {
                let mut req = Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json");
                if !cookie.is_empty() {
                    req = req.header("cookie", cookie);
                }
                if let Some(origin) = origin {
                    req = req.header("origin", origin);
                }
                app.clone()
                    .oneshot(req.body(Body::from(payload.to_string())).unwrap())
            };
        let response = web_request(
            "POST",
            "/web/auth/local",
            "",
            Some(&origin),
            json!({"email":"admin@example.test","password":"admin-secret"}),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookies: Vec<_> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(cookies.len(), 2);
        for cookie in &cookies {
            for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
                assert!(cookie.contains(attr));
            }
        }
        let access = cookies
            .iter()
            .find(|c| c.starts_with("__Host-tlab_access="))
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let refresh = cookies
            .iter()
            .find(|c| c.starts_with("__Host-tlab_refresh="))
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let login_body = body(response, StatusCode::OK).await;
        assert!(login_body.get("access_token").is_none());
        body(
            web_request("GET", "/web/rbac/roles", access, None, Value::Null)
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        for origin in [None, Some("https://foreign.test")] {
            let error = body(
                web_request(
                    "POST",
                    "/web/accounts/local",
                    access,
                    origin,
                    signup("csrf@example.test"),
                )
                .await
                .unwrap(),
                StatusCode::FORBIDDEN,
            )
            .await;
            assert_eq!(error["code"], "CSRF_REJECTED");
        }
        body(
            web_request(
                "POST",
                "/web/accounts/local",
                access,
                Some(&origin),
                signup("web-created@example.test"),
            )
            .await
            .unwrap(),
            StatusCode::CREATED,
        )
        .await;
        let refreshed = web_request(
            "POST",
            "/web/accounts/local",
            refresh,
            Some(&origin),
            signup("auto-refresh@example.test"),
        )
        .await
        .unwrap();
        assert_eq!(refreshed.headers()["cache-control"], "no-store, private");
        assert_eq!(refreshed.headers().get_all("set-cookie").iter().count(), 1);
        body(refreshed, StatusCode::CREATED).await;
        body(
            web_request("GET", "/api/v1/rbac/roles", access, None, Value::Null)
                .await
                .unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
        let tokens = login(&app, "admin@example.test", "admin-secret").await;
        body(
            send(
                &app,
                "GET",
                "/web/rbac/roles",
                tokens["access_token"].as_str(),
                Value::Null,
            )
            .await,
            StatusCode::UNAUTHORIZED,
        )
        .await;
        let mut conn = container.database.conn().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_account")
            .fetch_one(conn.context().backend())
            .await
            .unwrap();
        sqlx::query("ALTER TABLE tlab_rbac_account_role RENAME TO unavailable_assignments")
            .execute(conn.context().backend())
            .await
            .unwrap();
        drop(conn);
        let response = web_request(
            "POST",
            "/web/accounts/local",
            refresh,
            Some(&origin),
            signup("must-not-run@example.test"),
        )
        .await
        .unwrap();
        assert!(!response.headers().contains_key("set-cookie"));
        body(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        let mut conn = container.database.conn().await.unwrap();
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_account")
            .fetch_one(conn.context().backend())
            .await
            .unwrap();
        assert_eq!(before, after);
    })
    .await;
}

async fn blocked_pid(db: &crate::app::db::Database, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut conn = db.conn().await.unwrap();
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(conn.context().backend())
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("request must reach the expected database lock")
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn production_role_management_rechecks_status_and_permissions_after_role_wait() {
    isolated_with_url(|_db,url| async move {
        let container = setup(url).await;
        let app = crate::http::router(container.clone()).unwrap();
        let admin = login(&app,"admin@example.test","admin-secret").await;
        let admin_token = admin["access_token"].as_str().unwrap();
        let member = body(send(&app,"POST","/api/v1/accounts/local",Some(admin_token),signup("manager@example.test")).await,StatusCode::CREATED).await;
        let id = member["account_id"].as_i64().unwrap();
        let role = create_role(&app,admin_token,"manager",json!(["rbac.role.manage"])).await;
        body(send(&app,"PUT",&format!("/api/v1/accounts/{id}/roles/{role}"),Some(admin_token),Value::Null).await,StatusCode::NO_CONTENT).await;
        let manager = login(&app,"manager@example.test","member-secret").await;
        let token = manager["access_token"].as_str().unwrap();
        // Allowed-state control and withdrawn rejection retain all permissions.
        body(send(&app,"POST","/api/v1/rbac/roles",Some(token),json!({"name":"allowed-control","description":""})).await,StatusCode::CREATED).await;
        let mut conn = container.database.conn().await.unwrap();
        sqlx::query("UPDATE tlab_account SET status='WITHDRAWN',withdrawn_at=now() WHERE id=$1").bind(id).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        body(send(&app,"POST","/api/v1/rbac/roles",Some(token),json!({"name":"withdrawn-must-not-exist","description":""})).await,StatusCode::FORBIDDEN).await;
        let mut conn = container.database.conn().await.unwrap();
        let permission_count: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_rbac_role_permission WHERE role_id=$1 AND permission_code='rbac.role.manage'").bind(role).fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(permission_count,1);
        sqlx::query("UPDATE tlab_account SET status='ACTIVE',withdrawn_at=NULL WHERE id=$1").bind(id).execute(conn.context().backend()).await.unwrap();
        drop(conn);

        // Pause a real HTTP role editor after it owns role locks, using a
        // test-only trigger. Different accounts prevent account-lock serialization.
        let mut gate = container.database.tx().await.unwrap();
        let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(gate.context().backend()).await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1::bigint)").bind(i64::from(gate_pid)).execute(gate.context().backend()).await.unwrap();
        let mut conn = container.database.conn().await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION pause_role_edit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({gate_pid}::bigint); RETURN OLD; END $$; CREATE TRIGGER pause_role_edit BEFORE DELETE ON tlab_rbac_role_permission FOR EACH ROW EXECUTE FUNCTION pause_role_edit()"))).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let editor_app = app.clone();
        let editor_token = admin_token.to_owned();
        let editor = tokio::spawn(async move { send(&editor_app,"PUT",&format!("/api/v1/rbac/roles/{role}/permissions"),Some(&editor_token),json!({"version":2,"permissions":[]})).await });
        let editor_pid = blocked_pid(&container.database,gate_pid).await;
        let waiting_app = app.clone();
        let waiting_token = token.to_owned();
        let waiting = tokio::spawn(async move { send(&waiting_app,"POST","/api/v1/rbac/roles",Some(&waiting_token),json!({"name":"after-wait-must-not-exist","description":""})).await });
        blocked_pid(&container.database,editor_pid).await;
        gate.commit().await.unwrap();
        body(editor.await.unwrap(),StatusCode::OK).await;
        body(waiting.await.unwrap(),StatusCode::FORBIDDEN).await;
        let mut conn = container.database.conn().await.unwrap();
        let forbidden_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tlab_rbac_role WHERE name IN ('withdrawn-must-not-exist','after-wait-must-not-exist')").fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(forbidden_rows,0);
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn production_assignment_rechecks_delegation_after_http_role_editor() {
    isolated_with_url(|_db,url| async move {
        let container = setup(url).await;
        let app = crate::http::router(container.clone()).unwrap();
        let admin = login(&app,"admin@example.test","admin-secret").await;
        let token = admin["access_token"].as_str().unwrap();
        let operator = body(send(&app,"POST","/api/v1/accounts/local",Some(token),signup("operator@example.test")).await,StatusCode::CREATED).await;
        let operator_id = operator["account_id"].as_i64().unwrap();
        let target = body(send(&app,"POST","/api/v1/accounts/local",Some(token),signup("target@example.test")).await,StatusCode::CREATED).await;
        let target_id = target["account_id"].as_i64().unwrap();
        let manager = create_role(&app,token,"assignment-manager",json!(["rbac.assignment.manage","rbac.read"])).await;
        let reader = create_role(&app,token,"reader",json!(["rbac.read"])).await;
        body(send(&app,"PUT",&format!("/api/v1/accounts/{operator_id}/roles/{manager}"),Some(token),Value::Null).await,StatusCode::NO_CONTENT).await;
        let operator = login(&app,"operator@example.test","member-secret").await;
        let mut gate = container.database.tx().await.unwrap();
        let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(gate.context().backend()).await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1::bigint)").bind(i64::from(gate_pid)).execute(gate.context().backend()).await.unwrap();
        let mut conn = container.database.conn().await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION pause_role_edit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({gate_pid}::bigint); RETURN NEW; END $$; CREATE TRIGGER pause_role_edit BEFORE INSERT ON tlab_rbac_role_permission FOR EACH ROW EXECUTE FUNCTION pause_role_edit()"))).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let editor_app = app.clone();
        let editor_token = token.to_owned();
        let editor = tokio::spawn(async move { send(&editor_app,"PUT",&format!("/api/v1/rbac/roles/{reader}/permissions"),Some(&editor_token),json!({"version":2,"permissions":["rbac.read","account.create"]})).await });
        let editor_pid = blocked_pid(&container.database,gate_pid).await;
        let assign_app = app.clone();
        let assign_token = operator["access_token"].as_str().unwrap().to_owned();
        let assign = tokio::spawn(async move { send(&assign_app,"PUT",&format!("/api/v1/accounts/{target_id}/roles/{reader}"),Some(&assign_token),Value::Null).await });
        blocked_pid(&container.database,editor_pid).await;
        gate.commit().await.unwrap();
        body(editor.await.unwrap(),StatusCode::OK).await;
        body(assign.await.unwrap(),StatusCode::FORBIDDEN).await;
        let mut conn = container.database.conn().await.unwrap();
        assert_eq!(super::list_role_ids(&mut conn.context(),target_id).await.unwrap(),super::RoleIds::default());
        assert!(super::repository::role_permission::list_for_role(&mut conn.context(),super::RoleId(reader)).await.unwrap().contains(&super::PermissionCode("account.create".into())));
    }).await;
}
