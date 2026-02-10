import { convex } from "./convex";
import { api } from "../../convex/_generated/api";
import { PLANS, PlanId, isSubscriptionActive, resolvePlanId } from "./plans";

export type QuotaReservation = {
	allowed: boolean;
	reservationId: string | null;
	planId: PlanId;
	monthlyQuota: number | null;
	totalThisMonth: number;
	pendingUnits: number;
};

export async function reserveUnitsForClerkUser(
	clerkId: string,
	units: number,
): Promise<QuotaReservation> {
	const subscription = await convex.query(api.subscriptions.get, { userId: clerkId });
	const planId =
		subscription && isSubscriptionActive(subscription.status)
			? resolvePlanId(subscription.plan)
			: "free";

	const monthlyQuota = PLANS[planId].monthlyUnits;
	const reservation = await convex.action(api.usage.reserveForClerkUser, {
		clerkId,
		units,
		monthlyQuota: monthlyQuota ?? undefined,
	});

	return {
		allowed: reservation.allowed,
		reservationId: reservation.reservationId ?? null,
		planId,
		monthlyQuota,
		totalThisMonth: reservation.totalThisMonth,
		pendingUnits: reservation.pendingUnits ?? 0,
	};
}

export async function commitReservationForClerkUser(
	clerkId: string,
	reservationId: string,
) {
	return convex.action(api.usage.commitReservationForClerkUser, {
		clerkId,
		reservationId,
	});
}

export async function releaseReservationForClerkUser(
	clerkId: string,
	reservationId: string,
) {
	return convex.action(api.usage.releaseReservationForClerkUser, {
		clerkId,
		reservationId,
	});
}
