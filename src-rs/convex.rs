use anyhow::{anyhow, Context};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct ConvexClient {
    base_url: String,
    http: reqwest::Client,
}

const CONVEX_CLIENT_HEADER: &str = "npm-1.26.2";

impl ConvexClient {
    pub fn new(base_url: String) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "Convex-Client",
            HeaderValue::from_static(CONVEX_CLIENT_HEADER),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to create Convex HTTP client")?;

        Ok(Self { base_url, http })
    }

    pub async fn query<T: DeserializeOwned>(&self, path: &str, args: Value) -> anyhow::Result<T> {
        let value = self.call("query", path, args).await?;
        serde_json::from_value(value)
            .with_context(|| format!("failed to decode Convex query result for {path}"))
    }

    pub async fn query_value(&self, path: &str, args: Value) -> anyhow::Result<Value> {
        self.call("query", path, args).await
    }

    pub async fn action<T: DeserializeOwned>(&self, path: &str, args: Value) -> anyhow::Result<T> {
        let value = self.call("action", path, args).await?;
        serde_json::from_value(value)
            .with_context(|| format!("failed to decode Convex action result for {path}"))
    }

    pub async fn action_value(&self, path: &str, args: Value) -> anyhow::Result<Value> {
        self.call("action", path, args).await
    }

    async fn call(&self, kind: &str, path: &str, args: Value) -> anyhow::Result<Value> {
        let endpoint = format!("{}/api/{}", self.base_url.trim_end_matches('/'), kind);
        let mut args = args;
        prune_null_object_fields(&mut args);
        let body = json!({
            "path": path,
            "format": "convex_encoded_json",
            "args": [args],
        });

        let response = self
            .http
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Convex {} request failed for {} (base_url={})",
                    kind, path, self.base_url
                )
            })?;

        let status = response.status();
        let response_body: Value = response
            .json()
            .await
            .with_context(|| format!("failed to parse Convex {} response for {}", kind, path))?;

        if !status.is_success() && status.as_u16() != 560 {
            return Err(anyhow!(
                "Convex {} HTTP error {} for {}: {}",
                kind,
                status,
                path,
                response_body
            ));
        }

        match response_body.get("status").and_then(Value::as_str) {
            Some("success") => Ok(response_body.get("value").cloned().unwrap_or(Value::Null)),
            Some("error") => {
                let message = response_body
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .unwrap_or("Convex function error");
                Err(anyhow!("Convex {} {} failed: {}", kind, path, message))
            }
            _ => Err(anyhow!(
                "Invalid Convex {} response for {}: {}",
                kind,
                path,
                response_body
            )),
        }
    }
}

fn prune_null_object_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let null_keys: Vec<String> = map
                .iter()
                .filter_map(|(key, value)| {
                    if value.is_null() {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for key in null_keys {
                map.remove(&key);
            }
            for child in map.values_mut() {
                prune_null_object_fields(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                prune_null_object_fields(child);
            }
        }
        _ => {}
    }
}
