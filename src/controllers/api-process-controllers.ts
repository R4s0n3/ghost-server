import { unlink } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join, parse } from "node:path";
import { type Request, type Response } from "express";
import {
  analyzePdf,
  convertPdfToGrayscaleFile,
  getPdfPageCount,
  sanitizeBaseName,
} from "./process-controllers";
import {
  commitReservationForClerkUser,
  releaseReservationForClerkUser,
  reserveUnitsForClerkUser,
} from "../lib/quota";
import { ghostscriptQueue } from "../lib/ghostscriptQueue";

function maybeLogGhostscriptTiming(stage: string, startedAtMs: number) {
  if (process.env.LOG_GHOSTSCRIPT_TIMINGS !== "1") {
    return;
  }
  console.info(`[ghostscript:${stage}] durationMs=${Date.now() - startedAtMs}`);
}

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
    const clerkId = req.convexUser?.clerkId;
    if (!clerkId) {
      return res.status(500).send("Authenticated user missing Clerk ID.");
    }

    const result = await ghostscriptQueue.run(async () => {
      const pageCount = await getPdfPageCount(tempPath);
      const units = pageCount * 2;
      const reservation = await reserveUnitsForClerkUser(clerkId, units);
      if (!reservation.allowed) {
        return { reservation, units };
      }

      if (!reservation.reservationId) {
        throw new Error("Failed to create usage reservation.");
      }

      try {
        const analysis = await analyzePdf(tempPath, pageCount);
        const commitResult = await commitReservationForClerkUser(
          clerkId,
          reservation.reservationId
        );
        if (!commitResult?.committed) {
          console.warn("Usage reservation commit failed", commitResult);
        }
        return { analysis, reservation, units };
      } catch (error) {
        await releaseReservationForClerkUser(clerkId, reservation.reservationId);
        throw error;
      }
    });

    if (!result.analysis) {
      return res.status(402).json({
        error: "Monthly quota exceeded.",
        plan: result.reservation.planId,
        monthlyQuota: result.reservation.monthlyQuota,
        unitsThisMonth: result.reservation.totalThisMonth,
        pendingUnits: result.reservation.pendingUnits,
        unitsRequested: result.units,
      });
    }

    const analysis = result.analysis;
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
    const clerkId = req.convexUser?.clerkId;
    if (!clerkId) {
      return res.status(500).send("Authenticated user missing Clerk ID.");
    }

    const pageCountStartedAt = Date.now();
    const pageCount = await ghostscriptQueue.run(() => getPdfPageCount(tempPath));
    maybeLogGhostscriptTiming("page-count", pageCountStartedAt);
    const units = pageCount;
    const reservation = await reserveUnitsForClerkUser(clerkId, units);
    if (!reservation.allowed) {
      await unlink(tempPath).catch((cleanupErr) =>
        console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr)
      );
      await unlink(outputPath).catch((cleanupErr: any) => {
        if (cleanupErr?.code !== "ENOENT") {
          console.error(`Failed to delete output file: ${outputPath}`, cleanupErr);
        }
      });
      return res.status(402).json({
        error: "Monthly quota exceeded.",
        plan: reservation.planId,
        monthlyQuota: reservation.monthlyQuota,
        unitsThisMonth: reservation.totalThisMonth,
        pendingUnits: reservation.pendingUnits,
        unitsRequested: units,
      });
    }
    if (!reservation.reservationId) {
      throw new Error("Failed to create usage reservation.");
    }

    try {
      const conversionStartedAt = Date.now();
      await ghostscriptQueue.run(() =>
        convertPdfToGrayscaleFile(tempPath, outputPath)
      );
      maybeLogGhostscriptTiming("grayscale-conversion", conversionStartedAt);
      const commitResult = await commitReservationForClerkUser(
        clerkId,
        reservation.reservationId
      );
      if (!commitResult?.committed) {
        console.warn("Usage reservation commit failed", commitResult);
      }
    } catch (error) {
      await releaseReservationForClerkUser(clerkId, reservation.reservationId);
      throw error;
    }

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
