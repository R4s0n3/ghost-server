import { Router } from "express";
import { getHealth } from "../controllers/health-controllers";
import { requireAuth } from "@clerk/express";

const router = Router();


router.get("/", getHealth);

export default router;
