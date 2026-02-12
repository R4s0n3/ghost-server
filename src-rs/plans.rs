use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanId {
    Free,
    Starter,
    Pro,
    Business,
    Enterprise,
}

impl PlanId {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanId::Free => "free",
            PlanId::Starter => "starter",
            PlanId::Pro => "pro",
            PlanId::Business => "business",
            PlanId::Enterprise => "enterprise",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlanDefinition {
    pub monthly_units: Option<i64>,
}

pub fn plan_definition(plan_id: PlanId) -> PlanDefinition {
    match plan_id {
        PlanId::Free => PlanDefinition {
            monthly_units: Some(400),
        },
        PlanId::Starter => PlanDefinition {
            monthly_units: Some(5_000),
        },
        PlanId::Pro => PlanDefinition {
            monthly_units: Some(25_000),
        },
        PlanId::Business => PlanDefinition {
            monthly_units: Some(100_000),
        },
        PlanId::Enterprise => PlanDefinition {
            monthly_units: None,
        },
    }
}

pub fn resolve_plan_id(plan: Option<&str>) -> PlanId {
    match plan
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "starter" => PlanId::Starter,
        "pro" => PlanId::Pro,
        "business" => PlanId::Business,
        "enterprise" => PlanId::Enterprise,
        _ => PlanId::Free,
    }
}

pub fn is_subscription_active(status: Option<&str>) -> bool {
    matches!(
        status
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "active" | "trialing"
    )
}

#[derive(Clone, Debug)]
pub struct PriceMap {
    by_price_id: HashMap<String, PlanId>,
}

impl PriceMap {
    pub fn from_config(config: &Config) -> Self {
        let mut by_price_id = HashMap::new();
        insert_price(
            &mut by_price_id,
            config.stripe_price_id_starter.clone(),
            PlanId::Starter,
        );
        insert_price(
            &mut by_price_id,
            config.stripe_price_id_pro.clone(),
            PlanId::Pro,
        );
        insert_price(
            &mut by_price_id,
            config.stripe_price_id_business.clone(),
            PlanId::Business,
        );
        insert_price(
            &mut by_price_id,
            config.stripe_price_id_enterprise.clone(),
            PlanId::Enterprise,
        );
        Self { by_price_id }
    }

    pub fn get_plan_for_price_id(&self, price_id: Option<&str>) -> Option<PlanId> {
        let price_id = price_id?.trim();
        if price_id.is_empty() {
            return None;
        }
        self.by_price_id.get(price_id).copied()
    }
}

fn insert_price(map: &mut HashMap<String, PlanId>, price_id: Option<String>, plan_id: PlanId) {
    if let Some(price_id) = price_id.map(|v| v.trim().to_string()) {
        if !price_id.is_empty() {
            map.insert(price_id, plan_id);
        }
    }
}
