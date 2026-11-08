use super::token::{AccessClaims, AccessVerifier};
use crate::app::response;
use axum::{
    extract::{FromRef, FromRequestParts},
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
        let reject = || {
            let mut response = response::error(
                StatusCode::UNAUTHORIZED,
                "INVALID_ACCESS_TOKEN",
                None,
                "authentication required",
            );
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            response
        };
        let mut headers = parts.headers.get_all(AUTHORIZATION).iter();
        let value = headers.next().ok_or_else(reject)?;
        if headers.next().is_some() {
            return Err(reject());
        }
        let value = value.to_str().map_err(|_| reject())?;
        let (scheme, token) = value.split_once(' ').ok_or_else(reject)?;
        if !scheme.eq_ignore_ascii_case("Bearer")
            || token.is_empty()
            || token.bytes().any(|b| b.is_ascii_whitespace() || b == b',')
        {
            return Err(reject());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| reject())?
            .as_secs();
        AccessVerifier::from_ref(state)
            .verify(token, now)
            .map_err(|_| reject())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, body::Body, http::Request, routing::post};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn protected(claims: AccessClaims, Json(body): Json<Value>) -> Json<Value> {
        Json(json!({"sub": claims.sub, "body": body}))
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
        let key = EncodingKey::from_ed_pem(material.private_key_pem().as_bytes()).unwrap();
        let claims = json!({"iss":"issuer","aud":"audience","sub":"123","sid":uuid::Uuid::new_v4(),"iat":now,"exp":now+600,"user":{}});
        let sign = |value: &Value| encode(&Header::new(Algorithm::EdDSA), value, &key).unwrap();
        let mut invalid = Vec::new();
        for (field, value) in [
            ("iss", json!("wrong")),
            ("aud", json!("wrong")),
            ("sub", json!("")),
        ] {
            let mut altered = claims.clone();
            altered[field] = value;
            invalid.push(sign(&altered));
        }
        let mut expired = claims.clone();
        expired["iat"] = json!(now - 601);
        expired["exp"] = json!(now - 1);
        invalid.push(sign(&expired));
        let mut missing = claims.clone();
        missing.as_object_mut().unwrap().remove("sid");
        invalid.push(sign(&missing));
        invalid.push(
            encode(
                &Header::new(Algorithm::HS256),
                &claims,
                &EncodingKey::from_secret(b"wrong"),
            )
            .unwrap(),
        );
        let mut tampered = token.expose_secret().to_owned();
        let signature_start = tampered.rfind('.').unwrap() + 1;
        let replacement = if &tampered[signature_start..signature_start + 1] == "A" {
            "B"
        } else {
            "A"
        };
        tampered.replace_range(signature_start..signature_start + 1, replacement);
        invalid.push(tampered);
        let mut cases = vec![
            vec![],
            vec!["Basic abc".into()],
            vec!["Bearer  abc".into()],
            vec![
                format!("Bearer {}", token.expose_secret()),
                format!("Bearer {}", token.expose_secret()),
            ],
        ];
        cases.extend(
            invalid
                .into_iter()
                .map(|value| vec![format!("Bearer {value}")]),
        );
        for case in cases {
            let response = send(case).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()[WWW_AUTHENTICATE], "Bearer");
        }
    }
}
