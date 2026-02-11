import type { Request, Response } from "express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import { getClerkAuth } from "../lib/clerkAuth";
import { PLANS, isSubscriptionActive, resolvePlanId } from "../lib/plans";

export async function getUsage(req: Request, res: Response) {
  const auth = getClerkAuth(req);
  if (!auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const usageRecords = await convex.query(api.usage.getUsageData, {
      userId: auth.userId,
    });
    const reservationRecords = await convex.query(api.usage.getUsageReservations, {
      userId: auth.userId,
    });

    let totalUnits = 0;
    let unitsThisMonth = 0;
    let pendingUnits = 0;

    const currentMonth = new Date().toISOString().substring(0, 7); // YYYY-MM

    for (const record of usageRecords) {
      totalUnits += record.count;
      if (record.date.startsWith(currentMonth)) {
        unitsThisMonth += record.count;
      }
    }

    const now = Date.now();
    for (const reservation of reservationRecords) {
      if (
        reservation.status === "pending" &&
        reservation.date.startsWith(currentMonth) &&
        reservation.expiresAt > now
      ) {
        pendingUnits += reservation.units;
      }
    }

    const subscription = await convex.query(api.subscriptions.get, {
      userId: auth.userId,
    });
    const planId =
      subscription && isSubscriptionActive(subscription.status)
        ? resolvePlanId(subscription.plan)
        : "free";

    const monthlyQuota = PLANS[planId].monthlyUnits;
    const remainingUnits =
      monthlyQuota === null
        ? null
        : Math.max(monthlyQuota - unitsThisMonth - pendingUnits, 0);

    res.status(200).json({
      plan: planId,
      totalUnits,
      unitsThisMonth,
      pendingUnits,
      monthlyQuota,
      remainingUnits,
    });
  } catch (error) {
    console.error("Error fetching usage data:", error);
    res.status(500).send("Error fetching usage data");
  }
}
