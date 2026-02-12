use std::collections::HashMap;

use anyhow::{anyhow, Context};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct StripeApi {
    http: reqwest::Client,
    secret_key: Option<String>,
    webhook_secret: Option<String>,
    base_url: String,
}

impl StripeApi {
    pub fn new(secret_key: Option<String>, webhook_secret: Option<String>) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .build()
            .context("failed to create Stripe HTTP client")?;

        Ok(Self {
            http,
            secret_key,
            webhook_secret,
            base_url: "https://api.stripe.com/v1".to_string(),
        })
    }

    pub fn verify_webhook_signature(
        &self,
        signature_header: &str,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        let webhook_secret = self
            .webhook_secret
            .as_ref()
            .ok_or_else(|| anyhow!("STRIPE_WEBHOOK_SECRET is not configured."))?;

        let mut timestamp: Option<i64> = None;
        let mut v1_signatures: Vec<&str> = Vec::new();

        for part in signature_header.split(',') {
            let mut pieces = part.trim().splitn(2, '=');
            let key = pieces.next().unwrap_or_default();
            let value = pieces.next().unwrap_or_default();
            if key == "t" {
                timestamp = value.parse::<i64>().ok();
            } else if key == "v1" {
                v1_signatures.push(value);
            }
        }

        let timestamp =
            timestamp.ok_or_else(|| anyhow!("Missing Stripe timestamp in signature."))?;
        if v1_signatures.is_empty() {
            return Err(anyhow!("Missing Stripe v1 signature."));
        }

        let now = Utc::now().timestamp();
        if (now - timestamp).abs() > 300 {
            return Err(anyhow!("Stripe signature timestamp outside tolerance."));
        }

        let payload_str =
            std::str::from_utf8(payload).context("invalid UTF-8 payload for Stripe signature")?;
        let signed_payload = format!("{}.{}", timestamp, payload_str);

        let mut mac = Hmac::<Sha256>::new_from_slice(webhook_secret.as_bytes())
            .context("invalid Stripe webhook secret")?;
        mac.update(signed_payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        let is_match = v1_signatures
            .into_iter()
            .any(|candidate| expected.as_bytes().ct_eq(candidate.as_bytes()).into());

        if !is_match {
            return Err(anyhow!("Invalid Stripe signature."));
        }

        Ok(())
    }

    pub async fn create_customer(
        &self,
        email: &str,
        clerk_id: &str,
    ) -> anyhow::Result<StripeCustomer> {
        let params = vec![
            ("email".to_string(), email.to_string()),
            ("metadata[clerkId]".to_string(), clerk_id.to_string()),
        ];
        self.post_form("customers", &params).await
    }

    pub async fn retrieve_customer(&self, customer_id: &str) -> anyhow::Result<StripeCustomer> {
        self.get_json(&format!("customers/{}", customer_id), &[])
            .await
    }

    pub async fn create_checkout_session(
        &self,
        customer_id: &str,
        price_id: &str,
        success_url: &str,
        cancel_url: &str,
    ) -> anyhow::Result<StripeCheckoutSession> {
        let params = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("payment_method_types[0]".to_string(), "card".to_string()),
            ("line_items[0][price]".to_string(), price_id.to_string()),
            ("line_items[0][quantity]".to_string(), "1".to_string()),
            ("mode".to_string(), "subscription".to_string()),
            ("success_url".to_string(), success_url.to_string()),
            ("cancel_url".to_string(), cancel_url.to_string()),
        ];

        self.post_form("checkout/sessions", &params).await
    }

    pub async fn retrieve_checkout_session(
        &self,
        session_id: &str,
    ) -> anyhow::Result<StripeCheckoutSession> {
        self.get_json(
            &format!("checkout/sessions/{}", session_id),
            &[("expand[]", "line_items")],
        )
        .await
    }

    pub async fn create_billing_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> anyhow::Result<StripeBillingPortalSession> {
        let params = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("return_url".to_string(), return_url.to_string()),
        ];

        self.post_form("billing_portal/sessions", &params).await
    }

    pub async fn retrieve_subscription(
        &self,
        subscription_id: &str,
    ) -> anyhow::Result<StripeSubscription> {
        self.get_json(&format!("subscriptions/{}", subscription_id), &[])
            .await
    }

    fn require_secret_key(&self) -> anyhow::Result<&str> {
        self.secret_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("STRIPE_SECRET_KEY is not configured."))
    }

    async fn post_form<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> anyhow::Result<T> {
        let key = self.require_secret_key()?;
        let url = format!("{}/{}", self.base_url, path);

        let response = self
            .http
            .post(url)
            .bearer_auth(key)
            .form(params)
            .send()
            .await
            .with_context(|| format!("Stripe POST failed for {}", path))?;

        parse_stripe_response(response, path).await
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let key = self.require_secret_key()?;
        let url = format!("{}/{}", self.base_url, path);

        let response = self
            .http
            .get(url)
            .bearer_auth(key)
            .query(query)
            .send()
            .await
            .with_context(|| format!("Stripe GET failed for {}", path))?;

        parse_stripe_response(response, path).await
    }
}

async fn parse_stripe_response<T: DeserializeOwned>(
    response: reqwest::Response,
    path: &str,
) -> anyhow::Result<T> {
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("failed to read Stripe response body for {}", path))?;

    if !status.is_success() {
        return Err(anyhow!(
            "Stripe API {} failed with status {}: {}",
            path,
            status,
            text
        ));
    }

    serde_json::from_str::<T>(&text)
        .with_context(|| format!("failed to decode Stripe response for {}", path))
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeCustomer {
    pub id: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeCheckoutSession {
    pub url: Option<String>,
    pub status: Option<String>,
    pub subscription: Option<IdOrObject>,
    pub line_items: Option<StripeLineItems>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeBillingPortalSession {
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeSubscription {
    pub id: String,
    pub customer: IdOrObject,
    pub status: String,
    pub current_period_end: Option<i64>,
    pub items: StripeSubscriptionItems,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeSubscriptionItems {
    pub data: Vec<StripeSubscriptionItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeSubscriptionItem {
    pub price: Option<StripePrice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripePrice {
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeLineItems {
    pub data: Vec<StripeLineItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeLineItem {
    pub price: Option<StripePrice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdOrObject {
    Id(String),
    Object { id: String },
}

impl IdOrObject {
    pub fn id(&self) -> String {
        match self {
            IdOrObject::Id(value) => value.clone(),
            IdOrObject::Object { id } => id.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: StripeEventData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeEventData {
    pub object: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeInvoice {
    pub subscription: Option<IdOrObject>,
}
