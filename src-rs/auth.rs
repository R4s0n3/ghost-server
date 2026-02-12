use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AuthService {
    http: reqwest::Client,
    jwks_cache: Arc<RwLock<HashMap<String, CachedJwks>>>,
    jwks_ttl: Duration,
    expected_issuer: Option<String>,
}

#[derive(Clone)]
struct CachedJwks {
    keys: Vec<Jwk>,
    fetched_at: Instant,
}

#[derive(Debug, Deserialize, Clone)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize, Clone)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
    alg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnverifiedClaims {
    iss: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClerkClaims {
    pub sub: String,
    pub iss: String,
    pub exp: usize,
    pub nbf: Option<usize>,
}

impl AuthService {
    pub fn new(expected_issuer: Option<String>) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .build()
            .context("failed to build auth HTTP client")?;

        Ok(Self {
            http,
            jwks_cache: Arc::new(RwLock::new(HashMap::new())),
            jwks_ttl: Duration::from_secs(10 * 60),
            expected_issuer: expected_issuer
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty()),
        })
    }

    pub async fn verify_bearer_token(
        &self,
        authorization_header: &str,
    ) -> anyhow::Result<ClerkClaims> {
        let token = extract_bearer_token(authorization_header)?;
        self.verify_token(token).await
    }

    pub async fn verify_token(&self, token: &str) -> anyhow::Result<ClerkClaims> {
        let header = decode_header(token).context("invalid JWT header")?;
        let kid = header
            .kid
            .clone()
            .ok_or_else(|| anyhow!("JWT header missing kid"))?;

        let unverified_claims = parse_unverified_claims(token)?;
        let issuer = unverified_claims
            .iss
            .ok_or_else(|| anyhow!("JWT missing iss claim"))?;
        let issuer = issuer.trim().trim_end_matches('/').to_string();

        if let Some(expected_issuer) = &self.expected_issuer {
            if issuer != *expected_issuer {
                return Err(anyhow!(
                    "JWT issuer mismatch. expected={}, got={}",
                    expected_issuer,
                    issuer
                ));
            }
        }

        let jwks = self.get_jwks(&issuer).await?;
        let jwk = jwks
            .iter()
            .find(|candidate| candidate.kid.as_deref() == Some(kid.as_str()))
            .ok_or_else(|| anyhow!("No matching JWK found for kid"))?;

        if jwk.kty != "RSA" {
            return Err(anyhow!("Unsupported JWK type: {}", jwk.kty));
        }

        if let Some(alg) = &jwk.alg {
            if alg != "RS256" {
                return Err(anyhow!("Unsupported JWK alg: {}", alg));
            }
        }

        let n = jwk
            .n
            .as_ref()
            .ok_or_else(|| anyhow!("JWK missing modulus (n)"))?;
        let e = jwk
            .e
            .as_ref()
            .ok_or_else(|| anyhow!("JWK missing exponent (e)"))?;

        let decoding_key =
            DecodingKey::from_rsa_components(n, e).context("failed to build RSA decoding key")?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_nbf = true;
        validation.set_issuer(&[issuer.as_str()]);

        let token_data = decode::<ClerkClaims>(token, &decoding_key, &validation)
            .context("JWT signature validation failed")?;

        let claims = token_data.claims;
        tracing::debug!(
            iss = %claims.iss,
            exp = claims.exp,
            nbf = ?claims.nbf,
            "verified Clerk bearer token"
        );
        Ok(claims)
    }

    async fn get_jwks(&self, issuer: &str) -> anyhow::Result<Vec<Jwk>> {
        {
            let cache = self.jwks_cache.read().await;
            if let Some(cached) = cache.get(issuer) {
                if cached.fetched_at.elapsed() < self.jwks_ttl {
                    return Ok(cached.keys.clone());
                }
            }
        }

        let jwks_url = format!("{}/.well-known/jwks.json", issuer.trim_end_matches('/'));
        let response = self
            .http
            .get(&jwks_url)
            .send()
            .await
            .with_context(|| format!("failed to fetch JWKS from {jwks_url}"))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "failed to fetch JWKS from {}: HTTP {}",
                jwks_url,
                response.status()
            ));
        }

        let jwks: Jwks = response
            .json()
            .await
            .with_context(|| format!("invalid JWKS response from {jwks_url}"))?;

        let keys = jwks.keys;
        let mut cache = self.jwks_cache.write().await;
        cache.insert(
            issuer.to_string(),
            CachedJwks {
                keys: keys.clone(),
                fetched_at: Instant::now(),
            },
        );

        Ok(keys)
    }
}

pub fn extract_bearer_token(value: &str) -> anyhow::Result<&str> {
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default();

    if !scheme.eq_ignore_ascii_case("bearer") || token.trim().is_empty() {
        return Err(anyhow!("Invalid Authorization header format"));
    }

    Ok(token.trim())
}

fn parse_unverified_claims(token: &str) -> anyhow::Result<UnverifiedClaims> {
    let mut parts = token.split('.');
    let _header = parts.next();
    let payload = parts
        .next()
        .ok_or_else(|| anyhow!("JWT payload segment missing"))?;

    let decoded = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .context("failed to decode JWT payload")?;

    serde_json::from_slice::<UnverifiedClaims>(&decoded)
        .context("failed to parse unverified JWT claims")
}
