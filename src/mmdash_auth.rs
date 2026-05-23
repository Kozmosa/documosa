use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::models::{Identity, RoleMode};

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    name: Option<String>,
    email: Option<String>,
    #[allow(dead_code)]
    exp: i64,
}

pub struct MmdashIdentity(pub Identity);

impl<S: Send + Sync> FromRequestParts<S> for MmdashIdentity {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        let strict_mmdash = is_mmdash_path(parts.uri.path());
        let authorization = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());

        if let Some(header) = authorization {
            if let Some(token) = header.strip_prefix("Bearer ") {
                match decode_mmdash_identity(token) {
                    Ok(identity) => return Ok(MmdashIdentity(identity)),
                    Err(error) if strict_mmdash => return Err(error),
                    Err(_) => {}
                }
            } else if strict_mmdash {
                return Err(AppError::Unauthorized);
            }
        } else if strict_mmdash {
            return Err(AppError::Unauthorized);
        }

        Ok(MmdashIdentity(native_identity(&parts.headers)?))
    }
}

fn is_mmdash_path(path: &str) -> bool {
    path == "/api/mmdash" || path.starts_with("/api/mmdash/")
}

fn header_value(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn native_identity(headers: &HeaderMap) -> Result<Identity> {
    let client_id =
        header_value(headers, "x-documosa-client-id").unwrap_or_else(|| "native-ui".to_string());
    let nickname =
        header_value(headers, "x-documosa-nickname").unwrap_or_else(|| "Native UI".to_string());
    let role_mode = header_value(headers, "x-documosa-role-mode")
        .as_deref()
        .map(RoleMode::parse)
        .transpose()?
        .unwrap_or(RoleMode::Writer);

    Ok(Identity {
        client_id,
        nickname,
        role_mode,
        actor_kind: Default::default(),
    })
}

fn decode_mmdash_identity(token: &str) -> Result<Identity> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| AppError::Unauthorized)?;
    let algorithm = std::env::var("JWT_ALGORITHM")
        .ok()
        .and_then(|s| parse_algorithm(&s))
        .unwrap_or(Algorithm::HS256);

    let decoding_key = DecodingKey::from_secret(secret.as_bytes());
    let mut validation = Validation::new(algorithm);
    validation.validate_aud = false;

    let token_data =
        decode::<Claims>(token, &decoding_key, &validation).map_err(|_| AppError::Unauthorized)?;

    let claims = token_data.claims;
    let nickname = claims
        .name
        .filter(|s| !s.trim().is_empty())
        .or(claims.email.filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "mmdash-user".into());

    Ok(Identity {
        client_id: format!("mmdash-{}", claims.sub),
        nickname,
        role_mode: RoleMode::Writer,
        actor_kind: Default::default(),
    })
}

fn parse_algorithm(value: &str) -> Option<Algorithm> {
    match value {
        "HS256" => Some(Algorithm::HS256),
        "HS384" => Some(Algorithm::HS384),
        "HS512" => Some(Algorithm::HS512),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde::Serialize;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Serialize)]
    struct TestClaims {
        sub: String,
        name: Option<String>,
        email: Option<String>,
        exp: i64,
    }

    fn make_token(secret: &str, claims: TestClaims) -> String {
        let header = Header::new(Algorithm::HS256);
        encode(
            &header,
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn test_parse_algorithm() {
        assert_eq!(parse_algorithm("HS256"), Some(Algorithm::HS256));
        assert_eq!(parse_algorithm("HS384"), Some(Algorithm::HS384));
        assert_eq!(parse_algorithm("HS512"), Some(Algorithm::HS512));
        assert_eq!(parse_algorithm("RS256"), None);
    }

    #[test]
    fn test_middleware_valid_token() {
        let secret = "test-secret-123";
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let token = make_token(
            secret,
            TestClaims {
                sub: "user-42".into(),
                name: Some("Alice".into()),
                email: Some("alice@example.com".into()),
                exp: now + 3600,
            },
        );

        unsafe { std::env::set_var("JWT_SECRET", secret) };
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_aud = false;

        let result = decode::<Claims>(&token, &decoding_key, &validation);
        assert!(result.is_ok());
        let claims = result.unwrap().claims;
        assert_eq!(claims.sub, "user-42");
        assert_eq!(claims.name, Some("Alice".into()));
    }

    #[test]
    fn test_middleware_expired_token() {
        let secret = "test-secret-456";
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let token = make_token(
            secret,
            TestClaims {
                sub: "user-42".into(),
                name: Some("Alice".into()),
                email: None,
                exp: now - 10,
            },
        );

        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_aud = false;
        validation.leeway = 0;

        let result = decode::<Claims>(&token, &decoding_key, &validation);
        assert!(result.is_err());
    }
}
