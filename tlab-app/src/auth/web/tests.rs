use super::*;
use crate::{
    app,
    auth::{
        repository::refresh_token,
        test_support::{isolated, session_setup, token_keys},
        token::AccessToken,
    },
};
use axum::{
    body::{Body, to_bytes},
    http::Request as HttpRequest,
};
use serde_json::{Value, json};
use tlab::password::PasswordHasher;
use tokio::sync::Semaphore;
use tower::ServiceExt;

// Only common resource setup lives here; each test owns its session and actions.
async fn container(database: app::db::Database) -> Arc<Container> {
    let mut config: app::Config = tlab::config::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/app.toml"),
        "TLAB_WEB_TEST",
    )
    .unwrap();
    config.server.http.bind = "127.0.0.1:0".parse().unwrap();
    config.server.https = config.server.http.clone();
    let (access_issuer, verifier) = token_keys();
    let access_issuer = Arc::new(access_issuer);
    let hasher: Arc<dyn PasswordHasher> =
        Arc::new(tlab::password::argon2id::Hasher::new(&config.passwd_hasher).unwrap());
    let slots = Arc::new(Semaphore::new(2));
    let local_login =
        super::super::LocalLogin::new(hasher.clone(), slots.clone(), access_issuer.clone())
            .await
            .unwrap();
    Arc::new(Container {
        http: tlab::http::Server::new(&config.server.http).await.unwrap(),
        https: tlab::http::Server::new(&config.server.https).await.unwrap(),
        config,
        database,
        passwd_hasher: hasher,
        passwd_policy: Arc::new(super::super::passwd_policy::NonEmptyPasswordPolicy),
        passwd_hash_slots: slots,
        local_login,
        access_issuer,
        access_verifier: Arc::new(verifier),
    })
}
fn app(container: Arc<Container>) -> Router {
    Router::new()
        .merge(router(container.clone()))
        .merge(crate::auth::router())
        .route("/api/protected", get(me))
        .route("/health", get(|| async { "ok" }))
        .route("/login", get(|| async { "login page" }))
        .route("/assets/test", get(|| async { "static" }))
        .with_state(container)
}
fn request(method: &str, path: &str, cookie: &str) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}
fn set_cookies(response: &Response) -> Vec<Cookie<'static>> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| Cookie::parse(value.to_str().unwrap().to_owned()).unwrap())
        .collect()
}
fn assert_private(response: &Response) {
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-store, private"
    );
}
fn assert_cleared(response: &Response) {
    let values = set_cookies(response);
    assert_eq!(values.len(), 2);
    for name in [ACCESS, REFRESH] {
        let cookie = values.iter().find(|cookie| cookie.name() == name).unwrap();
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.domain(), None);
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.max_age(), Some(Duration::ZERO));
        assert!(cookie.expires_datetime().unwrap() < OffsetDateTime::now_utc());
    }
}
fn access(container: &Container, sub: &str, sid: uuid::Uuid, now: u64) -> AccessToken {
    container
        .access_issuer
        .issue(sub, sid, Default::default(), now)
        .unwrap()
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn valid_cookie_access_and_public_routes_never_acquire_database() {
    isolated(|database| async move {
        let container = container(database).await;
        let token = access(&container, "123", uuid::Uuid::new_v4(), now_seconds());
        // Exhaust every slot: any acquire would hang until the deadline fails.
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(container.database.conn().await.unwrap());
        }
        let app = app(container);
        let mut req = request(
            "GET",
            "/web/auth/me",
            &format!("{ACCESS}={}; {REFRESH}=unused", token.expose_secret()),
        );
        req.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer invalid"),
        );
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(2), app.clone().oneshot(req))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_private(&response);
        assert!(set_cookies(&response).is_empty());
        assert_eq!(body(response).await["sub"], "123");
        for path in ["/health", "/login", "/assets/test"] {
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                app.clone()
                    .oneshot(request("GET", path, &format!("{REFRESH}=unused"))),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(set_cookies(&response).is_empty());
        }
        drop(held);
    })
    .await;
}

