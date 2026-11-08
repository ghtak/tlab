//! Cookie transport and request authentication. Business lifecycle stays in slices.
use axum::response::IntoResponse;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use tower_cookies::{
    Cookie, CookieManagerLayer, Cookies,
    cookie::{
        SameSite,
        time::{Duration, OffsetDateTime},
    },
};

use super::{
    slice,
    token::{AccessClaims, Error as TokenError},
};
use crate::app::{Container, response};

pub(super) const ACCESS: &str = "__Host-tlab_access";
pub(super) const REFRESH: &str = "__Host-tlab_refresh";
// Include every attribute in the limit, leaving room below browser limits.
const MAX_COOKIE_BYTES: usize = 4_000;

pub(super) fn cookie_too_large() -> Response {
    response::error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "AUTH_COOKIE_TOO_LARGE",
        "authentication cookie exceeds the size limit",
    )
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub origin: String,
    /// Explicit development opt-in; production cookies always require HTTPS.
    pub allow_insecure_http: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            origin: "https://localhost:8081".into(),
            allow_insecure_http: false,
        }
    }
}
impl Config {
    fn insecure(&self) -> bool {
        self.allow_insecure_http && self.origin.starts_with("http://")
    }
    fn access_name(&self) -> &'static str {
        if self.insecure() {
            "tlab_dev_access"
        } else {
            ACCESS
        }
    }
    pub(super) fn refresh_name(&self) -> &'static str {
        if self.insecure() {
            "tlab_dev_refresh"
        } else {
            REFRESH
        }
    }
    pub fn validate(&self) -> tlab::Result<()> {
        let uri = self.origin.parse::<axum::http::Uri>().ok();
        let valid = uri.as_ref().is_some_and(|uri| {
            let scheme = uri.scheme_str();
            uri.authority().is_some_and(|a| !a.as_str().contains('@'))
                && (scheme == Some("https") || (self.allow_insecure_http && scheme == Some("http")))
                && uri.path() == "/"
                && uri.query().is_none()
                && format!("{}://{}", scheme.unwrap_or(""), uri.authority().unwrap()) == self.origin
        });
        if !valid {
            return Err(tlab::Error::InvalidInput("auth.web.origin must be an exact HTTPS origin (HTTP requires explicit development opt-in)".into()));
        }
        Ok(())
    }
}

// Only this module can construct this marker. Raw AccessClaims extensions are
// ignored by the extractor; no header is synthesized or trusted as web auth.
#[derive(Clone)]
pub(super) struct VerifiedWebClaims(AccessClaims);
impl VerifiedWebClaims {
    pub(super) fn claims(&self) -> &AccessClaims {
        &self.0
    }
}

pub(crate) fn router(container: Arc<Container>) -> Router<Arc<Container>> {
    let protected =
        Router::new()
            .route("/web/auth/me", get(me))
            .route_layer(middleware::from_fn_with_state(
                container.clone(),
                authenticate,
            ));
    Router::new()
        .merge(protected)
        .route("/web/auth/local", post(slice::login_local::web_login))
        .route("/web/auth/logout", post(slice::logout::web_logout))
        .layer(middleware::from_fn_with_state(container, guard))
        // Last added = outermost: read jar before auth; append delta after it.
        .layer(CookieManagerLayer::new())
}

async fn me(claims: AccessClaims) -> Response {
    response::success(
        StatusCode::OK,
        serde_json::json!({"sub": claims.sub, "sid": claims.sid, "user": claims.user}),
    )
}

pub(super) fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    response
}
fn denied() -> Response {
    response::error(
        StatusCode::UNAUTHORIZED,
        "INVALID_ACCESS_TOKEN",
        "authentication required",
    )
}

// Origin is mandatory for unsafe requests, including login/logout. Host and
// forwarded headers do not determine trust. Fetch metadata also blocks cross-
// origin navigation from triggering a refresh; absent metadata permits clients.
async fn guard(
    State(container): State<Arc<Container>>,
    cookies: Cookies,
    request: Request,
    next: Next,
) -> Response {
    let origins: Vec<_> = request.headers().get_all(header::ORIGIN).iter().collect();
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let origin_ok = match origins.as_slice() {
        [] => safe,
        [origin] => origin.to_str().ok() == Some(container.config.auth.web.origin.as_str()),
        _ => false,
    };
    let fetch_ok = request
        .headers()
        .get("sec-fetch-site")
        .is_none_or(|value| value == "same-origin" || value == "none");
    let response = if !origin_ok || !fetch_ok {
        response::error(
            StatusCode::FORBIDDEN,
            "CSRF_REJECTED",
            "same-origin request required",
        )
    } else if invalid_cookie_header(&request, &container.config.auth.web) {
        clear(&cookies, &container.config.auth.web);
        denied()
    } else {
        next.run(request).await
    };
    no_store(response)
}
fn invalid_cookie_header(request: &Request, config: &Config) -> bool {
    let mut access = 0;
    let mut refresh = 0;
    for value in request.headers().get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else {
            return true;
        };
        for pair in value.split(';') {
            let Ok(raw) = Cookie::parse(pair.trim().to_owned()) else {
                return true;
            };
            let Ok(parsed) = Cookie::parse_encoded(pair.trim().to_owned()) else {
                return true;
            };
            // Browsers enforce __Host- on the wire name, before decoding.
            // Never let an encoded Domain cookie alias a host-only credential.
            if (parsed.name() == config.access_name() || parsed.name() == config.refresh_name())
                && raw.name() != parsed.name()
            {
                return true;
            }
            if parsed.name() == config.access_name() {
                access += 1;
            }
            if parsed.name() == config.refresh_name() {
                refresh += 1;
            }
        }
    }
    access > 1 || refresh > 1
}

