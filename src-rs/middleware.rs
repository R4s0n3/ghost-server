use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::connect_info::ConnectInfo,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub clerk_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexUser {
    #[serde(rename = "clerkId")]
    pub clerk_id: Option<String>,
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_header = match request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let claims = match state.auth.verify_bearer_token(auth_header).await {
        Ok(claims) => claims,
        Err(error) => {
            tracing::warn!(error = %error, "authorization failed");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    };

    request.extensions_mut().insert(AuthenticatedUser {
        clerk_id: claims.sub,
    });

    next.run(request).await
}

pub async fn require_auth_and_sync(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_header = match request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let claims = match state.auth.verify_bearer_token(auth_header).await {
        Ok(claims) => claims,
        Err(error) => {
            tracing::warn!(error = %error, "authorization failed");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    };

    let clerk_id = claims.sub;

    if state.config.clerk_secret_key.is_some() {
        match state.clerk.get_primary_email(&clerk_id).await {
            Ok(Some(email)) => {
                if let Err(error) = state
                    .convex
                    .action_value("users:sync", json!({ "clerkId": clerk_id, "email": email }))
                    .await
                {
                    tracing::error!(error = %error, "failed to sync user to Convex");
                }
            }
            Ok(None) => {
                tracing::warn!(user_id = %clerk_id, "user has no primary email in Clerk");
            }
            Err(error) => {
                tracing::error!(error = %error, user_id = %clerk_id, "failed to load Clerk user");
            }
        }
    }

    request
        .extensions_mut()
        .insert(AuthenticatedUser { clerk_id });

    next.run(request).await
}

pub async fn api_key_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let api_key = match request
        .headers()
        .get("X-API-Key")
        .or_else(|| request.headers().get("x-api-key"))
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if !value.trim().is_empty() => value,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: API Key is required.",
            )
                .into_response()
        }
    };

    let user_value = match state
        .convex
        .action_value(
            "apiKeys:authenticateAndTrackUsage",
            json!({ "key": api_key }),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "API key authentication failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response();
        }
    };

    if user_value.is_null() {
        return (StatusCode::UNAUTHORIZED, "Unauthorized: Invalid API Key.").into_response();
    }

    let user: ConvexUser = match serde_json::from_value(user_value) {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to decode Convex user from API key auth");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response();
        }
    };

    request.extensions_mut().insert(user);

    next.run(request).await
}

pub async fn preflight_test_rate_limit(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let socket_addr = request
        .extensions()
        .get::<SocketAddr>()
        .copied()
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|value| value.0)
        });
    let key = client_identity(request.headers(), socket_addr, state.config.trust_proxy);

    if !state.preflight_test_limiter.check_and_count(&key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests from this IP, please try again after 15 minutes",
        )
            .into_response();
    }

    next.run(request).await
}

pub async fn api_rate_limit(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let socket_addr = request
        .extensions()
        .get::<SocketAddr>()
        .copied()
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|value| value.0)
        });
    let key = client_identity(request.headers(), socket_addr, state.config.trust_proxy);

    if !state.api_limiter.check_and_count(&key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests from this IP, please try again after 15 minutes",
        )
            .into_response();
    }

    next.run(request).await
}

fn client_identity(
    headers: &HeaderMap,
    socket_addr: Option<SocketAddr>,
    trust_proxy: bool,
) -> String {
    if trust_proxy {
        if let Some(value) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = value.split(',').next() {
                let candidate = first.trim();
                if !candidate.is_empty() {
                    return candidate.to_string();
                }
            }
        }

        if let Some(value) = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
        {
            let candidate = value.trim();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }

    socket_addr
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
