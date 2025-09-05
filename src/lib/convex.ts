import { ConvexHttpClient } from "convex/browser";
import "dotenv/config";

const convexUrl = process.env.CONVEX_URL;

if (!convexUrl) {
	throw new Error("CONVEX_URL environment variable is not set!");
}

export const convex = new ConvexHttpClient(convexUrl);
