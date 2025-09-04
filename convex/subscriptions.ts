import { v } from "convex/values";
import { query, internalMutation, action } from "./_generated/server";
import { internal, api } from "./_generated/api";

// --- Queries ---

export const get = query({
  args: { userId: v.string() }, // Clerk User ID
  handler: async (ctx, args) => {
    const user = await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.userId))
      .unique();

    if (!user) {
      return null;
    }

    return await ctx.db
      .query("subscriptions")
      .withIndex("by_userId", (q) => q.eq("userId", user._id))
      .unique();
  },
});

// --- Internal Mutations ---

export const create = internalMutation({
  args: {
    userId: v.id("users"),
    plan: v.string(),
    status: v.string(),
    stripeSubscriptionId: v.optional(v.string()),
    stripePriceId: v.optional(v.string()),
    endsAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    await ctx.db.insert("subscriptions", args);
  },
});

export const update = internalMutation({
  args: {
    subscriptionId: v.id("subscriptions"),
    plan: v.optional(v.string()),
    status: v.optional(v.string()),
    stripeSubscriptionId: v.optional(v.string()),
    stripePriceId: v.optional(v.string()),
    endsAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const { subscriptionId, ...rest } = args;
    await ctx.db.patch(subscriptionId, rest);
  },
});

// --- Public Actions ---

export const createSubscription = action({
  args: {
    userId: v.string(), // Clerk ID
    plan: v.string(),
    status: v.string(),
    stripeSubscriptionId: v.optional(v.string()),
    stripePriceId: v.optional(v.string()),
    endsAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.userId,
    });
    if (!user) {
      throw new Error("User not found");
    }
    const { userId, ...rest } = args; // remove clerkId from args
    await ctx.runMutation(internal.subscriptions.create, {
      userId: user._id,
      ...rest,
    });
  },
});

export const updateSubscription = action({
  args: {
    userId: v.string(), // Clerk ID
    plan: v.optional(v.string()),
    status: v.optional(v.string()),
    stripeSubscriptionId: v.optional(v.string()),
    stripePriceId: v.optional(v.string()),
    endsAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const subscription = await ctx.runQuery(api.subscriptions.get, {
      userId: args.userId,
    });
    if (!subscription) {
      throw new Error("Subscription not found");
    }
    const { userId, ...rest } = args; // remove clerkId from args
    await ctx.runMutation(internal.subscriptions.update, {
      subscriptionId: subscription._id,
      ...rest,
    });
  },
});