import type { Request } from "express";

type ClerkAuth = { userId?: string };

export function getClerkAuth(req: Request & { auth?: unknown }): ClerkAuth {
	const authValue = (req as any).auth;
	if (typeof authValue === "function") {
		return authValue() ?? {};
	}
	return (authValue ?? {}) as ClerkAuth;
}
