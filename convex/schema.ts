import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  users: defineTable({
    clerkId: v.string(),
    email: v.string(),
    stripeCustomerId: v.optional(v.string()),
  }).index("by_clerk_id", ["clerkId"]),

  apiKeys: defineTable({
    userId: v.id("users"),
    key: v.string(),
  }).index("by_userId_and_key", ["userId", "key"])
    .index("by_key", ["key"]), // New index

  subscriptions: defineTable({
    userId: v.id("users"),
    plan: v.string(), // e.g., "free", "pro"
    status: v.string(), // e.g., "active", "canceled", "past_due"
    endsAt: v.optional(v.number()), // Timestamp for subscription end
    stripeSubscriptionId: v.optional(v.string()),
    stripePriceId: v.optional(v.string()),
  }).index("by_userId", ["userId"]),

  usage: defineTable({
    userId: v.id("users"),
    date: v.string(), // YYYY-MM-DD format
    count: v.number(),
  }).index("by_userId_and_date", ["userId", "date"]),
});
