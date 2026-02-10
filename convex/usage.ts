import { v } from "convex/values";
import { internal } from "./_generated/api";
import { action, internalMutation, query } from "./_generated/server";

export const getUsageData = query({
  args: {
    userId: v.string(), // This is Clerk ID
  },
  handler: async (ctx, args) => {
    const user = await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.userId))
      .unique();

    if (!user) {
      return [];
    }

    // This will collect all usage records for the user.
    // You might want to add date range filters here in the future.
    return await ctx.db
      .query("usage")
      .withIndex("by_userId_and_date", (q) => q.eq("userId", user._id))
      .collect();
  },
});

export const increment = internalMutation({
  args: { userId: v.id("users"), requests: v.optional(v.number()) },
  handler: async (ctx, args) => {
    const today = new Date().toISOString().slice(0, 10); // YYYY-MM-DD
    const requestsToIncrement = args.requests ?? 1;

    const todaysUsage = await ctx.db
      .query("usage")
      .withIndex("by_userId_and_date", (q) =>
        q.eq("userId", args.userId).eq("date", today)
      )
      .unique();

    if (todaysUsage) {
      await ctx.db.patch(todaysUsage._id, {
        count: todaysUsage.count + requestsToIncrement,
      });
    } else {
      await ctx.db.insert("usage", {
        userId: args.userId,
        date: today,
        count: requestsToIncrement,
      });
    }
  },
});

export const incrementForClerkUser = action({
  args: { clerkId: v.string() },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, { 
      clerkId: args.clerkId 
    });

    if (user) {
      await ctx.runMutation(internal.usage.increment, { userId: user._id });
    }
  },
});