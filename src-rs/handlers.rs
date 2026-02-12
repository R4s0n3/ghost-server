use std::{path::Path, time::Instant};

use axum::{
    body::Bytes,
    extract::{Extension, Json, Multipart, Path as AxumPath, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ghostscript::{
        analyze_pdf, convert_pdf_to_grayscale_file, convert_pdf_to_grayscale_with_black_controls,
        get_pdf_page_count, sanitize_base_name,
    },
    middleware::{AuthenticatedUser, ConvexUser},
    plans::{is_subscription_active, plan_definition, resolve_plan_id, PlanId},
    quota::{
        commit_reservation_for_clerk_user, release_reservation_for_clerk_user,
        reserve_units_for_clerk_user, QuotaReservation,
    },
    serde_convex::de_i64_from_number,
    state::AppState,
    stripe_api::{StripeEvent, StripeInvoice, StripeSubscription},
    upload::{remove_file_if_exists, save_pdf_from_multipart, save_pdf_with_mode_from_multipart, UploadError},
};

#[derive(Debug, Deserialize)]
pub struct DeleteApiKeyPath {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    #[serde(rename = "priceId")]
    pub price_id: Option<String>,
    #[serde(rename = "successUrl")]
    pub success_url: Option<String>,
    #[serde(rename = "cancelUrl")]
    pub cancel_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyncStripeSessionRequest {
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConvexSubscription {
    pub plan: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConvexUsageRecord {
    pub date: String,
    #[serde(deserialize_with = "de_i64_from_number")]
    pub count: i64,
}

#[derive(Debug, Deserialize)]
struct ConvexUsageReservationRecord {
    pub date: String,
    pub status: String,
    #[serde(deserialize_with = "de_i64_from_number")]
    pub units: i64,
    #[serde(rename = "expiresAt")]
    #[serde(deserialize_with = "de_i64_from_number")]
    pub expires_at: i64,
}

#[derive(Debug, Deserialize, Clone)]
struct ConvexUserForStripe {
    #[serde(rename = "clerkId")]
    pub clerk_id: String,
    pub email: String,
    #[serde(rename = "stripeCustomerId")]
    pub stripe_customer_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct QuotaExceededBody {
    error: &'static str,
    plan: String,
    #[serde(rename = "monthlyQuota")]
    monthly_quota: Option<i64>,
    #[serde(rename = "unitsThisMonth")]
    units_this_month: i64,
    #[serde(rename = "pendingUnits")]
    pending_units: i64,
    #[serde(rename = "unitsRequested")]
    units_requested: i64,
}

pub async fn health(State(state): State<AppState>) -> Response {
    let (ghostscript_status, ghostscript_error) =
        match tokio::process::Command::new("gs").arg("-v").output().await {
            Ok(output) if output.status.success() => (
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
                None,
            ),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let message = if stderr.is_empty() {
                    format!("Exit status: {}", output.status)
                } else {
                    format!("Stderr: {}", stderr)
                };
                ("Not checked".to_string(), Some(message))
            }
            Err(error) => (
                "Not checked".to_string(),
                Some(format!("Failed to execute gs -v: {}", error)),
            ),
        };

    match state.convex.query::<String>("health:get", json!({})).await {
        Ok(convex_health) => {
            let suffix = ghostscript_error
                .map(|value| format!(" (Error: {})", value))
                .unwrap_or_default();
            (
                StatusCode::OK,
                format!(
                    "Express server is online. Convex status: \"{}\". Ghostscript status: {}{}",
                    convex_health, ghostscript_status, suffix
                ),
            )
                .into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to connect to Convex");
            let suffix = ghostscript_error
                .map(|value| format!(" (Error: {})", value))
                .unwrap_or_default();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to connect to Convex. Ghostscript status: {}{}",
                    ghostscript_status, suffix
                ),
            )
                .into_response()
        }
    }
}

pub async fn conversion_placeholder() -> Response {
    (StatusCode::OK, "conversion").into_response()
}

pub async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

pub async fn test_document(State(state): State<AppState>, multipart: Multipart) -> Response {
    let uploaded = match save_pdf_from_multipart(multipart, 5 * 1024 * 1024).await {
        Ok(file) => file,
        Err(error) => return upload_error_to_response(error),
    };

    let temp_path = uploaded.temp_path.clone();
    let original_name = uploaded.original_name.clone();

    let result = state
        .run_ghostscript_job("preflight-test", || async {
            let mut analysis = analyze_pdf(&temp_path, None).await?;
            analysis.file_name = original_name;
            Ok(analysis)
        })
        .await;

    remove_file_if_exists(&temp_path).await;

    match result {
        Ok(analysis) => Json(analysis).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to analyze PDF");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

pub async fn preflight_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    multipart: Multipart,
) -> Response {
    preflight_for_clerk_user(state, &user.clerk_id, multipart, 5 * 1024 * 1024).await
}

pub async fn process_document_api(
    State(state): State<AppState>,
    Extension(convex_user): Extension<ConvexUser>,
    multipart: Multipart,
) -> Response {
    let clerk_id = match convex_user.clerk_id {
        Some(value) if !value.trim().is_empty() => value,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Authenticated user missing Clerk ID.",
            )
                .into_response()
        }
    };

    preflight_for_clerk_user(state, &clerk_id, multipart, 20 * 1024 * 1024).await
}

pub async fn convert_document_to_grayscale(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    multipart: Multipart,
) -> Response {
    grayscale_for_clerk_user(state, &user.clerk_id, multipart).await
}

pub async fn convert_document_to_grayscale_api(
    State(state): State<AppState>,
    Extension(convex_user): Extension<ConvexUser>,
    multipart: Multipart,
) -> Response {
    let clerk_id = match convex_user.clerk_id {
        Some(value) if !value.trim().is_empty() => value,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Authenticated user missing Clerk ID.",
            )
                .into_response()
        }
    };

    grayscale_for_clerk_user(state, &clerk_id, multipart).await
}

