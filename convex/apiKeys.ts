import { v } from "convex/values";
import { internal } from "./_generated/api";
import { action, internalMutation, query, internalQuery } from "./_generated/server";
import { customAlphabet } from "nanoid";

const ALPHABET = "0123456789abcdefghijklmnopqrstuvwxyz";
const API_KEY_LENGTH = 32;

// Helper to generate a random API key
const generateApiKey = customAlphabet(ALPHABET, API_KEY_LENGTH);

// --- Actions ---

export const generate = action({
  args: { userId: v.string() }, // Clerk User ID
  handler: async (ctx, args) => {
    // Find the user in our database
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.userId,
    });

    if (!user) {
      throw new Error("User not found");
    }

    const newKey = `sk_${generateApiKey()}`;

    await ctx.runMutation(internal.apiKeys.create, {
      userId: user._id,
      key: newKey,
    });

    return newKey;
  },
});

export const deleteApiKey = action({
  args: { clerkId: v.string(), apiKeyId: v.id("apiKeys") },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, { clerkId: args.clerkId });
    if (!user) {
      throw new Error("User not found");
    }

    const keyToDelete = await ctx.runQuery(internal.apiKeys.getById, { id: args.apiKeyId });
    if (!keyToDelete || keyToDelete.userId !== user._id) {
      throw new Error("API Key not found or does not belong to user.");
    }

    await ctx.runMutation(internal.apiKeys.remove, { id: args.apiKeyId });
  },
});

// --- Mutations ---

export const create = internalMutation({
  args: {
    userId: v.id("users"),
    key: v.string(),
  },
  handler: async (ctx, args) => {
    await ctx.db.insert("apiKeys", {
      userId: args.userId,
      key: args.key,
    });
  },
});

export const remove = internalMutation({
  args: { id: v.id("apiKeys") },
  handler: async (ctx, args) => {
    await ctx.db.delete(args.id);
  },
});

// --- Queries ---

export const list = query({
  args: { userId: v.string() }, // Clerk User ID
  handler: async (ctx, args) => {
    const user = await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.userId))
      .unique();

    if (!user) {
      return [];
    }

    const keys = await ctx.db
      .query("apiKeys")
      .withIndex("by_userId_and_key", (q) => q.eq("userId", user._id))
      .collect();

    return keys;
  },
});

export const getById = internalQuery({
  args: { id: v.id("apiKeys") },
  handler: async (ctx, args) => {
    return await ctx.db.get(args.id);
  },
});