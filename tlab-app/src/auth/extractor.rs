use super::token::{AccessClaims, AccessVerifier, Error as TokenError};
use crate::app::response;
use axum::{
    extract::{FromRef, FromRequestParts, OptionalFromRequestParts},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
        request::Parts,
    },
    response::Response,
};
use std::time::{SystemTime, UNIX_EPOCH};

impl<S> FromRequestParts<S> for AccessClaims
where
    S: Send + Sync,
    AccessVerifier: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match <Self as OptionalFromRequestParts<S>>::from_request_parts(parts, state).await {
            Ok(Some(claims)) => Ok(claims),
            Ok(None) => {
                let mut response = response::error(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_ACCESS_TOKEN",
                    "authentication required",
                );
                response
                    .headers_mut()
                    .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
                Err(response)
            }
            Err(response) => Err(response),
        }
    }
}

impl<S> OptionalFromRequestParts<S> for AccessClaims
where
    S: Send + Sync,
    AccessVerifier: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        if let Some(verified) = parts.extensions.get::<super::web::VerifiedWebClaims>() {
            return Ok(Some(verified.claims().clone()));
        }
        let Some(token) = bearer_token(parts) else {
            return Ok(None);
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| response::internal_error("authentication clock failed"))?
            .as_secs();
        verified_claims(AccessVerifier::from_ref(state).verify(token, now))
            .map_err(|_| response::internal_error("access verification failed"))
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    let mut headers = parts.headers.get_all(AUTHORIZATION).iter();
    let value = headers.next()?;
    if headers.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("Bearer")
        && !token.is_empty()
        && !token.bytes().any(|b| b.is_ascii_whitespace() || b == b','))
    .then_some(token)
}

fn verified_claims(
    result: Result<AccessClaims, TokenError>,
) -> Result<Option<AccessClaims>, TokenError> {
    match result {
        Ok(claims) => Ok(Some(claims)),
        Err(TokenError::InvalidAccess | TokenError::ExpiredAccess) => Ok(None),
        Err(
            error @ (TokenError::InvalidConfiguration
            | TokenError::InvalidInput
            | TokenError::SigningFailed
            | TokenError::RandomFailed),
        ) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, body::Body, http::Request, routing::post};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn protected(claims: AccessClaims, Json(body): Json<Value>) -> Json<Value> {
        Json(json!({"sub": claims.sub, "body": body}))
    }

    async fn optional(claims: Option<AccessClaims>) -> Json<Value> {
        Json(json!({"sub": claims.map(|claims| claims.sub)}))
    }

    #[test]
    fn verification_errors_preserve_internal_failures() {
        for error in [TokenError::InvalidAccess, TokenError::ExpiredAccess] {
            assert!(matches!(verified_claims(Err(error)), Ok(None)));
        }
        for error in [
            TokenError::InvalidConfiguration,
            TokenError::InvalidInput,
            TokenError::SigningFailed,
            TokenError::RandomFailed,
        ] {
            let expected = std::mem::discriminant(&error);
            let Err(error) = verified_claims(Err(error)) else {
                panic!("internal verification failure must remain an error");
            };
            assert_eq!(std::mem::discriminant(&error), expected);
        }
    }

    #[tokio::test]
    async fn optional_authentication_returns_none_without_a_valid_access_token() {
        let (issuer, verifier) = crate::auth::test_support::token_keys();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = issuer
            .issue("123", uuid::Uuid::new_v4(), Default::default(), now)
            .unwrap();
        let expired = issuer
            .issue("123", uuid::Uuid::new_v4(), Default::default(), now - 600)
            .unwrap();
        let router = Router::new()
            .route("/", post(optional))
            .with_state(verifier);
        for (auth, expected) in [
            (None, json!({"sub": null})),
            (
                Some(format!("Bearer {}", token.expose_secret())),
                json!({"sub": "123"}),
            ),
            (Some("Bearer invalid".into()), json!({"sub": null})),
            (Some("Basic abc".into()), json!({"sub": null})),
            (Some(String::new()), json!({"sub": null})),
            (Some("Bearer ".into()), json!({"sub": null})),
            (
                Some(format!("Bearer {}", expired.expose_secret())),
                json!({"sub": null}),
            ),
        ] {
            let mut request = Request::post("/");
            if let Some(auth) = auth {
                request = request.header(AUTHORIZATION, auth);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn arbitrary_claims_extension_cannot_authenticate_a_request() {
        let (issuer, verifier) = crate::auth::test_support::token_keys();
        let token = issuer
            .issue(
                "injected",
                uuid::Uuid::new_v4(),
                Default::default(),
                1_800_000_000,
            )
            .unwrap();
        let untrusted = verifier
            .verify(token.expose_secret(), 1_800_000_000)
            .unwrap();
        let router = Router::new()
            .route("/", post(protected))
            .with_state(verifier);
        let mut request = Request::post("/")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        request.extensions_mut().insert(untrusted);
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_contract_without_database() {
        let dir = tempfile::tempdir().unwrap();
        let material = tlab::keyfiles::Ed25519KeyFile {
            private_key: dir.path().join("private"),
            public_key: dir.path().join("public"),
            generate_if_missing: true,
        }
        .load()
        .unwrap();
        let issuer =
            super::super::token::AccessIssuer::new(&material, "issuer", "audience").unwrap();
        let verifier =
            AccessVerifier::new(material.public_key_pem(), "issuer", "audience").unwrap();
        let router = Router::new()
            .route("/", post(protected))
            .with_state(verifier);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = issuer
            .issue("123", uuid::Uuid::new_v4(), Default::default(), now)
            .unwrap();
        let send = |auth: Vec<String>| {
            let router = router.clone();
            async move {
                let mut req = Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"hello\":true}"))
                    .unwrap();
                for value in auth {
                    req.headers_mut()
                        .append(AUTHORIZATION, value.parse().unwrap());
                }
                router.oneshot(req).await.unwrap()
            }
        };
        let response = send(vec![format!("bEaReR {}", token.expose_secret())]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"sub":"123", "body":{"hello":true}})
        );
        let mut tampered = token.expose_secret().to_owned();
        let signature_start = tampered.rfind('.').unwrap() + 1;
        let replacement = if &tampered[signature_start..signature_start + 1] == "A" {
            "B"
        } else {
            "A"
        };
        tampered.replace_range(signature_start..signature_start + 1, replacement);
        // Cryptographic and claim permutations belong to token tests; this checks
        // that the HTTP boundary turns verifier rejection into a Bearer challenge.
        for (name, auth) in [
            ("missing", vec![]),
            ("wrong scheme", vec!["Basic abc".into()]),
            ("extra space", vec!["Bearer  abc".into()]),
            (
                "duplicate authorization",
                vec![
                    format!("Bearer {}", token.expose_secret()),
                    format!("Bearer {}", token.expose_secret()),
                ],
            ),
            ("invalid signature", vec![format!("Bearer {tampered}")]),
        ] {
            let response = send(auth).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{name}");
            assert_eq!(response.headers()[WWW_AUTHENTICATE], "Bearer", "{name}");
        }
    }
}