pub async fn generate_api_key(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    match state
        .convex
        .action_value("apiKeys:generate", json!({ "userId": &user.clerk_id }))
        .await
    {
        Ok(api_key) => (StatusCode::CREATED, Json(json!({ "apiKey": api_key }))).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to generate API key");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error generating API key",
            )
                .into_response()
        }
    }
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    match state
        .convex
        .query_value("apiKeys:list", json!({ "userId": &user.clerk_id }))
        .await
    {
        Ok(keys) => (StatusCode::OK, Json(keys)).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to list API keys");
            (StatusCode::INTERNAL_SERVER_ERROR, "Error listing API keys").into_response()
        }
    }
}

pub async fn delete_api_key(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    AxumPath(path): AxumPath<DeleteApiKeyPath>,
) -> Response {
    if path.id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing API key ID.").into_response();
    }

    match state
        .convex
        .action_value(
            "apiKeys:deleteApiKey",
            json!({
                "clerkId": &user.clerk_id,
                "apiKeyId": path.id,
            }),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "message": "API key deleted successfully." })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to delete API key");
            (StatusCode::INTERNAL_SERVER_ERROR, "Error deleting API key.").into_response()
        }
    }
}

pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let subscription = state
        .convex
        .query_value("subscriptions:get", json!({ "userId": &user.clerk_id }))
        .await;

    match subscription {
        Ok(value) => {
            if value.is_null() {
                (
                    StatusCode::OK,
                    Json(json!({ "plan": "free", "status": "inactive" })),
                )
                    .into_response()
            } else {
                (StatusCode::OK, Json(value)).into_response()
            }
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch subscription");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error fetching subscription",
            )
                .into_response()
        }
    }
}

