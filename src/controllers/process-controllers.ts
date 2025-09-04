import { unlink } from "node:fs/promises";
import { type Request, type Response } from "express";
import { type WithAuthProp } from "@clerk/express";



export async function testDocument(
	req: Request,
	res: Response
) {

	const file = req.file;

	if (!file) {
		return res.status(400).json({ error: "File not found" });
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
		await unlink(tempPath).catch(err =>
			console.error(`Failed to delete temp file: ${tempPath}`, err),
		);
	}

}
export async function preflightDocument(
	req: WithAuthProp<Request>,
	res: Response,
) {
	// The ClerkExpressWithAuth middleware should protect this route.
	// If you have routes that are not protected, you might need to check req.auth.
	if (!req.auth?.userId) {
		return res.status(401).send("Unauthorized");
	}

	const file = req.file;

	if (!file) {
		return res.status(400).json({ error: "File not found" });
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
		await unlink(tempPath).catch(err =>
			console.error(`Failed to delete temp file: ${tempPath}`, err),
		);
	}
}

async function analyzePdf(filePath: string): Promise<any> {
	try {
		// 1. Get Page Count
		const pageCountProc = Bun.spawnSync([
			"gs",
			"-q",
			"-dNODISPLAY",
			"-c",
			`(${filePath}) (r) file runpdfbegin pdfpagecount = quit`,
		]);
		if (pageCountProc.exitCode !== 0)
			throw new Error(
				`Ghostscript page count failed: ${pageCountProc.stderr}`,
			);
		const pageCount = parseInt(pageCountProc.stdout.toString().trim(), 10);

		// 2. Get Page Sizes (BBox)
		const bboxProc = Bun.spawnSync([
			"gs",
			"-q",
			"-dNODISPLAY",
			"-dBATCH",
			"-dNOPAUSE",
			"-sDEVICE=bbox",
			filePath,
		]);
		if (bboxProc.exitCode !== 0)
			throw new Error(`Ghostscript bbox failed: ${bboxProc.stderr}`);
		const bboxOutput = bboxProc.stderr.toString();
		console.log("BBox output:", bboxOutput);
		const pageSizes =
			bboxOutput
				.match(/%%BoundingBox: \d+ \d+ \d+ \d+/g)
				?.map(line => {
					const parts = line.replace("%%BoundingBox: ", "").split(" ");
					const [x1, y1, x2, y2] = parts.map(p => parseInt(p, 10));
					return { width_pt: x2 - x1, height_pt: y2 - y1 };
				}) || [];
		console.log("Parsed page sizes:", pageSizes);

		// 3. Get Color Info (Ink Coverage) for each page
		const colorProfiles = [];
		for (let i = 1; i <= pageCount; i++) {
			const inkcovProc = Bun.spawnSync([
				"gs",
				"-q",
				"-o",
				"-",
				"-sDEVICE=inkcov",
				`-dFirstPage=${i}`,
				`-dLastPage=${i}`,
				filePath,
			]);
			if (inkcovProc.exitCode !== 0) {
				console.error(
					`Ghostscript inkcov failed for page ${i}:`,
					inkcovProc.stderr.toString(),
				);
				throw new Error(
					`Ghostscript inkcov failed for page ${i}: ${inkcovProc.stderr}`,
				);
			}
			const inkcovOutput = inkcovProc.stdout.toString().trim();
			console.log(`Inkcov output for page ${i}:`, inkcovOutput);
			const [c, m, y, k, type] = inkcovOutput.split(/\s+/);

			const newProfile = {
				page: i,
				c: +c,
				m: +m,
				y: +y,
				k: +k,
				type,
			};

			console.log("generated Profile:: ", newProfile);
			colorProfiles.push(newProfile);
		}

		// 4. Check for Form Fields (by checking for Annots with /Widget subtype)
		const annotsProc = Bun.spawnSync([
			"gs",
			"-q",
			"-dNODISPLAY",
			"-dDumpAnnots",
			"-sDEVICE=nullpage",
			filePath,
		]);
		if (annotsProc.exitCode !== 0) {
			console.error(
				`Ghostscript DumpAnnots failed:`,
				annotsProc.stderr.toString(),
			);
			throw new Error(`Ghostscript DumpAnnots failed: ${annotsProc.stderr}`);
		}
		const annotsOutput =
			annotsProc.stdout.toString() + annotsProc.stderr.toString();
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
