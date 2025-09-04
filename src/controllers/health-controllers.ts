import type { Request, Response } from "express";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import { exec } from "child_process";

export async function getHealth(req: Request, res: Response) {
  let ghostscriptStatus = "Not checked";
  let ghostscriptError = null;

  try {
    await new Promise<void>((resolve, reject) => {
      exec("gs -v", (error, stdout, stderr) => {
        if (error) {
          ghostscriptError = `Error: ${error.message}`;
          reject(error);
          return;
        }
        if (stderr) {
          ghostscriptError = `Stderr: ${stderr}`;
        }
        ghostscriptStatus = stdout.trim();
        resolve();
      });
    });
  } catch (error) {
    if (!ghostscriptError) { // If error was not set by exec callback
      ghostscriptError = `Failed to execute gs -v: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  try {
    const convexHealth = await convex.query(api.health.get);
    res
      .status(200)
      .send(`Express server is online. Convex status: "${convexHealth}". Ghostscript status: ${ghostscriptStatus}${ghostscriptError ? ` (Error: ${ghostscriptError})` : ""}`);
  } catch (error) {
    console.error("Failed to connect to Convex:", error);
    res.status(500).send(`Failed to connect to Convex. Ghostscript status: ${ghostscriptStatus}${ghostscriptError ? ` (Error: ${ghostscriptError})` : ""}`);
  }
}