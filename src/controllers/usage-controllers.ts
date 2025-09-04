import type { Request, Response } from "express";
import { type WithAuthProp } from "@clerk/express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";

export async function getUsage(req: WithAuthProp<Request>, res: Response) {
  if (!req.auth.userId) {
    return res.status(401).send("Unauthorized");
  }

  try {
    const usageRecords = await convex.query(api.usage.getUsageData, {
      userId: req.auth.userId,
    });

    let totalRequests = 0;
    let requestsThisMonth = 0;

    const currentMonth = new Date().toISOString().substring(0, 7); // YYYY-MM

    for (const record of usageRecords) {
      totalRequests += record.count;
      if (record.date.startsWith(currentMonth)) {
        requestsThisMonth += record.count;
      }
    }

    res.status(200).json({
      totalRequests,
      requestsThisMonth,
    });
  } catch (error) {
    console.error("Error fetching usage data:", error);
    res.status(500).send("Error fetching usage data");
  }
}