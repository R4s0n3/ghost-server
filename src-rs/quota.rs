use anyhow::Context;
use serde::Deserialize;
use serde_json::json;

use crate::{
    convex::ConvexClient,
    plans::{is_subscription_active, plan_definition, resolve_plan_id, PlanId},
    serde_convex::{de_i64_from_number, de_opt_i64_from_number},
};

#[derive(Debug, Clone)]
pub struct QuotaReservation {
    pub allowed: bool,
    pub reservation_id: Option<String>,
    pub plan_id: PlanId,
    pub monthly_quota: Option<i64>,
    pub total_this_month: i64,
    pub pending_units: i64,
}

#[derive(Debug, Deserialize)]
struct SubscriptionRecord {
    pub plan: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReserveResult {
    pub allowed: bool,
    #[serde(rename = "reservationId")]
    pub reservation_id: Option<String>,
    #[serde(rename = "totalThisMonth")]
    #[serde(deserialize_with = "de_i64_from_number")]
    pub total_this_month: i64,
    #[serde(rename = "pendingUnits")]
    #[serde(default, deserialize_with = "de_opt_i64_from_number")]
    pub pending_units: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CommitReservationResult {
    pub committed: bool,
}

pub async fn reserve_units_for_clerk_user(
    convex: &ConvexClient,
    clerk_id: &str,
    units: i64,
) -> anyhow::Result<QuotaReservation> {
    let subscription: Option<SubscriptionRecord> = convex
        .query("subscriptions:get", json!({ "userId": clerk_id }))
        .await
        .context("failed to fetch subscription for quota reservation")?;

    let plan_id = match subscription {
        Some(subscription) if is_subscription_active(subscription.status.as_deref()) => {
            resolve_plan_id(subscription.plan.as_deref())
        }
        _ => PlanId::Free,
    };

    let monthly_quota = plan_definition(plan_id).monthly_units;

    let reserve_result: ReserveResult = convex
        .action(
            "usage:reserveForClerkUser",
            json!({
                "clerkId": clerk_id,
                "units": units,
                "monthlyQuota": monthly_quota,
            }),
        )
        .await
        .with_context(|| {
            format!(
                "failed to reserve usage units (clerk_id={}, units={})",
                clerk_id, units
            )
        })?;

    Ok(QuotaReservation {
        allowed: reserve_result.allowed,
        reservation_id: reserve_result.reservation_id,
        plan_id,
        monthly_quota,
        total_this_month: reserve_result.total_this_month,
        pending_units: reserve_result.pending_units.unwrap_or(0),
    })
}

pub async fn commit_reservation_for_clerk_user(
    convex: &ConvexClient,
    clerk_id: &str,
    reservation_id: &str,
) -> anyhow::Result<CommitReservationResult> {
    convex
        .action(
            "usage:commitReservationForClerkUser",
            json!({
                "clerkId": clerk_id,
                "reservationId": reservation_id,
            }),
        )
        .await
        .context("failed to commit usage reservation")
}

pub async fn release_reservation_for_clerk_user(
    convex: &ConvexClient,
    clerk_id: &str,
    reservation_id: &str,
) -> anyhow::Result<()> {
    let _value: serde_json::Value = convex
        .action(
            "usage:releaseReservationForClerkUser",
            json!({
                "clerkId": clerk_id,
                "reservationId": reservation_id,
            }),
        )
        .await
        .context("failed to release usage reservation")?;

    Ok(())
}
