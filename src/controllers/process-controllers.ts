import { unlink } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join, parse } from "node:path";
import { type Request, type Response } from "express";
import { getClerkAuth } from "../lib/clerkAuth";
import {
	commitReservationForClerkUser,
	releaseReservationForClerkUser,
	reserveUnitsForClerkUser,
} from "../lib/quota";
import { ghostscriptQueue } from "../lib/ghostscriptQueue";

async function runGhostscriptCommand(args: string[]) {
	const proc = Bun.spawn(args, { stdout: "pipe", stderr: "pipe" });
	const [exitCode, stdout, stderr] = await Promise.all([
		proc.exited,
		new Response(proc.stdout).text(),
		new Response(proc.stderr).text(),
	]);

	if (exitCode !== 0) {
		const message = stderr.trim() || stdout.trim() || "Unknown Ghostscript error";
		throw new Error(message);
	}

	return { stdout, stderr };
}

export async function getPdfPageCount(filePath: string): Promise<number> {
	const { stdout, stderr } = await runGhostscriptCommand([
		"gs",
		"-q",
		"-dNODISPLAY",
		"-dSAFER",
		`--permit-file-read=${filePath}`,
		"-c",
		`(${filePath}) (r) file runpdfbegin pdfpagecount = quit`,
	]);
	const raw = stdout.trim() || stderr.trim();
	const pageCount = parseInt(raw, 10);
	if (!Number.isFinite(pageCount) || pageCount <= 0) {
		throw new Error("Invalid page count reported by Ghostscript.");
	}
	return pageCount;
}

export async function analyzePdf(
	filePath: string,
	pageCountOverride?: number,
): Promise<any> {
	try {
		// 1. Get Page Count
		const pageCount = pageCountOverride ?? (await getPdfPageCount(filePath));

		// 2. Get Page Sizes (BBox)
		const bboxResult = await runGhostscriptCommand([
			"gs",
			"-q",
			"-dNODISPLAY",
			"-dSAFER",
			"-dBATCH",
			"-dNOPAUSE",
			"-sDEVICE=bbox",
			filePath,
		]);
		const bboxOutput = bboxResult.stderr;
		console.log("BBox output:", bboxOutput);
		const pageSizes =
			bboxOutput
				.match(/%%BoundingBox: \d+ \d+ \d+ \d+/g)
				?.map(line => {
					const parts = line.replace("%%BoundingBox: ", "").split(" ");
					const x1 = Number.parseInt(parts[0] ?? "0", 10);
					const y1 = Number.parseInt(parts[1] ?? "0", 10);
					const x2 = Number.parseInt(parts[2] ?? "0", 10);
					const y2 = Number.parseInt(parts[3] ?? "0", 10);
					return { width_pt: x2 - x1, height_pt: y2 - y1 };
				}) || [];
		console.log("Parsed page sizes:", pageSizes);

		// 3. Get Color Info (Ink Coverage) for each page
		const colorProfiles = [];
		for (let i = 1; i <= pageCount; i++) {
			const inkcovResult = await runGhostscriptCommand([
				"gs",
				"-q",
				"-o",
				"-",
				"-dSAFER",
				"-sDEVICE=inkcov",
				`-dFirstPage=${i}`,
				`-dLastPage=${i}`,
				filePath,
			]);
			const inkcovOutput = inkcovResult.stdout.toString().trim();
			console.log(`Inkcov output for page ${i}:`, inkcovOutput);
			const [cRaw, mRaw, yRaw, kRaw, typeRaw] = inkcovOutput.split(/\s+/);

			const newProfile = {
				page: i,
				c: Number.parseFloat(cRaw ?? "0"),
				m: Number.parseFloat(mRaw ?? "0"),
				y: Number.parseFloat(yRaw ?? "0"),
				k: Number.parseFloat(kRaw ?? "0"),
				type: typeRaw ?? "",
			};

			console.log("generated Profile:: ", newProfile);
			colorProfiles.push(newProfile);
		}

		// 4. Check for Form Fields (by checking for Annots with /Widget subtype)
		const annotsResult = await runGhostscriptCommand([
			"gs",
			"-q",
			"-dNODISPLAY",
			"-dSAFER",
			"-dDumpAnnots",
			"-sDEVICE=nullpage",
			filePath,
		]);
		const annotsOutput =
			annotsResult.stdout.toString() + annotsResult.stderr.toString();
		console.log("DumpAnnots output:", annotsOutput);
		const has_formfields = /\/Subtype \/Widget/.test(annotsOutput);

		return {
			file_name: filePath.split("/").pop(),
			page_count: pageCount,
			has_formfields,
			colorProfiles,
		};
	} catch (e) {
		console.error("Ghostscript analysis failed", e);
		throw new Error("Failed to analyze PDF with Ghostscript.");
	}
}

