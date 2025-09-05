
import { Router } from "express";
import { preflightDocument, testDocument } from "../controllers/process-controllers";
import multer from "multer";
import { requireAuth } from "@clerk/express";
import { tmpdir } from "os";
import { syncUser } from "../middleware/syncUser";
import { preflightTestLimiter } from "../middleware/rateLimiters";

const router = Router();

// Configure multer to save files to the system's temp directory
const upload = multer({ 
  dest: tmpdir(),
  limits: { fileSize: 5 * 1024 * 1024 } // 5 MB
});

// Route for preflighting a document
router.post("/preflight-test", preflightTestLimiter, upload.single("file"), testDocument);

// All routes after this point require authentication and will sync the user.
router.use(requireAuth(), syncUser);

router.post("/preflight", upload.single("file"), preflightDocument);

// Route for document conversion
router.get("/conversion", (req, res) => res.send("conversion"));

export default router;
