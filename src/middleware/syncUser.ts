import type { Request, Response, NextFunction } from "express";
import { clerkClient, type WithAuthProp } from "@clerk/express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

export const syncUser = async (
  req: WithAuthProp<Request>,
  res: Response,
  next: NextFunction
) => {
  if (!req.auth?.userId) {
    return next(
      new Error("User not authenticated. This middleware should be used after requireAuth.")
    );
  }

  try {
    const user = await clerkClient.users.getUser(req.auth.userId);
    const primaryEmail =
      user.emailAddresses.find((e) => e.id === user.primaryEmailAddressId)
        ?.emailAddress;

    if (!primaryEmail) {
      // This case should be rare if users are required to have an email.
      console.warn(`User ${req.auth.userId} has no primary email address.`);
      return next();
    }

    // This action is idempotent, so it's safe to call on every request.
    // It will only write to the database if the user is new or their email has changed.
    await convex.action(api.users.sync, {
      clerkId: req.auth.userId,
      email: primaryEmail,
    });

    return next();
  } catch (error) {
    console.error("Error syncing user to Convex:", error);
    // We don't want to block the user's request if the sync fails.
    // Log the error and continue.
    return next();
  }
};