pub async fn get_usage(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let usage_records: Vec<ConvexUsageRecord> = match state
        .convex
        .query("usage:getUsageData", json!({ "userId": &user.clerk_id }))
        .await
    {
        Ok(records) => records,
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch usage records");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error fetching usage data",
            )
                .into_response();
        }
    };

    let reservation_records: Vec<ConvexUsageReservationRecord> = match state
        .convex
        .query(
            "usage:getUsageReservations",
            json!({ "userId": &user.clerk_id }),
        )
        .await
    {
        Ok(records) => records,
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch usage reservations");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error fetching usage data",
            )
                .into_response();
        }
    };

    let current_month = Utc::now().format("%Y-%m").to_string();

    let mut total_units = 0i64;
    let mut units_this_month = 0i64;
    for record in usage_records {
        total_units += record.count;
        if record.date.starts_with(&current_month) {
            units_this_month += record.count;
        }
    }

    let now = Utc::now().timestamp_millis();
    let mut pending_units = 0i64;
    for reservation in reservation_records {
        if reservation.status == "pending"
            && reservation.date.starts_with(&current_month)
            && reservation.expires_at > now
        {
            pending_units += reservation.units;
        }
    }

    let subscription: Option<ConvexSubscription> = match state
        .convex
        .query("subscriptions:get", json!({ "userId": &user.clerk_id }))
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch subscription for usage");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error fetching usage data",
            )
                .into_response();
        }
    };

    let plan_id = match subscription {
        Some(subscription) if is_subscription_active(subscription.status.as_deref()) => {
            resolve_plan_id(subscription.plan.as_deref())
        }
        _ => PlanId::Free,
    };

    let monthly_quota = plan_definition(plan_id).monthly_units;
    let remaining_units =
        monthly_quota.map(|quota| (quota - units_this_month - pending_units).max(0));

    (
        StatusCode::OK,
        Json(json!({
            "plan": plan_id.as_str(),
            "totalUnits": total_units,
            "unitsThisMonth": units_this_month,
            "pendingUnits": pending_units,
            "monthlyQuota": monthly_quota,
            "remainingUnits": remaining_units,
        })),
    )
        .into_response()
}

pub async fn create_checkout_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<CreateCheckoutRequest>,
) -> Response {
    let price_id = match body.price_id.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing required parameters: priceId, successUrl, cancelUrl",
            )
                .into_response();
        }
    };
    let success_url = match body.success_url.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing required parameters: priceId, successUrl, cancelUrl",
            )
                .into_response();
        }
    };
    let cancel_url = match body.cancel_url.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing required parameters: priceId, successUrl, cancelUrl",
            )
                .into_response();
        }
    };

    if state
        .price_map
        .get_plan_for_price_id(Some(price_id.as_str()))
        .is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            "Unknown or unsupported Stripe price ID.",
        )
            .into_response();
    }

    let user_for_stripe: Option<ConvexUserForStripe> = match state
        .convex
        .action(
            "users:getUserForStripe",
            json!({ "clerkId": &user.clerk_id }),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to load user for Stripe checkout");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error creating checkout session",
            )
                .into_response();
        }
    };

    let mut user_for_stripe = match user_for_stripe {
        Some(value) => value,
        None => {
            return (StatusCode::NOT_FOUND, "User not found in Convex database.").into_response()
        }
    };

    let stripe_customer_id = if let Some(customer_id) = user_for_stripe.stripe_customer_id.clone() {
        customer_id
    } else {
        let customer = match state
            .stripe
            .create_customer(&user_for_stripe.email, &user_for_stripe.clerk_id)
            .await
        {
            Ok(customer) => customer,
            Err(error) => {
                tracing::error!(error = %error, "failed to create Stripe customer");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Error creating checkout session",
                )
                    .into_response();
            }
        };

        if let Err(error) = state
            .convex
            .action_value(
                "users:setStripeCustomerId",
                json!({
                    "clerkId": &user_for_stripe.clerk_id,
                    "stripeCustomerId": &customer.id,
                }),
            )
            .await
        {
            tracing::error!(error = %error, "failed to persist Stripe customer id");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error creating checkout session",
            )
                .into_response();
        }

        user_for_stripe.stripe_customer_id = Some(customer.id.clone());
        customer.id
    };

    let session = match state
        .stripe
        .create_checkout_session(&stripe_customer_id, &price_id, &success_url, &cancel_url)
        .await
    {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(error = %error, "failed to create Stripe checkout session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error creating checkout session",
            )
                .into_response();
        }
    };

    match session.url {
        Some(url) => (StatusCode::OK, Json(json!({ "url": url }))).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error creating Stripe checkout session.",
        )
            .into_response(),
    }
}

