mod auth;
mod clerk;
mod config;
mod convex;
mod ghostscript;
mod handlers;
mod middleware;
mod plans;
mod quota;
mod rate_limit;
mod serde_convex;
mod state;
mod stripe_api;
mod upload;

use std::{collections::HashSet, env, net::SocketAddr, path::PathBuf};

use anyhow::Context;
use axum::{
    extract::DefaultBodyLimit,
    http::Method,
    middleware as axum_middleware,
    routing::{delete, get, post},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use serde_json::json;
use state::AppState;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let loaded_env_files = load_env_files()?;
    init_tracing();
    if loaded_env_files.is_empty() {
        tracing::warn!("No .env or .env.local file found. Using process environment only.");
    } else {
        let files = loaded_env_files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        tracing::info!(files = %files, "Loaded environment files");
    }

    let config = Config::from_env()?;

    if config.stripe_secret_key.is_none() {
        if env::var("NODE_ENV")
            .ok()
            .map(|value| value.eq_ignore_ascii_case("production"))
            .unwrap_or(false)
        {
            return Err(anyhow::anyhow!(
                "STRIPE_SECRET_KEY environment variable is not set"
            ));
        }

        tracing::warn!(
            "STRIPE_SECRET_KEY is not set. Stripe functionality will not work until it is provided."
        );
    }

    let convex = convex::ConvexClient::new(config.convex_url.clone())?;
    if config.clerk_issuer.is_none() {
        tracing::warn!(
            "CLERK_ISSUER is not set. JWT verification will accept any valid Clerk issuer."
        );
    }

    let auth = auth::AuthService::new(config.clerk_issuer.clone())?;
    let clerk = clerk::ClerkClient::new(
        config.clerk_api_base.clone(),
        config.clerk_secret_key.as_deref(),
    )?;
    let stripe = stripe_api::StripeApi::new(
        config.stripe_secret_key.clone(),
        config.stripe_webhook_secret.clone(),
    )?;

    let state = AppState::new(config.clone(), convex, auth, clerk, stripe);

    match state.convex.query::<String>("health:get", json!({})).await {
        Ok(value) => {
            tracing::info!(convex_health = %value, "Convex connectivity check passed");
        }
        Err(error) => {
            tracing::error!(
                error = ?error,
                convex_url = %config.convex_url,
                "Convex connectivity check failed. If using local Convex, run `bunx convex dev` and ensure CONVEX_URL matches that deployment."
            );
        }
    }

    let app = build_router(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    if let Some((cert_path, key_path)) = valid_tls_paths(&config) {
        let tls_config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .context("failed to load TLS certificate/key")?;

        tracing::info!(
            port = config.port,
            "TLS configuration loaded. Running in HTTPS mode."
        );

        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .context("HTTPS server failed")?;
    } else {
        tracing::info!(port = config.port, "Running in HTTP mode.");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .context("failed to bind TCP listener")?;

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .context("HTTP server failed")?;
    }

    Ok(())
}

fn build_router(state: AppState) -> Router {
    let process_public_router = Router::new().route(
        "/preflight-test",
        post(handlers::test_document).route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::preflight_test_rate_limit,
        )),
    );

    let process_private_router = Router::new()
        .route("/preflight", post(handlers::preflight_document))
        .route("/grayscale", post(handlers::convert_document_to_grayscale))
        .route("/conversion", get(handlers::conversion_placeholder))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_auth_and_sync,
        ));

    let process_router = Router::new()
        .merge(process_public_router)
        .merge(process_private_router);

    let api_key_router = Router::new()
        .route(
            "/",
            post(handlers::generate_api_key).get(handlers::list_api_keys),
        )
        .route("/{id}", delete(handlers::delete_api_key))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_auth_and_sync,
        ));

    let subscription_router = Router::new()
        .route("/", get(handlers::get_subscription))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_auth_and_sync,
        ));

    let stripe_router = Router::new()
        .route(
            "/create-checkout-session",
            post(handlers::create_checkout_session),
        )
        .route("/sync-session", post(handlers::sync_stripe_session))
        .route(
            "/create-customer-portal-session",
            post(handlers::create_customer_portal_session),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_auth_and_sync,
        ));

    let usage_router = Router::new()
        .route("/", get(handlers::get_usage))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_auth,
        ));

    let api_process_router = Router::new()
        .route("/analyze", post(handlers::process_document_api))
        .route(
            "/grayscale",
            post(handlers::convert_document_to_grayscale_api),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::api_key_auth,
        ));

    let api_router = Router::new()
        .nest("/keys", api_key_router)
        .nest("/subscription", subscription_router)
        .nest("/stripe", stripe_router)
        .nest("/usage", usage_router)
        .nest("/process", api_process_router)
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::api_rate_limit,
        ));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    Router::new()
        .route("/api/stripe/webhook", post(handlers::handle_stripe_webhook))
        .nest("/health", Router::new().route("/", get(handlers::health)))
        .nest("/process", process_router)
        .nest("/api", api_router)
        .fallback(handlers::not_found)
        .with_state(state)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

fn valid_tls_paths(config: &Config) -> Option<(String, String)> {
    let cert_path = config
        .tls_cert_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let key_path = config
        .tls_key_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    match (cert_path, key_path) {
        (Some(cert_path), Some(key_path)) => {
            let cert_exists = std::path::Path::new(&cert_path).exists();
            let key_exists = std::path::Path::new(&key_path).exists();

            if cert_exists && key_exists {
                Some((cert_path, key_path))
            } else {
                if !key_exists {
                    tracing::error!(path = %key_path, "TLS key file not found");
                }
                if !cert_exists {
                    tracing::error!(path = %cert_path, "TLS certificate file not found");
                }
                tracing::error!("Proceeding without TLS.");
                None
            }
        }
        (Some(cert_path), None) => {
            tracing::error!(path = %cert_path, "TLS certificate file provided but TLS key path missing");
            tracing::error!("Proceeding without TLS.");
            None
        }
        (None, Some(key_path)) => {
            tracing::error!(path = %key_path, "TLS key file provided but TLS certificate path missing");
            tracing::error!("Proceeding without TLS.");
            None
        }
        (None, None) => None,
    }
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

fn load_env_files() -> anyhow::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(executable_path) = env::current_exe() {
        if let Some(executable_dir) = executable_path.parent() {
            roots.push(executable_dir.to_path_buf());
        }
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    let mut seen_roots = HashSet::new();
    let mut loaded = Vec::new();

    for root in roots {
        let key = root.to_string_lossy().to_string();
        if !seen_roots.insert(key) {
            continue;
        }

        for filename in [".env", ".env.local"] {
            let path = root.join(filename);
            if path.is_file() {
                dotenvy::from_path(&path)
                    .with_context(|| format!("failed to load {}", path.display()))?;
                loaded.push(path);
            }
        }
    }

    if loaded.is_empty() {
        if let Ok(path) = dotenvy::dotenv() {
            loaded.push(path);
        }
    }

    Ok(loaded)
}
