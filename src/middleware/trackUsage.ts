import type { Response, NextFunction } from "express";
import { type WithAuthProp } from "@clerk/express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import { getClerkAuth } from "../lib/clerkAuth";

/**
 * This middleware tracks usage for a logged-in Clerk user.
 * It should be placed after the requireAuth() and syncUser middleware.
 */
export const trackUsage = (req: WithAuthProp<Request>, res: Response, next: NextFunction) => {
  const auth = getClerkAuth(req);
  if (auth.userId) {
    // Fire-and-forget the usage tracking action.
    // We don't await it so we don't slow down the user's request.
    // If it fails, it will be logged on the Convex side.
    convex.action(api.usage.incrementForClerkUser, { clerkId: auth.userId });
  }
  next();
};