pub async fn sync_stripe_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<SyncStripeSessionRequest>,
) -> Response {
    let session_id = match body.session_id.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => return (StatusCode::BAD_REQUEST, "Missing sessionId").into_response(),
    };

    let session = match state.stripe.retrieve_checkout_session(&session_id).await {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(error = %error, "failed to retrieve Stripe checkout session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error syncing Stripe session",
            )
                .into_response();
        }
    };

    if session.status.as_deref() != Some("complete") {
        return (StatusCode::BAD_REQUEST, "Checkout session not complete.").into_response();
    }

    let subscription_id = session.subscription.map(|value| value.id());
    let price_id = session
        .line_items
        .as_ref()
        .and_then(|line_items| line_items.data.first())
        .and_then(|item| item.price.as_ref())
        .and_then(|price| price.id.clone());

    let (subscription_id, price_id) = match (subscription_id, price_id) {
        (Some(subscription_id), Some(price_id)) => (subscription_id, price_id),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Could not find subscription or price ID in session.",
            )
                .into_response()
        }
    };

    let plan_id = match state
        .price_map
        .get_plan_for_price_id(Some(price_id.as_str()))
    {
        Some(plan_id) => plan_id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Unknown or unsupported Stripe price ID.",
            )
                .into_response()
        }
    };

    let user_exists: Option<ConvexUserForStripe> = match state
        .convex
        .action(
            "users:getUserForStripe",
            json!({ "clerkId": &user.clerk_id }),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch user for Stripe sync");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error syncing Stripe session",
            )
                .into_response();
        }
    };

    if user_exists.is_none() {
        return (StatusCode::NOT_FOUND, "User not found.").into_response();
    }

    let existing_subscription: Option<ConvexSubscription> = match state
        .convex
        .query("subscriptions:get", json!({ "userId": &user.clerk_id }))
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to fetch existing subscription");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error syncing Stripe session",
            )
                .into_response();
        }
    };

    let action_name = if existing_subscription.is_some() {
        "subscriptions:updateSubscription"
    } else {
        "subscriptions:createSubscription"
    };

    if let Err(error) = state
        .convex
        .action_value(
            action_name,
            json!({
                "userId": &user.clerk_id,
                "plan": plan_id.as_str(),
                "status": "active",
                "stripeSubscriptionId": subscription_id,
                "stripePriceId": price_id,
            }),
        )
        .await
    {
        tracing::error!(error = %error, "failed to sync subscription in Convex");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error syncing Stripe session",
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({ "message": "Subscription synced successfully." })),
    )
        .into_response()
}

pub async fn create_customer_portal_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let user_for_stripe: Option<ConvexUserForStripe> = match state
        .convex
        .action(
            "users:getUserForStripe",
            json!({ "clerkId": &user.clerk_id }),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to load user for portal session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error creating customer portal session",
            )
                .into_response();
        }
    };

    let user_for_stripe = match user_for_stripe {
        Some(user) => user,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "User or Stripe Customer ID not found.",
            )
                .into_response()
        }
    };

    let stripe_customer_id = match user_for_stripe.stripe_customer_id {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "User or Stripe Customer ID not found.",
            )
                .into_response()
        }
    };

    let return_url = format!(
        "{}/dashboard",
        state
            .config
            .frontend_url
            .clone()
            .unwrap_or_else(|| "".to_string())
            .trim_end_matches('/')
    );

    let session = match state
        .stripe
        .create_billing_portal_session(&stripe_customer_id, &return_url)
        .await
    {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(error = %error, "failed to create Stripe portal session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error creating customer portal session",
            )
                .into_response();
        }
    };

    match session.url {
        Some(url) => (StatusCode::OK, Json(json!({ "url": url }))).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error creating Stripe customer portal session.",
        )
            .into_response(),
    }
}

