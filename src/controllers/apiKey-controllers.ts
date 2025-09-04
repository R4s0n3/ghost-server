import type { Request, Response } from "express";
import { type WithAuthProp } from "@clerk/express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

export async function generateApiKey(req: WithAuthProp<Request>, res: Response) {
  if (!req.auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const newKey = await convex.action(api.apiKeys.generate, {
      userId: req.auth.userId,
    });
    res.status(201).json({ apiKey: newKey });
  } catch (error) {
    console.error("Error generating API key:", error);
    res.status(500).send("Error generating API key");
  }
}

export async function listApiKeys(req: WithAuthProp<Request>, res: Response) {
  if (!req.auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const keys = await convex.query(api.apiKeys.list, {
      userId: req.auth.userId,
    });
    res.status(200).json(keys);
  } catch (error) {
    console.error("Error listing API keys:", error);
    res.status(500).send("Error listing API keys");
  }
}

export async function deleteApiKey(req: WithAuthProp<Request>, res: Response) {
  if (!req.auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  const { id } = req.params; // The API key ID from the URL parameter

  if (!id) {
    return res.status(400).send("Missing API key ID.");
  }

  try {
    await convex.action(api.apiKeys.deleteApiKey, {
      clerkId: req.auth.userId,
      apiKeyId: id,
    });
    res.status(200).json({ message: "API key deleted successfully." });
  } catch (error) {
    console.error("Error deleting API key:", error);
    res.status(500).send("Error deleting API key.");
  }
}
