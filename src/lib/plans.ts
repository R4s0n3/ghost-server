export type PlanId = "free" | "starter" | "pro" | "business" | "enterprise";

type PlanDefinition = {
	id: PlanId;
	label: string;
	monthlyUnits: number | null;
	overagePerUnit: number | null;
	sla: "none" | "standard" | "priority";
};

export const PLANS: Record<PlanId, PlanDefinition> = {
	free: {
		id: "free",
		label: "Free",
		monthlyUnits: 400,
		overagePerUnit: null,
		sla: "none",
	},
	starter: {
		id: "starter",
		label: "Starter",
		monthlyUnits: 5_000,
		overagePerUnit: 0.004,
		sla: "standard",
	},
	pro: {
		id: "pro",
		label: "Pro",
		monthlyUnits: 25_000,
		overagePerUnit: 0.003,
		sla: "standard",
	},
	business: {
		id: "business",
		label: "Business",
		monthlyUnits: 100_000,
		overagePerUnit: 0.002,
		sla: "priority",
	},
	enterprise: {
		id: "enterprise",
		label: "Enterprise",
		monthlyUnits: null,
		overagePerUnit: null,
		sla: "priority",
	},
};

const PRICE_ID_BY_PLAN: Partial<Record<PlanId, string>> = {
	starter: process.env.STRIPE_PRICE_ID_STARTER,
	pro: process.env.STRIPE_PRICE_ID_PRO,
	business: process.env.STRIPE_PRICE_ID_BUSINESS,
	enterprise: process.env.STRIPE_PRICE_ID_ENTERPRISE,
};

const PRICE_ID_TO_PLAN = new Map<string, PlanId>(
	Object.entries(PRICE_ID_BY_PLAN)
		.filter(([, priceId]) => typeof priceId === "string" && priceId.trim().length > 0)
		.map(([planId, priceId]) => [priceId as string, planId as PlanId]),
);

export function getPlanForPriceId(priceId: string | null | undefined): PlanId | null {
	if (!priceId) {
		return null;
	}
	return PRICE_ID_TO_PLAN.get(priceId) ?? null;
}

export function getKnownPriceIds(): string[] {
	return Array.from(PRICE_ID_TO_PLAN.keys());
}

export function isSubscriptionActive(status: string | null | undefined): boolean {
	const normalized = (status ?? "").trim().toLowerCase();
	return normalized === "active" || normalized === "trialing";
}

export function resolvePlanId(plan: string | null | undefined): PlanId {
	const normalized = (plan ?? "").trim().toLowerCase();
	if (normalized in PLANS) {
		return normalized as PlanId;
	}
	return "free";
}
