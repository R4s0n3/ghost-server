import type { Request, Response, NextFunction } from "express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

declare global {
  namespace Express {
    interface Request {
      convexUser?: any; // Consider defining a proper type for the user
    }
  }
}

export const apiKeyAuth = async (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const apiKey = req.header("X-API-Key");

  if (!apiKey) {
    return res.status(401).send("Unauthorized: API Key is required.");
  }

  try {
    // This single action authenticates the key, gets the user, and tracks usage.
    const user = await convex.action(api.apiKeys.authenticateAndTrackUsage, {
      key: apiKey,
    });

    if (!user) {
      return res.status(401).send("Unauthorized: Invalid API Key.");
    }

    // Attach user to the request object for use in subsequent controllers
    req.convexUser = user;

    next();
  } catch (error) {
    console.error("Error during API key authentication:", error);
    return res.status(500).send("Internal Server Error");
  }
};