pub async fn handle_stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = match headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value,
        None => return (StatusCode::BAD_REQUEST, "Missing Stripe signature.").into_response(),
    };

    if let Err(error) = state.stripe.verify_webhook_signature(signature, &body) {
        tracing::error!(error = %error, "Stripe webhook signature verification failed");
        let message = error.to_string();
        if message.contains("STRIPE_WEBHOOK_SECRET") {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Webhook not configured.").into_response();
        }
        return (StatusCode::BAD_REQUEST, "Invalid signature.").into_response();
    }

    let event: StripeEvent = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(error) => {
            tracing::error!(error = %error, "invalid Stripe webhook payload");
            return (StatusCode::BAD_REQUEST, "Invalid signature.").into_response();
        }
    };

    let result = match event.event_type.as_str() {
        "customer.subscription.created"
        | "customer.subscription.updated"
        | "customer.subscription.deleted" => {
            let subscription: StripeSubscription = match serde_json::from_value(event.data.object) {
                Ok(value) => value,
                Err(error) => {
                    tracing::error!(error = %error, "failed to decode subscription object");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Webhook handler failed.")
                        .into_response();
                }
            };
            sync_subscription_from_stripe(&state, subscription).await
        }
        "invoice.payment_failed" | "invoice.payment_succeeded" => {
            let invoice: StripeInvoice = match serde_json::from_value(event.data.object) {
                Ok(value) => value,
                Err(error) => {
                    tracing::error!(error = %error, "failed to decode invoice object");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Webhook handler failed.")
                        .into_response();
                }
            };

            if let Some(subscription_ref) = invoice.subscription {
                let subscription_id = subscription_ref.id();
                match state.stripe.retrieve_subscription(&subscription_id).await {
                    Ok(subscription) => sync_subscription_from_stripe(&state, subscription).await,
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    };

    match result {
        Ok(_) => (StatusCode::OK, Json(json!({ "received": true }))).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "Stripe webhook handling failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Webhook handler failed.").into_response()
        }
    }
}

async fn sync_subscription_from_stripe(
    state: &AppState,
    subscription: StripeSubscription,
) -> anyhow::Result<()> {
    let customer_id = subscription.customer.id();

    let clerk_id = get_clerk_id_for_customer(state, &customer_id).await?;
    let clerk_id = match clerk_id {
        Some(value) => value,
        None => {
            tracing::warn!(customer_id = %customer_id, "Stripe webhook: missing clerkId metadata for customer");
            return Ok(());
        }
    };

    let price_id = subscription
        .items
        .data
        .first()
        .and_then(|item| item.price.as_ref())
        .and_then(|price| price.id.clone());

    let existing_subscription: Option<ConvexSubscription> = state
        .convex
        .query("subscriptions:get", json!({ "userId": &clerk_id }))
        .await?;

    let plan_from_price = state.price_map.get_plan_for_price_id(price_id.as_deref());
    let plan_id = match (plan_from_price, existing_subscription.as_ref()) {
        (Some(plan_id), _) => Some(plan_id),
        (None, Some(subscription)) => Some(resolve_plan_id(subscription.plan.as_deref())),
        (None, None) => None,
    };

    let plan_id = match plan_id {
        Some(value) => value,
        None => {
            tracing::warn!(price_id = ?price_id, "Stripe webhook: unable to resolve plan for price");
            return Ok(());
        }
    };

    let ends_at = subscription
        .current_period_end
        .map(|seconds| seconds * 1000);

    let action_name = if existing_subscription.is_some() {
        "subscriptions:updateSubscription"
    } else {
        "subscriptions:createSubscription"
    };

    state
        .convex
        .action_value(
            action_name,
            json!({
                "userId": &clerk_id,
                "plan": plan_id.as_str(),
                "status": subscription.status,
                "stripeSubscriptionId": subscription.id,
                "stripePriceId": price_id,
                "endsAt": ends_at,
            }),
        )
        .await?;

    Ok(())
}

async fn get_clerk_id_for_customer(
    state: &AppState,
    customer_id: &str,
) -> anyhow::Result<Option<String>> {
    let customer = state.stripe.retrieve_customer(customer_id).await?;
    if customer.deleted {
        return Ok(None);
    }
    Ok(customer.metadata.get("clerkId").cloned())
}

