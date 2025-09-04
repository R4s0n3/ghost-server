import { Router } from "express";
import {
  createCheckoutSession,
  syncStripeSession,
  createCustomerPortalSession,
} from "../controllers/stripe-controllers";
import { requireAuth } from "@clerk/express";
import { syncUser } from "../middleware/syncUser";

const router = Router();

// All routes in this file require authentication and will sync the user.
router.use(requireAuth(), syncUser);

router.post("/create-checkout-session", createCheckoutSession);
router.post("/sync-session", syncStripeSession);
router.post("/create-customer-portal-session", createCustomerPortalSession);

export default router;