export async function testDocument(
	req: Request,
	res: Response
) {
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
		const analysis = await ghostscriptQueue.run(() => analyzePdf(tempPath));
		// Set the file_name to the original name of the uploaded file.
		analysis.file_name = file.originalname;
		return res.json(analysis);
	} catch (error: any) {
		console.error(error);
		return res.status(500).json({ error: error.message });
	} finally {
		// Clean up the temporary file
		await unlink(tempPath).catch(err =>
			console.error(`Failed to delete temp file: ${tempPath}`, err),
		);
	}
}

export async function preflightDocument(
	req: Request,
	res: Response,
) {
	// The ClerkExpressWithAuth middleware should protect this route.
	// If you have routes that are not protected, you might need to check auth explicitly.
	const auth = getClerkAuth(req);
	if (!auth.userId) {
		return res.status(401).send("Unauthorized");
	}
	const userId = auth.userId;

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
		const result = await ghostscriptQueue.run(async () => {
			const pageCount = await getPdfPageCount(tempPath);
			const units = pageCount * 2;
			const reservation = await reserveUnitsForClerkUser(userId, units);
			if (!reservation.allowed) {
				return { reservation, units };
			}

			if (!reservation.reservationId) {
				throw new Error("Failed to create usage reservation.");
			}

			try {
				const analysis = await analyzePdf(tempPath, pageCount);
				const commitResult = await commitReservationForClerkUser(
					userId,
					reservation.reservationId,
				);
				if (!commitResult?.committed) {
					console.warn("Usage reservation commit failed", commitResult);
				}
				return { analysis, reservation, units };
			} catch (error) {
				await releaseReservationForClerkUser(userId, reservation.reservationId);
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
		await unlink(tempPath).catch(err =>
			console.error(`Failed to delete temp file: ${tempPath}`, err),
		);
	}
}

export function sanitizeBaseName(value: string): string {
	const base = value.replace(/[^a-zA-Z0-9_-]+/g, "_").replace(/^_+|_+$/g, "");
	return base.length > 0 ? base.slice(0, 80) : "document";
}

function maybeLogGhostscriptTiming(stage: string, startedAtMs: number) {
	if (process.env.LOG_GHOSTSCRIPT_TIMINGS !== "1") {
		return;
	}
	console.info(`[ghostscript:${stage}] durationMs=${Date.now() - startedAtMs}`);
}

async function runGhostscript(args: string[]): Promise<void> {
	const proc = Bun.spawn(args, { stdout: "ignore", stderr: "pipe" });
	const [exitCode, stderr] = await Promise.all([
		proc.exited,
		new Response(proc.stderr).text(),
	]);
	if (exitCode !== 0) {
		const message = stderr.trim() || "Unknown Ghostscript error";
		throw new Error(`Ghostscript grayscale conversion failed: ${message}`);
	}
}

export async function convertPdfToGrayscaleFile(
	inputPath: string,
	outputPath: string,
): Promise<void> {
	// DeviceGray ensures K-only output (no C/M/Y channels).
	await runGhostscript([
		"gs",
		"-q",
		"-dNOPAUSE",
		"-dBATCH",
		"-dSAFER",
		"-sDEVICE=pdfwrite",
		"-sColorConversionStrategy=Gray",
		"-dProcessColorModel=/DeviceGray",
		`-sOutputFile=${outputPath}`,
		inputPath,
	]);
}

export async function convertDocumentToGrayscale(
	req: Request,
	res: Response,
) {
	const auth = getClerkAuth(req);
	if (!auth.userId) {
		return res.status(401).send("Unauthorized");
	}
	const userId = auth.userId;

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
		const pageCountStartedAt = Date.now();
		const pageCount = await ghostscriptQueue.run(() => getPdfPageCount(tempPath));
		maybeLogGhostscriptTiming("page-count", pageCountStartedAt);
		const units = pageCount;
		const reservation = await reserveUnitsForClerkUser(userId, units);
		if (!reservation.allowed) {
			await unlink(tempPath).catch((cleanupErr) =>
				console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr),
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
				convertPdfToGrayscaleFile(tempPath, outputPath),
			);
			maybeLogGhostscriptTiming("grayscale-conversion", conversionStartedAt);
			const commitResult = await commitReservationForClerkUser(
				userId,
				reservation.reservationId,
			);
			if (!commitResult?.committed) {
				console.warn("Usage reservation commit failed", commitResult);
			}
		} catch (error) {
			await releaseReservationForClerkUser(userId, reservation.reservationId);
			throw error;
		}

		res.setHeader("Content-Type", "application/pdf");
		return res.download(outputPath, outputName, async (err) => {
			await unlink(tempPath).catch((cleanupErr) =>
				console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr),
			);
			await unlink(outputPath).catch((cleanupErr) =>
				console.error(`Failed to delete output file: ${outputPath}`, cleanupErr),
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
			console.error(`Failed to delete temp file: ${tempPath}`, cleanupErr),
		);
		await unlink(outputPath).catch((cleanupErr) =>
			console.error(`Failed to delete output file: ${outputPath}`, cleanupErr),
		);
		return res
			.status(500)
			.json({ error: error.message ?? "Failed to convert PDF" });
	}
}
