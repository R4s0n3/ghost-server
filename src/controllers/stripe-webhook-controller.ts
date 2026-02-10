import type { Request, Response } from "express";
import Stripe from "stripe";
import { stripe } from "../lib/stripe";
import { convex } from "../lib/convex";
import { api } from "../../convex/_generated/api";
import { getPlanForPriceId, resolvePlanId } from "../lib/plans";

const webhookSecret = process.env.STRIPE_WEBHOOK_SECRET;

async function getClerkIdForCustomer(customerId: string): Promise<string | null> {
	const customer = await stripe.customers.retrieve(customerId);
	if (customer.deleted) {
		return null;
	}
	const clerkId = customer.metadata?.clerkId;
	return clerkId ? String(clerkId) : null;
}

async function syncSubscriptionFromStripe(subscription: Stripe.Subscription) {
	const customerId =
		typeof subscription.customer === "string"
			? subscription.customer
			: subscription.customer.id;

	const clerkId = await getClerkIdForCustomer(customerId);
	if (!clerkId) {
		console.warn("Stripe webhook: missing clerkId metadata for customer", customerId);
		return;
	}

	const priceId = subscription.items.data[0]?.price?.id;
	const existing = await convex.query(api.subscriptions.get, { userId: clerkId });
	const planFromPrice = getPlanForPriceId(priceId);
	const planId = planFromPrice ?? (existing ? resolvePlanId(existing.plan) : null);
	if (!planId) {
		console.warn("Stripe webhook: unable to resolve plan for price", priceId);
		return;
	}

	const endsAt = subscription.current_period_end
		? subscription.current_period_end * 1000
		: undefined;

	if (existing) {
		await convex.action(api.subscriptions.updateSubscription, {
			userId: clerkId,
			plan: planId,
			status: subscription.status,
			stripeSubscriptionId: subscription.id,
			stripePriceId: priceId,
			endsAt,
		});
		return;
	}

	await convex.action(api.subscriptions.createSubscription, {
		userId: clerkId,
		plan: planId,
		status: subscription.status,
		stripeSubscriptionId: subscription.id,
		stripePriceId: priceId,
		endsAt,
	});
}

export async function handleStripeWebhook(req: Request, res: Response) {
	if (!webhookSecret) {
		console.error("STRIPE_WEBHOOK_SECRET is not configured.");
		return res.status(500).send("Webhook not configured.");
	}

	const signature = req.headers["stripe-signature"];
	if (!signature || Array.isArray(signature)) {
		return res.status(400).send("Missing Stripe signature.");
	}

	let event: Stripe.Event;
	try {
		event = stripe.webhooks.constructEvent(req.body, signature, webhookSecret);
	} catch (error) {
		console.error("Stripe webhook signature verification failed:", error);
		return res.status(400).send("Invalid signature.");
	}

	try {
		switch (event.type) {
			case "customer.subscription.created":
			case "customer.subscription.updated":
			case "customer.subscription.deleted": {
				const subscription = event.data.object as Stripe.Subscription;
				await syncSubscriptionFromStripe(subscription);
				break;
			}
			case "invoice.payment_failed":
			case "invoice.payment_succeeded": {
				const invoice = event.data.object as Stripe.Invoice;
				if (invoice.subscription) {
					const subscriptionId =
						typeof invoice.subscription === "string"
							? invoice.subscription
							: invoice.subscription.id;
					const subscription = await stripe.subscriptions.retrieve(subscriptionId);
					await syncSubscriptionFromStripe(subscription);
				}
				break;
			}
			default:
				// Unhandled event types are acknowledged for Stripe retry logic.
				break;
		}

		return res.json({ received: true });
	} catch (error) {
		console.error("Stripe webhook handling failed:", error);
		return res.status(500).send("Webhook handler failed.");
	}
}
