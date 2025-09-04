import { Router } from "express";
import { getSubscription } from "../controllers/subscription-controllers";
import { requireAuth } from "@clerk/express";
import { syncUser } from "../middleware/syncUser";

const router = Router();

// All routes in this file require authentication and will sync the user.
router.use(requireAuth(), syncUser);

router.get("/", getSubscription);

export default router;
