//! Clerk session tokens and roles, as in jrg-life: `privateMetadata.role` must be `admin` and
//! `privateMetadata.scope` must list this app.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::Context;
use axum::http::StatusCode;
use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::error::{Result, status};

/// How long a role read from Clerk is trusted (jrg-life's ROLE_TTL_MS).
const ROLE_TTL: Duration = Duration::from_secs(60);
/// Unknown key ids refetch the JWKS at most this often, so forged tokens cannot hammer Clerk.
const JWKS_REFRESH: Duration = Duration::from_secs(60);
const API: &str = "https://api.clerk.com/v1";

pub struct Clerk {
    secret_key: String,
    pub publishable_key: String,
    /// e.g. `foo.clerk.accounts.dev`, decoded from the publishable key; the tokens' issuer.
    frontend_api: String,
    scope: String,
    jwks: RwLock<(JwkSet, Option<Instant>)>,
    roles: Mutex<HashMap<String, (bool, Instant)>>,
}

impl Clerk {
    pub fn new(secret_key: String, publishable_key: String, scope: String) -> anyhow::Result<Self> {
        let encoded = publishable_key
            .strip_prefix("pk_test_")
            .or_else(|| publishable_key.strip_prefix("pk_live_"))
            .context("CLERK_PUBLISHABLE_KEY must start with pk_test_ or pk_live_")?;
        let decoded = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(encoded.trim_end_matches('='))?;
        let frontend_api = String::from_utf8(decoded)?
            .trim_end_matches('$')
            .to_string();
        Ok(Self {
            secret_key,
            publishable_key,
            frontend_api,
            scope,
            jwks: RwLock::new((JwkSet { keys: vec![] }, None)),
            roles: Mutex::new(HashMap::new()),
        })
    }

    async fn key(&self, http: &reqwest::Client, kid: &str) -> anyhow::Result<Option<DecodingKey>> {
        {
            let (set, fetched) = &*self.jwks.read().await;
            if let Some(jwk) = set.find(kid) {
                return Ok(Some(DecodingKey::from_jwk(jwk)?));
            }
            if fetched.is_some_and(|t| t.elapsed() < JWKS_REFRESH) {
                return Ok(None);
            }
        }
        let set: JwkSet = self.get(http, "/jwks").await?;
        let key = set.find(kid).map(DecodingKey::from_jwk).transpose()?;
        *self.jwks.write().await = (set, Some(Instant::now()));
        Ok(key)
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        http: &reqwest::Client,
        path: &str,
    ) -> anyhow::Result<T> {
        Ok(http
            .get(format!("{API}{path}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// The Clerk user id of a valid session token.
    pub async fn verify(&self, http: &reqwest::Client, token: &str) -> Result<String> {
        let unauthorized = || status(StatusCode::UNAUTHORIZED, "invalid session");
        let kid = jsonwebtoken::decode_header(token)
            .ok()
            .and_then(|h| h.kid)
            .ok_or_else(unauthorized)?;
        let key = self.key(http, &kid).await?.ok_or_else(unauthorized)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[format!("https://{}", self.frontend_api)]);
        validation.validate_aud = false; // Clerk session tokens carry no audience
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["exp", "nbf", "iss", "sub"]);
        #[derive(Deserialize)]
        struct Claims {
            sub: String,
        }
        let token =
            jsonwebtoken::decode::<Claims>(token, &key, &validation).map_err(|_| unauthorized())?;
        Ok(token.claims.sub)
    }

    pub async fn is_admin(&self, http: &reqwest::Client, user_id: &str) -> anyhow::Result<bool> {
        if let Some((admin, at)) = self.roles.lock().await.get(user_id)
            && at.elapsed() < ROLE_TTL
        {
            return Ok(*admin);
        }
        anyhow::ensure!(
            user_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "unexpected Clerk user id"
        );
        let user: Value = self.get(http, &format!("/users/{user_id}")).await?;
        let admin = is_admin_metadata(&user["private_metadata"], &self.scope);
        self.roles
            .lock()
            .await
            .insert(user_id.to_string(), (admin, Instant::now()));
        Ok(admin)
    }
}

/// Default deny: exactly `role: "admin"`, and `scope` ("a,b" or ["a","b"]) must list this app.
fn is_admin_metadata(meta: &Value, scope: &str) -> bool {
    let scopes: Vec<&str> = match &meta["scope"] {
        Value::String(s) => s.split(',').collect(),
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    meta["role"] == "admin" && scopes.iter().any(|s| s.trim().eq_ignore_ascii_case(scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn admin_needs_role_and_scope() {
        assert!(is_admin_metadata(
            &json!({"role": "admin", "scope": "tree, beacon"}),
            "beacon"
        ));
        assert!(is_admin_metadata(
            &json!({"role": "admin", "scope": ["beacon"]}),
            "beacon"
        ));
        assert!(!is_admin_metadata(
            &json!({"role": "admin", "scope": "awesome"}),
            "beacon"
        ));
        assert!(!is_admin_metadata(
            &json!({"role": "writer", "scope": "beacon"}),
            "beacon"
        ));
        assert!(!is_admin_metadata(&json!({}), "beacon"));
    }

    #[test]
    fn decodes_frontend_api_from_publishable_key() {
        let pk = format!(
            "pk_test_{}",
            base64::engine::general_purpose::STANDARD.encode("foo.clerk.accounts.dev$")
        );
        let clerk = Clerk::new("sk".into(), pk, "beacon".into()).unwrap();
        assert_eq!(clerk.frontend_api, "foo.clerk.accounts.dev");
    }
}
