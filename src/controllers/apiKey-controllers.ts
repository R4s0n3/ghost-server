import type { Request, Response } from "express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import type { Id } from "../../convex/_generated/dataModel";
import { getClerkAuth } from "../lib/clerkAuth";

export async function generateApiKey(req: Request, res: Response) {
  const auth = getClerkAuth(req);
  if (!auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const newKey = await convex.action(api.apiKeys.generate, {
      userId: auth.userId,
    });
    res.status(201).json({ apiKey: newKey });
  } catch (error) {
    console.error("Error generating API key:", error);
    res.status(500).send("Error generating API key");
  }
}

export async function listApiKeys(req: Request, res: Response) {
  const auth = getClerkAuth(req);
  if (!auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const keys = await convex.query(api.apiKeys.list, {
      userId: auth.userId,
    });
    res.status(200).json(keys);
  } catch (error) {
    console.error("Error listing API keys:", error);
    res.status(500).send("Error listing API keys");
  }
}

export async function deleteApiKey(req: Request, res: Response) {
  const auth = getClerkAuth(req);
  if (!auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  const { id } = req.params; // The API key ID from the URL parameter

  if (!id) {
    return res.status(400).send("Missing API key ID.");
  }

  try {
    await convex.action(api.apiKeys.deleteApiKey, {
      clerkId: auth.userId,
      apiKeyId: id as Id<"apiKeys">,
    });
    res.status(200).json({ message: "API key deleted successfully." });
  } catch (error) {
    console.error("Error deleting API key:", error);
    res.status(500).send("Error deleting API key.");
  }
}
