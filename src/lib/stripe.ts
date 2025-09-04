import Stripe from "stripe";
import "dotenv/config";

const stripeSecretKey = process.env.STRIPE_SECRET_KEY;

if (!stripeSecretKey) {
  // In production, this should throw an error.
  // In development, we can allow it to proceed with a warning.
  if (process.env.NODE_ENV === "production") {
    throw new Error("STRIPE_SECRET_KEY environment variable is not set!");
  } else {
    console.warn(
      "STRIPE_SECRET_KEY is not set. Stripe functionality will not work until it is provided."
    );
  }
}

export const stripe = new Stripe(stripeSecretKey || "", {
  apiVersion: "2024-06-20",
  typescript: true,
});