async fn preflight_for_clerk_user(
    state: AppState,
    clerk_id: &str,
    multipart: Multipart,
    max_upload_size_bytes: usize,
) -> Response {
    let uploaded = match save_pdf_from_multipart(multipart, max_upload_size_bytes).await {
        Ok(file) => file,
        Err(error) => return upload_error_to_response(error),
    };

    let temp_path = uploaded.temp_path.clone();
    let original_name = uploaded.original_name.clone();
    let clerk_id = clerk_id.to_string();

    let result = state
        .run_ghostscript_job("preflight", || async {
            let page_count = get_pdf_page_count(&temp_path).await?;
            let units = page_count * 2;
            let reservation = reserve_units_for_clerk_user(&state.convex, &clerk_id, units).await?;
            if !reservation.allowed {
                return Ok(PreflightOutcome::QuotaExceeded { reservation, units });
            }

            let reservation_id = reservation
                .reservation_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Failed to create usage reservation."))?;

            let mut analysis_result = analyze_pdf(&temp_path, Some(page_count)).await;
            match analysis_result.as_mut() {
                Ok(analysis) => {
                    let commit_result = commit_reservation_for_clerk_user(
                        &state.convex,
                        &clerk_id,
                        &reservation_id,
                    )
                    .await?;
                    if !commit_result.committed {
                        tracing::warn!("Usage reservation commit failed");
                    }

                    analysis.file_name = original_name;
                    Ok(PreflightOutcome::Analysis {
                        analysis: analysis.clone(),
                    })
                }
                Err(error) => {
                    let _ = release_reservation_for_clerk_user(
                        &state.convex,
                        &clerk_id,
                        &reservation_id,
                    )
                    .await;
                    Err(anyhow::anyhow!(error.to_string()))
                }
            }
        })
        .await;

    remove_file_if_exists(&temp_path).await;

    match result {
        Ok(PreflightOutcome::Analysis { analysis }) => Json(analysis).into_response(),
        Ok(PreflightOutcome::QuotaExceeded { reservation, units }) => {
            quota_exceeded_response(reservation, units)
        }
        Err(error) => {
            tracing::error!(error = ?error, "preflight failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum GrayscaleMode {
    Preview,
    Production,
}

impl GrayscaleMode {
    fn parse(raw: Option<&str>) -> Result<Self, &'static str> {
        let normalized = raw
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if normalized.is_empty() || normalized == "preview" {
            return Ok(Self::Preview);
        }
        if normalized == "production" {
            return Ok(Self::Production);
        }
        Err("Invalid mode. Use \"preview\" or \"production\".")
    }
}

async fn grayscale_for_clerk_user(
    state: AppState,
    clerk_id: &str,
    multipart: Multipart,
) -> Response {
    let total_started = Instant::now();

    let upload_started = Instant::now();
    let uploaded = match save_pdf_with_mode_from_multipart(multipart, 20 * 1024 * 1024).await {
        Ok(file) => file,
        Err(error) => return upload_error_to_response(error),
    };
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-upload",
        upload_started,
    );

    let temp_path = uploaded.temp_path.clone();
    let original_name = uploaded.original_name;
    let mode = match GrayscaleMode::parse(uploaded.mode.as_deref()) {
        Ok(value) => value,
        Err(message) => {
            remove_file_if_exists(&temp_path).await;
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response();
        }
    };
    let force_black_text = state.config.grayscale_production_force_black_text;
    let force_black_vector = state.config.grayscale_production_force_black_vector;
    let black_threshold_l = state.config.grayscale_production_black_threshold_l;
    let black_threshold_c = state.config.grayscale_production_black_threshold_c;

    let base_name = sanitize_base_name(
        Path::new(&original_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document"),
    );
    let output_name = format!("{}-grayscale.pdf", base_name);
    let output_path =
        std::env::temp_dir().join(format!("{}-{}-grayscale.pdf", base_name, Uuid::new_v4()));

    let clerk_id = clerk_id.to_string();

    let page_count_started = Instant::now();
    let page_count = match state
        .run_ghostscript_job("grayscale-page-count", || async {
            get_pdf_page_count(&temp_path).await
        })
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "failed to get page count for grayscale");
            remove_file_if_exists(&temp_path).await;
            remove_file_if_exists(&output_path).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    maybe_log_ghostscript_timing(
        state.config.log_ghostscript_timings,
        "page-count",
        page_count_started,
    );
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-page-count",
        page_count_started,
    );

    let units = page_count;
    let reserve_started = Instant::now();
    let reservation = match reserve_units_for_clerk_user(&state.convex, &clerk_id, units).await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = ?error, "failed to reserve quota for grayscale");
            remove_file_if_exists(&temp_path).await;
            remove_file_if_exists(&output_path).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to reserve usage quota." })),
            )
                .into_response();
        }
    };
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-reserve",
        reserve_started,
    );

    if !reservation.allowed {
        remove_file_if_exists(&temp_path).await;
        remove_file_if_exists(&output_path).await;
        return quota_exceeded_response(reservation, units);
    }

    let reservation_id = match reservation.reservation_id.clone() {
        Some(value) => value,
        None => {
            remove_file_if_exists(&temp_path).await;
            remove_file_if_exists(&output_path).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to create usage reservation." })),
            )
                .into_response();
        }
    };

    let conversion_started = Instant::now();
    let conversion_result = state
        .run_ghostscript_job("grayscale-conversion", || async {
            match mode {
                GrayscaleMode::Preview => {
                    convert_pdf_to_grayscale_file(&temp_path, &output_path).await
                }
                GrayscaleMode::Production => {
                    convert_pdf_to_grayscale_with_black_controls(
                        &temp_path,
                        &output_path,
                        force_black_text,
                        force_black_vector,
                        black_threshold_l,
                        black_threshold_c,
                    )
                    .await
                }
            }
        })
        .await;

    if let Err(error) = conversion_result {
        let _ = release_reservation_for_clerk_user(&state.convex, &clerk_id, &reservation_id).await;
        tracing::error!(error = %error, "grayscale conversion failed");
        remove_file_if_exists(&temp_path).await;
        remove_file_if_exists(&output_path).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response();
    }

    maybe_log_ghostscript_timing(
        state.config.log_ghostscript_timings,
        "grayscale-conversion",
        conversion_started,
    );
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-conversion",
        conversion_started,
    );

    let commit_started = Instant::now();
    match commit_reservation_for_clerk_user(&state.convex, &clerk_id, &reservation_id).await {
        Ok(result) => {
            if !result.committed {
                tracing::warn!("Usage reservation commit failed");
            }
        }
        Err(error) => {
            tracing::warn!(error = %error, "failed to commit reservation");
        }
    }
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-commit",
        commit_started,
    );

    let read_started = Instant::now();
    let pdf_bytes = match tokio::fs::read(&output_path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(error = %error, "failed to read grayscale output");
            remove_file_if_exists(&temp_path).await;
            remove_file_if_exists(&output_path).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to send grayscale PDF" })),
            )
                .into_response();
        }
    };
    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-read",
        read_started,
    );

    remove_file_if_exists(&temp_path).await;
    remove_file_if_exists(&output_path).await;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    if let Ok(content_disposition) = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        sanitize_filename_for_header(&output_name)
    )) {
        headers.insert(CONTENT_DISPOSITION, content_disposition);
    }

    maybe_log_processing_timing(
        state.config.log_processing_timings,
        "grayscale-total",
        total_started,
    );

    (StatusCode::OK, headers, pdf_bytes).into_response()
}

