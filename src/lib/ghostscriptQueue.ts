import { TaskQueue } from "./taskQueue";

function parseConcurrency(value: string | undefined, fallback: number): number {
	if (!value) return fallback;
	const parsed = Number.parseInt(value, 10);
	return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

const DEFAULT_CONCURRENCY = 3;
const concurrency = parseConcurrency(
	process.env.GHOSTSCRIPT_CONCURRENCY || process.env.PROCESSING_CONCURRENCY,
	DEFAULT_CONCURRENCY,
);

export const ghostscriptQueue = new TaskQueue(concurrency, "ghostscript");