async fn assert_refresh_response(
    container: Arc<Container>,
    cookie: String,
    id: i64,
    sid: uuid::Uuid,
) {
    let response = app(container.clone())
        .oneshot(request("GET", "/web/auth/me", &cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_private(&response);
    let cookies = set_cookies(&response);
    assert_eq!(cookies.len(), 1, "refresh must never be rewritten");
    assert_eq!(cookies[0].name(), ACCESS);
    assert_eq!(cookies[0].path(), Some("/"));
    assert_eq!(cookies[0].domain(), None);
    assert_eq!(cookies[0].secure(), Some(true));
    assert_eq!(cookies[0].http_only(), Some(true));
    assert_eq!(cookies[0].same_site(), Some(SameSite::Lax));
    let claims = container
        .access_verifier
        .verify(cookies[0].value(), now_seconds())
        .unwrap();
    assert_eq!(claims.sub, id.to_string());
    assert_eq!(claims.sid, sid);
    let result = body(response).await;
    assert_eq!(result["sub"], id.to_string());
    assert_eq!(result["sid"], sid.to_string());
    assert_eq!(result["user"]["nickname"], "original");
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn missing_access_refreshes_same_request_and_preserves_refresh_row() {
    isolated(|database| async move {
        let (id, secret, sid) = session_setup(&database).await;
        let container = container(database).await;
        let before = {
            let mut conn = container.database.conn().await.unwrap();
            refresh_token::find_by_hash(&mut conn.context(), &secret.hash())
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            before.expires_at - before.session_created_at,
            Duration::days(7)
        );
        assert_refresh_response(
            container.clone(),
            format!("{REFRESH}={}", secret.expose_secret()),
            id,
            sid,
        )
        .await;
        let mut conn = container.database.conn().await.unwrap();
        let after = refresh_token::find_by_hash(&mut conn.context(), &secret.hash())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.id, after.id);
        assert!(before.token_hash == after.token_hash);
        assert_eq!(before.created_at, after.created_at);
        assert_eq!(before.expires_at, after.expires_at);
        assert_eq!(before.session_created_at, after.session_created_at);
        assert_eq!(before.session_expires_at, after.session_expires_at);
        assert!(after.revoked_at.is_none());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn genuinely_expired_access_refreshes_using_refresh_identity() {
    isolated(|database| async move {
        let (id, secret, sid) = session_setup(&database).await;
        let container = container(database).await;
        // Refresh authenticates independently; the expired Access is not identity evidence.
        let expired = access(
            &container,
            "old-identity",
            uuid::Uuid::new_v4(),
            now_seconds() - 601,
        );
        assert_refresh_response(
            container,
            format!(
                "{ACCESS}={}; {REFRESH}={}",
                expired.expose_secret(),
                secret.expose_secret()
            ),
            id,
            sid,
        )
        .await;
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn malformed_access_is_rejected_without_refresh_database_query() {
    isolated(|database| async move {
        let (_, secret, _) = session_setup(&database).await;
        let container = container(database).await;
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(container.database.conn().await.unwrap());
        }
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            app(container).oneshot(request(
                "GET",
                "/web/auth/me",
                &format!("{ACCESS}=invalid; {REFRESH}={}", secret.expose_secret()),
            )),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_private(&response);
        assert_cleared(&response);
        drop(held);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn unknown_refresh_is_unauthorized_and_clears_both_cookies() {
    isolated(|database| async move {
        let response = app(container(database).await)
            .oneshot(request(
                "GET",
                "/web/auth/me",
                &format!("{REFRESH}=unknown"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_private(&response);
        assert_cleared(&response);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn refresh_database_failure_is_internal_and_keeps_cookies_for_retry() {
    isolated(|database| async move {
        let (_, secret, _) = session_setup(&database).await;
        let mut conn = database.conn().await.unwrap();
        // Destroy only this scenario's temporary table, never a shared schema.
        sqlx::raw_sql("DROP TABLE tlab_auth_refresh_token")
            .execute(conn.context().backend())
            .await
            .unwrap();
        drop(conn);
        let response = app(container(database).await)
            .oneshot(request(
                "GET",
                "/web/auth/me",
                &format!("{REFRESH}={}", secret.expose_secret()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_private(&response);
        assert!(set_cookies(&response).is_empty());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn logout_revokes_refresh_and_removes_matching_cookie_scope() {
    isolated(|database| async move {
        let (_, secret, _) = session_setup(&database).await;
        let container = container(database).await;
        let mut req = request(
            "POST",
            "/web/auth/logout",
            &format!("{REFRESH}={}", secret.expose_secret()),
        );
        req.headers_mut().insert(
            header::ORIGIN,
            container.config.auth.web.origin.parse().unwrap(),
        );
        let response = app(container.clone()).oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_private(&response);
        assert_cleared(&response);
        let mut conn = container.database.conn().await.unwrap();
        let row = refresh_token::find_by_hash(&mut conn.context(), &secret.hash())
            .await
            .unwrap()
            .unwrap();
        assert!(row.revoked_at.is_some());
        drop(conn);
        let response = app(container)
            .oneshot(request(
                "GET",
                "/web/auth/me",
                &format!("{REFRESH}={}", secret.expose_secret()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_cleared(&response);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn web_login_issues_secure_cookies_after_common_issuance() {
    isolated(|database| async move {
        let container = container(database).await;
        let app = app(container.clone());
        let credentials = json!({"email":"member@example.test", "password":"secret"});
        let req = HttpRequest::post("/api/v1/accounts/local")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"email":"member@example.test", "password":"secret", "nickname":"member"})
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::CREATED
        );
        let req = HttpRequest::post("/web/auth/local")
            .header(header::ORIGIN, container.config.auth.web.origin.as_str())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(credentials.to_string()))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_private(&response);
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 2);
        for cookie in &cookies {
            assert_eq!(cookie.path(), Some("/"));
            assert_eq!(cookie.domain(), None);
            assert_eq!(cookie.http_only(), Some(true));
            assert_eq!(cookie.secure(), Some(true));
            assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        }
        let access = cookies
            .iter()
            .find(|cookie| cookie.name() == ACCESS)
            .unwrap();
        let refresh = cookies
            .iter()
            .find(|cookie| cookie.name() == REFRESH)
            .unwrap();
        let claims = container
            .access_verifier
            .verify(access.value(), now_seconds())
            .unwrap();
        let mut conn = container.database.conn().await.unwrap();
        let row = refresh_token::find_by_hash(
            &mut conn.context(),
            &super::super::token::RefreshHash::of(refresh.value()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(refresh.expires_datetime().unwrap(), row.expires_at);
        assert_eq!(row.session_expires_at, row.expires_at);
        assert_eq!(row.expires_at - row.session_created_at, Duration::days(7));
        assert_eq!(claims.sid, row.session_id);
        assert!(refresh.max_age().unwrap().whole_seconds() <= 604800);
        assert!(access.max_age().unwrap().whole_seconds() <= 600);
        let body = body(response).await;
        assert!(body.get("access_token").is_none());
        assert!(body.get("refresh_token").is_none());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn csrf_rejects_login_logout_and_cross_origin_refresh_before_database() {
    isolated(|database| async move {
        let container = container(database).await;
        let app = app(container.clone());
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(container.database.conn().await.unwrap());
        }
        for (name, method, path, origin, site) in [
            (
                "login missing origin",
                "POST",
                "/web/auth/local",
                None,
                None,
            ),
            (
                "logout null origin",
                "POST",
                "/web/auth/logout",
                Some("null"),
                None,
            ),
            (
                "logout foreign origin",
                "POST",
                "/web/auth/logout",
                Some("https://evil.test"),
                None,
            ),
            (
                "refresh foreign navigation",
                "GET",
                "/web/auth/me",
                None,
                Some("cross-site"),
            ),
            (
                "refresh same-site sibling",
                "GET",
                "/web/auth/me",
                None,
                Some("same-site"),
            ),
        ] {
            let mut req = request(method, path, &format!("{REFRESH}=unused"));
            if let Some(origin) = origin {
                req.headers_mut()
                    .insert(header::ORIGIN, origin.parse().unwrap());
            }
            if let Some(site) = site {
                req.headers_mut()
                    .insert("sec-fetch-site", site.parse().unwrap());
            }
            let response =
                tokio::time::timeout(std::time::Duration::from_secs(2), app.clone().oneshot(req))
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{name}");
            assert_private(&response);
            assert!(set_cookies(&response).is_empty());
        }
        drop(held);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn api_and_web_authentication_sources_are_separate() {
    isolated(|database| async move {
        let container = container(database).await;
        let bearer = access(&container, "api", uuid::Uuid::new_v4(), now_seconds());
        let cookie = access(&container, "web", uuid::Uuid::new_v4(), now_seconds());
        let app = app(container);
        for (path, include_bearer, include_cookie, expected) in [
            ("/api/protected", true, true, Some("api")),
            ("/web/auth/me", true, true, Some("web")),
            ("/api/protected", false, true, None),
            ("/web/auth/me", true, false, None),
        ] {
            let mut req = request("GET", path, if include_cookie { "" } else { "other=value" });
            if include_cookie {
                req.headers_mut().insert(
                    header::COOKIE,
                    format!("{ACCESS}={}", cookie.expose_secret())
                        .parse()
                        .unwrap(),
                );
            }
            if include_bearer {
                req.headers_mut().insert(
                    header::AUTHORIZATION,
                    format!("Bearer {}", bearer.expose_secret())
                        .parse()
                        .unwrap(),
                );
            }
            let response = app.clone().oneshot(req).await.unwrap();
            if let Some(sub) = expected {
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(body(response).await["sub"], sub);
            } else {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }
        }
    })
    .await;
}

#[test]
fn secure_origin_configuration_requires_explicit_http_opt_in() {
    assert!(Config::default().validate().is_ok());
    for origin in [
        "http://localhost:8080",
        "https://host/path",
        "https://host/",
        "https://host?query",
        "https://user@host",
        "null",
    ] {
        assert!(
            Config {
                origin: origin.into(),
                allow_insecure_http: false
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        Config {
            origin: "http://localhost:8080".into(),
            allow_insecure_http: true
        }
        .validate()
        .is_ok()
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn expired_access_with_bad_signature_cannot_trigger_refresh() {
    isolated(|database| async move {
        let (_, secret, _) = session_setup(&database).await;
        let container = container(database).await;
        let (foreign, _) = token_keys();
        let expired = foreign
            .issue(
                "forged",
                uuid::Uuid::new_v4(),
                Default::default(),
                now_seconds() - 601,
            )
            .unwrap();
        let response = app(container)
            .oneshot(request(
                "GET",
                "/web/auth/me",
                &format!(
                    "{ACCESS}={}; {REFRESH}={}",
                    expired.expose_secret(),
                    secret.expose_secret()
                ),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_cleared(&response);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn duplicate_auth_cookies_are_rejected_before_jars_choose_a_value() {
    isolated(|database| async move {
        let container = container(database).await;
        let token = access(&container, "valid", uuid::Uuid::new_v4(), now_seconds());
        let response = app(container)
            .oneshot(request(
                "GET",
                "/web/auth/me",
                &format!("{ACCESS}={}; {ACCESS}=invalid", token.expose_secret()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_cleared(&response);
    })
    .await;
}

#[test]
fn insecure_development_cookies_require_http_opt_in_and_have_separate_names() {
    let config = Config {
        origin: "http://localhost:8080".into(),
        allow_insecure_http: true,
    };
    config.validate().unwrap();
    let jar = Cookies::default();
    clear(&jar, &config);
    for cookie in jar.list() {
        assert!(cookie.name().starts_with("tlab_dev_"));
        assert_eq!(cookie.secure(), Some(false));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.path(), Some("/"));
    }
    let config = Config {
        allow_insecure_http: true,
        ..Config::default()
    };
    assert!(
        !config.insecure(),
        "HTTP opt-in must not weaken an HTTPS origin"
    );
}

#[test]
fn access_cookie_limit_counts_the_entire_serialized_value_in_bytes() {
    // Claims only supply the expiry; transport sizing is independent of signing.
    let claims = AccessClaims {
        iss: "issuer".into(),
        aud: "audience".into(),
        sub: "1".into(),
        sid: uuid::Uuid::new_v4(),
        iat: now_seconds(),
        exp: now_seconds() + 600,
        user: Default::default(),
    };
    for config in [
        Config::default(),
        Config {
            origin: "http://localhost:8080".into(),
            allow_insecure_http: true,
        },
    ] {
        let overhead = access_cookie(&config, "", &claims)
            .unwrap()
            .to_string()
            .len();
        let exact = "a".repeat(4_000 - overhead);
        let cookie = access_cookie(&config, &exact, &claims).unwrap();
        assert_eq!(cookie.to_string().len(), 4_000);
        assert!(access_cookie(&config, &(exact + "a"), &claims).is_err());
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn encoded_auth_cookie_names_are_rejected_before_authentication() {
    isolated(|database| async move {
        let container = container(database).await;
        let token = access(&container, "attacker", uuid::Uuid::new_v4(), now_seconds());
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(container.database.conn().await.unwrap());
        }
        let app = app(container);
        for alias in [
            "__Host%2Dtlab_access",
            "%5F_Host-tlab_access",
            "__Host%2Dtlab_refresh",
            "%5F_Host-tlab_refresh",
        ] {
            for mixed in [false, true] {
                let mut cookie = format!("{alias}={}", token.expose_secret());
                if mixed {
                    cookie.push_str(&format!("; {ACCESS}={}", token.expose_secret()));
                }
                let response = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    app.clone().oneshot(request("GET", "/web/auth/me", &cookie)),
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::UNAUTHORIZED,
                    "{alias}, mixed={mixed}"
                );
                assert_private(&response);
                assert_cleared(&response);
            }
        }
        drop(held);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL schema"]
async fn oversized_web_login_preserves_issued_session_and_refresh_preserves_existing_session() {
    isolated(|database| async move {
        let container = container(database).await;
        let app = app(container.clone());
        for (index, nickname) in ["a".repeat(3_000), "가".repeat(1_000)].into_iter().enumerate() {
            let email = format!("large-{index}@example.test");
            let credentials = json!({"email": email, "password": "secret"});
            let send = |path: &str, payload: Value| {
                let req = HttpRequest::post(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, container.config.auth.web.origin.as_str())
                    .header(header::COOKIE, "other=preserve")
                    .body(Body::from(payload.to_string())).unwrap();
                app.clone().oneshot(req)
            };
            assert_eq!(send("/api/v1/accounts/local", json!({"email": email, "password": "secret", "nickname": nickname})).await.unwrap().status(), StatusCode::CREATED);
            let count = async || {
                let mut conn = container.database.conn().await.unwrap();
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tlab_auth_refresh_token")
                    .fetch_one(conn.context().backend()).await.unwrap()
            };
            let before = count().await;
            let response = send("/web/auth/local", credentials.clone()).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_private(&response);
            assert!(set_cookies(&response).is_empty());
            assert_eq!(body(response).await["code"], "AUTH_COOKIE_TOO_LARGE");
            assert_eq!(count().await, before + 1, "cookie preparation must not roll back committed token issuance");

            // JSON APIs retain their existing size contract and can create this session.
            let response = send("/api/v1/auth/local", credentials).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let tokens = body(response).await;
            let refresh = tokens["refresh_token"].as_str().unwrap();
            let snapshot = async || {
                let mut conn = container.database.conn().await.unwrap();
                sqlx::query_scalar::<_, String>("SELECT row_to_json(r)::text FROM tlab_auth_refresh_token r WHERE token_hash=$1")
                    .bind(super::super::token::RefreshHash::of(refresh).as_bytes().as_slice())
                    .fetch_one(conn.context().backend()).await.unwrap()
            };
            let before = snapshot().await;
            let response = app.clone().oneshot(request("GET", "/web/auth/me", &format!("{REFRESH}={refresh}"))).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_private(&response);
            assert!(set_cookies(&response).is_empty());
            assert_eq!(body(response).await["code"], "AUTH_COOKIE_TOO_LARGE");
            assert_eq!(snapshot().await, before);
        }
    }).await;
}
