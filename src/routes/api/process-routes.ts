import { Router } from "express";
import {
  convertDocumentToGrayscaleApi,
  processDocumentApi,
} from "../../controllers/api-process-controllers";
import multer from "multer";
import { tmpdir } from "os";
import { apiKeyAuth } from "../../middleware/apiKeyAuth";

const router = Router();

// Configure multer to save files to the system's temp directory
const upload = multer({
  dest: tmpdir(),
  limits: { fileSize: 5 * 1024 * 1024 }, // 5 MB
});

// All routes in this file are for API key authenticated requests
router.use(apiKeyAuth);

router.post("/analyze", upload.single("file"), processDocumentApi);
router.post("/grayscale", upload.single("file"), convertDocumentToGrayscaleApi);

export default router;
