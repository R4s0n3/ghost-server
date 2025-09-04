import type { Request, Response } from "express";
import { type WithAuthProp } from "@clerk/express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

export async function getSubscription(req: WithAuthProp<Request>, res: Response) {
  if (!req.auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const subscription = await convex.query(api.subscriptions.get, {
      userId: req.auth.userId,
    });

    if (!subscription) {
      // If the user has no subscription record, you might want to return a default
      // free plan status, or simply indicate no active subscription.
      return res.status(200).json({ plan: "free", status: "inactive" });
    }

    res.status(200).json(subscription);
  } catch (error) {
    console.error("Error fetching subscription:", error);
    res.status(500).send("Error fetching subscription");
  }
}
