use anyhow::{anyhow, Context};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

#[derive(Clone)]
pub struct ClerkClient {
    http: reqwest::Client,
    api_base: String,
}

#[derive(Debug, Deserialize)]
pub struct ClerkUser {
    pub primary_email_address_id: Option<String>,
    #[serde(default)]
    pub email_addresses: Vec<ClerkEmailAddress>,
}

#[derive(Debug, Deserialize)]
pub struct ClerkEmailAddress {
    pub id: String,
    pub email_address: String,
}

impl ClerkClient {
    pub fn new(api_base: String, secret_key: Option<&str>) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(secret) = secret_key {
            let value = format!("Bearer {}", secret);
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&value).context("invalid CLERK_SECRET_KEY for header")?,
            );
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build Clerk HTTP client")?;

        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
        })
    }

    pub async fn get_user(&self, user_id: &str) -> anyhow::Result<ClerkUser> {
        let url = format!("{}/users/{}", self.api_base, user_id);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to call Clerk API for user {user_id}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Clerk API get user failed with status {}: {}",
                status,
                body
            ));
        }

        response
            .json::<ClerkUser>()
            .await
            .context("failed to decode Clerk user response")
    }

    pub async fn get_primary_email(&self, user_id: &str) -> anyhow::Result<Option<String>> {
        let user = self.get_user(user_id).await?;
        let primary_id = match user.primary_email_address_id {
            Some(value) => value,
            None => return Ok(None),
        };

        let email = user
            .email_addresses
            .into_iter()
            .find(|entry| entry.id == primary_id)
            .map(|entry| entry.email_address);

        Ok(email)
    }
}
