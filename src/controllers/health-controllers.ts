import type { Request, Response } from "express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

export async function getHealth(req: Request, res: Response) {
  try {
    const convexHealth = await convex.query(api.health.get);
    res
      .status(200)
      .send(`Express server is online. Convex status: "${convexHealth}"`);
  } catch (error) {
    console.error("Failed to connect to Convex:", error);
    res.status(500).send("Failed to connect to Convex.");
  }
}