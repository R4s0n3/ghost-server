import { query } from "./_generated/server";

export const get = query({
  args: {},
  handler: async (_ctx) => {
    return "Hello from Convex!";
  },
});
