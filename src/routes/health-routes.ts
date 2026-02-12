import { Router } from "express";
import { getHealth } from "../controllers/health-controllers";

const router = Router();


router.get("/", getHealth);

export default router;
