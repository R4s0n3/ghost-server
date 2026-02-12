use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub trust_proxy: bool,
    pub tls_key_path: Option<PathBuf>,
    pub tls_cert_path: Option<PathBuf>,
    pub convex_url: String,
    pub clerk_secret_key: Option<String>,
    pub clerk_issuer: Option<String>,
    pub clerk_api_base: String,
    pub stripe_secret_key: Option<String>,
    pub stripe_webhook_secret: Option<String>,
    pub frontend_url: Option<String>,
    pub ghostscript_concurrency: usize,
    pub log_ghostscript_timings: bool,
    pub log_task_queue_timings: bool,
    pub stripe_price_id_starter: Option<String>,
    pub stripe_price_id_pro: Option<String>,
    pub stripe_price_id_business: Option<String>,
    pub stripe_price_id_enterprise: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let port = parse_u16(env::var("PORT").ok(), 9001);

        let trust_proxy = match env::var("TRUST_PROXY") {
            Ok(value) => {
                let normalized = value.trim().to_lowercase();
                !matches!(normalized.as_str(), "false" | "0" | "off" | "no")
            }
            Err(_) => true,
        };

        let convex_url = env::var("CONVEX_URL")
            .map_err(|_| anyhow::anyhow!("CONVEX_URL environment variable is not set"))?;
        let convex_url = normalize_convex_url(&convex_url);

        let ghostscript_concurrency = parse_usize(
            env::var("GHOSTSCRIPT_CONCURRENCY")
                .ok()
                .or_else(|| env::var("PROCESSING_CONCURRENCY").ok()),
            3,
        );

        Ok(Self {
            port,
            trust_proxy,
            tls_key_path: env::var("TLS_KEY_PATH").ok().map(PathBuf::from),
            tls_cert_path: env::var("TLS_CERT_PATH").ok().map(PathBuf::from),
            convex_url,
            clerk_secret_key: env::var("CLERK_SECRET_KEY").ok(),
            clerk_issuer: env::var("CLERK_ISSUER").ok(),
            clerk_api_base: env::var("CLERK_API_BASE")
                .unwrap_or_else(|_| "https://api.clerk.com/v1".to_string()),
            stripe_secret_key: env::var("STRIPE_SECRET_KEY").ok(),
            stripe_webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").ok(),
            frontend_url: env::var("FRONTEND_URL").ok(),
            ghostscript_concurrency,
            log_ghostscript_timings: env::var("LOG_GHOSTSCRIPT_TIMINGS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            log_task_queue_timings: env::var("LOG_TASK_QUEUE_TIMINGS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            stripe_price_id_starter: env::var("STRIPE_PRICE_ID_STARTER").ok(),
            stripe_price_id_pro: env::var("STRIPE_PRICE_ID_PRO").ok(),
            stripe_price_id_business: env::var("STRIPE_PRICE_ID_BUSINESS").ok(),
            stripe_price_id_enterprise: env::var("STRIPE_PRICE_ID_ENTERPRISE").ok(),
        })
    }
}

fn parse_u16(value: Option<String>, fallback: u16) -> u16 {
    value
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn parse_usize(value: Option<String>, fallback: usize) -> usize {
    value
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn normalize_convex_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix("wss://") {
        return format!("https://{}", stripped);
    }
    if let Some(stripped) = trimmed.strip_prefix("ws://") {
        return format!("http://{}", stripped);
    }
    trimmed.to_string()
}
