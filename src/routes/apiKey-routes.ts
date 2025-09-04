import { Router } from "express";
import { generateApiKey, listApiKeys, deleteApiKey } from "../controllers/apiKey-controllers";
import { requireAuth } from "@clerk/express";
import { syncUser } from "../middleware/syncUser";

const router = Router();

// All routes in this file require authentication and will sync the user.
router.use(requireAuth(), syncUser);

router.post("/", generateApiKey);
router.get("/", listApiKeys);
router.delete("/:id", deleteApiKey);

export default router;