async fn authenticate(
    State(container): State<Arc<Container>>,
    cookies: Cookies,
    mut request: Request,
    next: Next,
) -> Response {
    let Ok(now) = u64::try_from(OffsetDateTime::now_utc().unix_timestamp()) else {
        return no_store(response::internal_error("authentication clock failed"));
    };
    let result = cookies
        .get(container.config.auth.web.access_name())
        .map(|cookie| container.access_verifier.verify(cookie.value(), now));
    let claims = match result {
        Some(Ok(claims)) => claims,
        Some(Err(TokenError::ExpiredAccess)) | None => {
            let Some(refresh) = cookies.get(container.config.auth.web.refresh_name()) else {
                clear(&cookies, &container.config.auth.web);
                return no_store(denied());
            };
            match slice::refresh_access_token::execute(
                &container.database,
                refresh.value(),
                &container.access_issuer,
            )
            .await
            {
                Ok(access) => {
                    let Ok(claims) = container
                        .access_verifier
                        .verify(access.expose_secret(), now_seconds())
                    else {
                        return no_store(response::internal_error(
                            "issued access verification failed",
                        ));
                    };
                    let Ok(cookie) =
                        access_cookie(&container.config.auth.web, access.expose_secret(), &claims)
                    else {
                        return no_store(cookie_too_large());
                    };
                    cookies.add(cookie);
                    claims
                }
                Err(crate::app::error::AppError {
                    code: crate::app::error::AppErrorCode::InvalidRefreshToken,
                    ..
                }) => {
                    clear(&cookies, &container.config.auth.web);
                    return no_store(denied());
                }
                Err(error) => {
                    return no_store(error.into_response());
                }
            }
        }
        Some(Err(_)) => {
            clear(&cookies, &container.config.auth.web);
            return no_store(denied());
        }
    };
    request.extensions_mut().insert(VerifiedWebClaims(claims));
    no_store(next.run(request).await)
}
fn now_seconds() -> u64 {
    u64::try_from(OffsetDateTime::now_utc().unix_timestamp()).unwrap_or(0)
}
fn cookie(config: &Config, name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(!config.insecure())
        .same_site(SameSite::Lax)
        .build()
}
fn checked_cookie(value: Cookie<'static>) -> Result<Cookie<'static>, ()> {
    if value.to_string().len() > MAX_COOKIE_BYTES {
        return Err(());
    }
    Ok(value)
}

fn access_cookie(
    config: &Config,
    token: &str,
    claims: &AccessClaims,
) -> Result<Cookie<'static>, ()> {
    let mut value = cookie(config, config.access_name(), token.into());
    value.set_expires(
        OffsetDateTime::from_unix_timestamp(claims.exp as i64).expect("validated token timestamp"),
    );
    value.set_max_age(Duration::seconds(
        claims.exp.saturating_sub(now_seconds()).min(600) as i64,
    ));
    checked_cookie(value)
}
pub(super) fn login_cookies(
    config: &Config,
    tokens: &super::LoginTokens,
    claims: &AccessClaims,
) -> Result<[Cookie<'static>; 2], ()> {
    let access = access_cookie(config, tokens.access.expose_secret(), claims)?;
    let mut value = cookie(
        config,
        config.refresh_name(),
        tokens.refresh.expose_secret().into(),
    );
    let expires = claims.iat + 604800;
    value.set_expires(
        OffsetDateTime::from_unix_timestamp(expires as i64).expect("issued timestamp"),
    );
    value.set_max_age(Duration::seconds(
        expires.saturating_sub(now_seconds()).min(604800) as i64,
    ));
    Ok([access, checked_cookie(value)?])
}
pub(super) fn clear(cookies: &Cookies, config: &Config) {
    // Explicit expired Set-Cookie even when the browser omitted one cookie.
    for name in [config.access_name(), config.refresh_name()] {
        let mut value = cookie(config, name, String::new());
        value.make_removal();
        cookies.add(value);
    }
}

#[cfg(test)]
mod tests;
