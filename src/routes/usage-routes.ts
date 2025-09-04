import { Router } from "express";
import { getUsage } from "../controllers/usage-controllers";
import { requireAuth } from "@clerk/express";

const router = Router();

router.get("/", requireAuth(), getUsage);

export default router;