fn maybe_log_ghostscript_timing(enabled: bool, stage: &str, started_at: Instant) {
    if !enabled {
        return;
    }
    let duration_ms = Instant::now().duration_since(started_at).as_millis();
    tracing::info!(stage = stage, duration_ms, "ghostscript timing");
}

fn maybe_log_processing_timing(enabled: bool, stage: &str, started_at: Instant) {
    if !enabled {
        return;
    }
    let duration_ms = Instant::now().duration_since(started_at).as_millis();
    tracing::info!(stage = stage, duration_ms, "processing timing");
}

fn sanitize_filename_for_header(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn upload_error_to_response(error: UploadError) -> Response {
    match error {
        UploadError::MissingFile => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "File not found" })),
        )
            .into_response(),
        UploadError::UnsupportedFileType => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Only PDF files are supported" })),
        )
            .into_response(),
        UploadError::FileTooLarge => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "File exceeds upload limit" })),
        )
            .into_response(),
        UploadError::MultipartError | UploadError::IoError => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to parse upload" })),
        )
            .into_response(),
    }
}

fn quota_exceeded_response(reservation: QuotaReservation, units: i64) -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(QuotaExceededBody {
            error: "Monthly quota exceeded.",
            plan: reservation.plan_id.as_str().to_string(),
            monthly_quota: reservation.monthly_quota,
            units_this_month: reservation.total_this_month,
            pending_units: reservation.pending_units,
            units_requested: units,
        }),
    )
        .into_response()
}

enum PreflightOutcome {
    Analysis {
        analysis: crate::ghostscript::PdfAnalysis,
    },
    QuotaExceeded {
        reservation: QuotaReservation,
        units: i64,
    },
}
