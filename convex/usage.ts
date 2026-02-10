import { v } from "convex/values";
import { internal } from "./_generated/api";
import { action, internalMutation, query } from "./_generated/server";

const RESERVATION_TTL_MS = 10 * 60 * 1000; // 10 minutes

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

export const getUsageReservations = query({
  args: {
    userId: v.string(), // Clerk ID
  },
  handler: async (ctx, args) => {
    const user = await ctx.db
      .query("users")
      .withIndex("by_clerk_id", (q) => q.eq("clerkId", args.userId))
      .unique();

    if (!user) {
      return [];
    }

    return await ctx.db
      .query("usageReservations")
      .withIndex("by_userId_and_date", (q) => q.eq("userId", user._id))
      .collect();
  },
});

export const increment = internalMutation({
  args: { userId: v.id("users"), units: v.optional(v.number()) },
  handler: async (ctx, args) => {
    const today = new Date().toISOString().slice(0, 10); // YYYY-MM-DD
    const unitsToIncrement = Math.max(args.units ?? 1, 0);
    if (unitsToIncrement === 0) {
      return;
    }

    const todaysUsage = await ctx.db
      .query("usage")
      .withIndex("by_userId_and_date", (q) =>
        q.eq("userId", args.userId).eq("date", today)
      )
      .unique();

    if (todaysUsage) {
      await ctx.db.patch(todaysUsage._id, {
        count: todaysUsage.count + unitsToIncrement,
      });
    } else {
      await ctx.db.insert("usage", {
        userId: args.userId,
        date: today,
        count: unitsToIncrement,
      });
    }
  },
});

export const incrementForClerkUser = action({
  args: { clerkId: v.string(), units: v.optional(v.number()) },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });

    if (user) {
      await ctx.runMutation(internal.usage.increment, {
        userId: user._id,
        units: args.units,
      });
    }
  },
});

export const reserve = internalMutation({
  args: {
    userId: v.id("users"),
    units: v.number(),
    monthlyQuota: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const unitsToReserve = Math.max(args.units, 0);
    const now = Date.now();
    const today = new Date(now).toISOString().slice(0, 10); // YYYY-MM-DD
    const currentMonth = today.substring(0, 7); // YYYY-MM

    const usageRecords = await ctx.db
      .query("usage")
      .withIndex("by_userId_and_date", (q) => q.eq("userId", args.userId))
      .collect();

    let totalThisMonth = 0;
    for (const record of usageRecords) {
      if (record.date.startsWith(currentMonth)) {
        totalThisMonth += record.count;
      }
    }

    const reservationRecords = await ctx.db
      .query("usageReservations")
      .withIndex("by_userId_and_date", (q) => q.eq("userId", args.userId))
      .collect();

    let pendingUnits = 0;
    for (const record of reservationRecords) {
      if (!record.date.startsWith(currentMonth)) {
        continue;
      }

      if (record.status === "pending") {
        if (record.expiresAt <= now) {
          await ctx.db.patch(record._id, {
            status: "expired",
            releasedAt: now,
          });
          continue;
        }
        pendingUnits += record.units;
      }
    }

    const monthlyQuota = args.monthlyQuota ?? null;
    if (
      monthlyQuota !== null &&
      totalThisMonth + pendingUnits + unitsToReserve > monthlyQuota
    ) {
      return {
        allowed: false,
        totalThisMonth,
        pendingUnits,
        monthlyQuota,
      };
    }

    if (unitsToReserve === 0) {
      return {
        allowed: true,
        reservationId: null,
        totalThisMonth,
        pendingUnits,
        monthlyQuota,
      };
    }

    const reservationId = await ctx.db.insert("usageReservations", {
      userId: args.userId,
      date: today,
      units: unitsToReserve,
      status: "pending",
      createdAt: now,
      expiresAt: now + RESERVATION_TTL_MS,
    });

    return {
      allowed: true,
      reservationId,
      totalThisMonth,
      pendingUnits,
      monthlyQuota,
    };
  },
});

export const commitReservation = internalMutation({
  args: {
    reservationId: v.id("usageReservations"),
    userId: v.id("users"),
  },
  handler: async (ctx, args) => {
    const reservation = await ctx.db.get(args.reservationId);
    if (!reservation || reservation.userId !== args.userId) {
      return { committed: false, reason: "not_found" };
    }

    if (reservation.status === "committed") {
      return { committed: true, already: true };
    }

    if (reservation.status !== "pending") {
      return { committed: false, reason: reservation.status };
    }

    const now = Date.now();
    if (reservation.expiresAt <= now) {
      await ctx.db.patch(reservation._id, {
        status: "expired",
        releasedAt: now,
      });
      return { committed: false, reason: "expired" };
    }

    const todaysUsage = await ctx.db
      .query("usage")
      .withIndex("by_userId_and_date", (q) =>
        q.eq("userId", args.userId).eq("date", reservation.date)
      )
      .unique();

    if (todaysUsage) {
      await ctx.db.patch(todaysUsage._id, {
        count: todaysUsage.count + reservation.units,
      });
    } else {
      await ctx.db.insert("usage", {
        userId: args.userId,
        date: reservation.date,
        count: reservation.units,
      });
    }

    await ctx.db.patch(reservation._id, {
      status: "committed",
      committedAt: now,
    });

    return { committed: true };
  },
});

export const releaseReservation = internalMutation({
  args: {
    reservationId: v.id("usageReservations"),
    userId: v.id("users"),
  },
  handler: async (ctx, args) => {
    const reservation = await ctx.db.get(args.reservationId);
    if (!reservation || reservation.userId !== args.userId) {
      return { released: false, reason: "not_found" };
    }

    if (reservation.status !== "pending") {
      return { released: false, reason: reservation.status };
    }

    const now = Date.now();
    await ctx.db.patch(reservation._id, {
      status: "released",
      releasedAt: now,
    });

    return { released: true };
  },
});

export const reserveForClerkUser = action({
  args: {
    clerkId: v.string(),
    units: v.number(),
    monthlyQuota: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });

    if (!user) {
      return {
        allowed: false,
        reservationId: null,
        totalThisMonth: 0,
        pendingUnits: 0,
        monthlyQuota: args.monthlyQuota ?? null,
      };
    }

    return await ctx.runMutation(internal.usage.reserve, {
      userId: user._id,
      units: args.units,
      monthlyQuota: args.monthlyQuota,
    });
  },
});

export const commitReservationForClerkUser = action({
  args: {
    clerkId: v.string(),
    reservationId: v.id("usageReservations"),
  },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });

    if (!user) {
      return { committed: false, reason: "user_not_found" };
    }

    return await ctx.runMutation(internal.usage.commitReservation, {
      userId: user._id,
      reservationId: args.reservationId,
    });
  },
});

export const releaseReservationForClerkUser = action({
  args: {
    clerkId: v.string(),
    reservationId: v.id("usageReservations"),
  },
  handler: async (ctx, args) => {
    const user = await ctx.runQuery(internal.users.getUserByClerkId, {
      clerkId: args.clerkId,
    });

    if (!user) {
      return { released: false, reason: "user_not_found" };
    }

    return await ctx.runMutation(internal.usage.releaseReservation, {
      userId: user._id,
      reservationId: args.reservationId,
    });
  },
});
