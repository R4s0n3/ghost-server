import { unlink } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join, parse } from "node:path";
import { type Request, type Response } from "express";
import {
  analyzePdf,
  convertPdfToGrayscaleFile,
  sanitizeBaseName,
} from "./process-controllers";

export async function processDocumentApi(req: Request, res: Response) {
  // The apiKeyAuth middleware has already validated the user and attached it to req.convexUser
  if (!req.convexUser) {
    // This should technically not be reached if the middleware is applied correctly.
    return res.status(401).send("Unauthorized.");
  }

  const file = req.file;

  if (!file) {
    return res.status(400).json({ error: "File not found" });
  }
  const isPdf =
    file.mimetype === "application/pdf" ||
    file.originalname.toLowerCase().endsWith(".pdf");
  if (!isPdf) {
    return res.status(400).json({ error: "Only PDF files are supported" });
  }

  const tempPath = file.path;

  try {
    const analysis = await analyzePdf(tempPath);
    // Set the file_name to the original name of the uploaded file.
    analysis.file_name = file.originalname;
    return res.json(analysis);
  } catch (error: any) {
    console.error(error);
    return res.status(500).json({ error: error.message });
  } finally {
    // Clean up the temporary file
    await unlink(tempPath).catch((err) =>
      console.error(`Failed to delete temp file: ${tempPath}`, err)
    );
  }
}

export async function convertDocumentToGrayscaleApi(req: Request, res: Response) {
  // The apiKeyAuth middleware has already validated the user and attached it to req.convexUser
  if (!req.convexUser) {
    return res.status(401).send("Unauthorized.");
  }

  const file = req.file;
  if (!file) {
    return res.status(400).json({ error: "File not found" });
  }

  const isPdf =
    file.mimetype === "application/pdf" ||
    file.originalname.toLowerCase().endsWith(".pdf");
  if (!isPdf) {
    return res.status(400).json({ error: "Only PDF files are supported" });
  }

  const tempPath = file.path;
  const baseName = sanitizeBaseName(parse(file.originalname).name);
  const outputName = `${baseName}-grayscale.pdf`;
  const outputPath = join(tmpdir(), `${baseName}-${randomUUID()}-grayscale.pdf`);

  try {
    await convertPdfToGrayscaleFile(tempPath, outputPath);
    res.setHeader("Content-Type", "application/pdf");
    return res.download(outputPath, outputName, async (err) => {
      await unlink(tempPath).catch((cleanupErr) =>
        console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr)
      );
      await unlink(outputPath).catch((cleanupErr) =>
        console.error(`Failed to delete output file: ${outputPath}`, cleanupErr)
      );
      if (err) {
        console.error("Failed to send grayscale PDF", err);
        if (!res.headersSent) {
          res.status(500).json({ error: "Failed to send grayscale PDF" });
        }
      }
    });
  } catch (error: any) {
    console.error(error);
    await unlink(tempPath).catch((cleanupErr) =>
      console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr)
    );
    await unlink(outputPath).catch((cleanupErr) =>
      console.error(`Failed to delete output file: ${outputPath}`, cleanupErr)
    );
    return res
      .status(500)
      .json({ error: error.message ?? "Failed to convert PDF" });
  }
}
