import { v } from "convex/values";
import { action, internalMutation, internalQuery } from "./_generated/server";
import { internal } from "./_generated/api";
import type { Doc } from "./_generated/dataModel";

// Internal mutation to create or update a user
export const createOrUpdateUser = internalMutation({
  args: {
    clerkId: v.string(),
    email: v.string(),
  },
  handler: async (ctx, args) => {
    const user = await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.clerkId))
      .unique();

    if (user === null) {
      await ctx.db.insert("users", {
        clerkId: args.clerkId,
        email: args.email,
      });
    } else {
      if (user.email !== args.email) {
        await ctx.db.patch(user._id, { email: args.email });
      }
    }
  },
});

// Public action to be called from the Express server
export const sync = action({
  args: {
    clerkId: v.string(),
    email: v.string(),
  },
  handler: async (ctx, args) => {
    await ctx.runMutation(internal.users.createOrUpdateUser, {
      clerkId: args.clerkId,
      email: args.email,
    });
  },
});

export const getUserByClerkId = internalQuery({
  args: { clerkId: v.string() },
  handler: async (ctx, args) => {
    return await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.clerkId))
      .unique();
  },
});

export const updateStripeCustomerId = internalMutation({
  args: {
    userId: v.id("users"),
    stripeCustomerId: v.string(),
  },
  handler: async (ctx, args) => {
    await ctx.db.patch(args.userId, {
      stripeCustomerId: args.stripeCustomerId,
    });
  },
});

// --- Stripe Actions ---

export const getUserForStripe = action({
  args: { clerkId: v.string() },
  handler: async (ctx, args): Promise<Doc<"users"> | null> => {
    return await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });
  },
});

export const setStripeCustomerId = action({
  args: { clerkId: v.string(), stripeCustomerId: v.string() },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });
    if (!user) {
      throw new Error("User not found");
    }
    await ctx.runMutation(internal.users.updateStripeCustomerId, {
      userId: user._id,
      stripeCustomerId: args.stripeCustomerId,
    });
  },
});
