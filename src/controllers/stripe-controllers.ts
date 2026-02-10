import type { Request, Response } from "express";
import { type WithAuthProp } from "@clerk/express";
import { stripe } from "../lib/stripe";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import { getClerkAuth } from "../lib/clerkAuth";
import { getPlanForPriceId } from "../lib/plans";

export async function createCheckoutSession(
	req: WithAuthProp<Request>,
	res: Response
) {
	const auth = getClerkAuth(req);
	if (!auth.userId) {
		return res.status(401).send("Unauthorized");
	}

	try {
		const user = await convex.action(api.users.getUserForStripe, {
			clerkId: auth.userId,
		});

		if (!user) {
			return res.status(404).send("User not found in Convex database.");
		}

		let stripeCustomerId = user.stripeCustomerId;

		if (!stripeCustomerId) {
			const customer = await stripe.customers.create({
				email: user.email,
				metadata: {
					clerkId: user.clerkId,
				},
			});
			stripeCustomerId = customer.id;

			await convex.action(api.users.setStripeCustomerId, {
				clerkId: user.clerkId,
				stripeCustomerId: stripeCustomerId,
			});
		}

		const { priceId, successUrl, cancelUrl } = req.body;

		if (!priceId || !successUrl || !cancelUrl) {
			return res
				.status(400)
				.send("Missing required parameters: priceId, successUrl, cancelUrl");
		}

		const planId = getPlanForPriceId(priceId);
		if (!planId) {
			return res.status(400).send("Unknown or unsupported Stripe price ID.");
		}

		const session = await stripe.checkout.sessions.create({
			customer: stripeCustomerId,
			payment_method_types: ["card"],
			line_items: [
				{
					price: priceId,
					quantity: 1,
				},
			],
			mode: "subscription",
			success_url: successUrl,
			cancel_url: cancelUrl,
		});

		if (!session.url) {
			return res.status(500).send("Error creating Stripe checkout session.");
		}

		return res.json({ url: session.url });
	} catch (error) {
		console.error("Error creating checkout session:", error);
		res.status(500).send("Error creating checkout session");
	}
}

export async function syncStripeSession(
	req: WithAuthProp<Request>,
	res: Response
) {
	const auth = getClerkAuth(req);
	if (!auth.userId) {
		return res.status(401).send("Unauthorized");
	}

	const { sessionId } = req.body;

	if (!sessionId) {
		return res.status(400).send("Missing sessionId");
	}

	try {
		const session = await stripe.checkout.sessions.retrieve(sessionId, { expand: ["line_items"] });

		if (session.status !== "complete") {
			return res.status(400).send("Checkout session not complete.");
		}

		const subscriptionId = session.subscription;
		const priceId = session.line_items?.data[0]?.price?.id;

		if (!subscriptionId || !priceId) {
			return res
				.status(400)
				.send("Could not find subscription or price ID in session.");
		}

		const planId = getPlanForPriceId(priceId);
		if (!planId) {
			return res.status(400).send("Unknown or unsupported Stripe price ID.");
		}

		const user = await convex.action(api.users.getUserForStripe, {
			clerkId: auth.userId,
		});

		if (!user) {
			return res.status(404).send("User not found.");
		}

		// Check if user already has a subscription
		const existingSubscription = await convex.query(api.subscriptions.get, {
			userId: auth.userId,
		});

		if (existingSubscription) {
			// Update existing subscription
			await convex.action(api.subscriptions.updateSubscription, {
				userId: auth.userId, // Pass Clerk ID to action
				plan: planId,
				status: "active",
				stripeSubscriptionId: subscriptionId as string,
				stripePriceId: priceId,
			});
		} else {
			// Create new subscription
			await convex.action(api.subscriptions.createSubscription, {
				userId: auth.userId, // Pass Clerk ID to action
				plan: planId,
				status: "active",
				stripeSubscriptionId: subscriptionId as string,
				stripePriceId: priceId,
			});
		}

		return res.status(200).json({ message: "Subscription synced successfully." });
	} catch (error) {
		console.error("Error syncing Stripe session:", error);
		res.status(500).send("Error syncing Stripe session");
	}
}

export async function createCustomerPortalSession(
	req: WithAuthProp<Request>,
	res: Response
) {
	const auth = getClerkAuth(req);
	if (!auth.userId) {
		return res.status(401).send("Unauthorized");
	}

	try {
		const user = await convex.action(api.users.getUserForStripe, {
			clerkId: auth.userId,
		});

		if (!user || !user.stripeCustomerId) {
			return res.status(400).send("User or Stripe Customer ID not found.");
		}

		const session = await stripe.billingPortal.sessions.create({
			customer: user.stripeCustomerId,
			return_url: `${process.env.FRONTEND_URL}/dashboard`, // User returns here after managing subscription
		});
		console.log("CREATED SESSION: ", session)
		if (!session.url) {
			return res.status(500).send("Error creating Stripe customer portal session.");
		}

		return res.json({ url: session.url });
	} catch (error) {
		console.error("Error creating customer portal session:", error);
		res.status(500).send("Error creating customer portal session");
	}
}
