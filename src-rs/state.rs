use std::{future::Future, sync::Arc, time::Instant};

use tokio::sync::Semaphore;

use crate::{
    auth::AuthService, clerk::ClerkClient, config::Config, convex::ConvexClient, plans::PriceMap,
    rate_limit::InMemoryRateLimiter, stripe_api::StripeApi,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub convex: ConvexClient,
    pub auth: AuthService,
    pub clerk: ClerkClient,
    pub stripe: StripeApi,
    pub price_map: PriceMap,
    pub ghostscript_semaphore: Arc<Semaphore>,
    pub preflight_test_limiter: Arc<InMemoryRateLimiter>,
    pub api_limiter: Arc<InMemoryRateLimiter>,
}

impl AppState {
    pub fn new(
        config: Config,
        convex: ConvexClient,
        auth: AuthService,
        clerk: ClerkClient,
        stripe: StripeApi,
    ) -> Self {
        let price_map = PriceMap::from_config(&config);
        Self {
            ghostscript_semaphore: Arc::new(Semaphore::new(config.ghostscript_concurrency)),
            preflight_test_limiter: Arc::new(InMemoryRateLimiter::new(
                std::time::Duration::from_secs(15 * 60),
                5,
            )),
            api_limiter: Arc::new(InMemoryRateLimiter::new(
                std::time::Duration::from_secs(15 * 60),
                100,
            )),
            config: Arc::new(config),
            convex,
            auth,
            clerk,
            stripe,
            price_map,
        }
    }

    pub async fn run_ghostscript_job<F, Fut, T>(
        &self,
        task_name: &str,
        task: F,
    ) -> anyhow::Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let enqueued_at = Instant::now();
        let permit = self
            .ghostscript_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("ghostscript queue closed"))?;
        let started_at = Instant::now();
        let wait_ms = started_at.duration_since(enqueued_at).as_millis();

        let result = task().await;

        let run_ms = Instant::now().duration_since(started_at).as_millis();
        drop(permit);

        if self.config.log_task_queue_timings {
            let available = self.ghostscript_semaphore.available_permits();
            let running = self
                .config
                .ghostscript_concurrency
                .saturating_sub(available);
            tracing::info!(
                queue = "ghostscript",
                task = task_name,
                wait_ms,
                run_ms,
                running,
                "queue timing"
            );
        }

        result
    }
}
