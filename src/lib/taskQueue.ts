type Task<T> = () => Promise<T>;

type QueueItem<T> = {
	task: Task<T>;
	resolve: (value: T) => void;
	reject: (reason?: unknown) => void;
	enqueuedAtMs: number;
};

export class TaskQueue {
	private running = 0;
	private readonly queue: QueueItem<any>[] = [];
	private readonly logTimings = process.env.LOG_TASK_QUEUE_TIMINGS === "1";

	constructor(private readonly concurrency: number, private readonly name = "queue") {
		if (!Number.isFinite(concurrency) || concurrency <= 0) {
			throw new Error(`Invalid concurrency for ${name}: ${concurrency}`);
		}
	}

	run<T>(task: Task<T>): Promise<T> {
		return new Promise((resolve, reject) => {
			this.queue.push({ task, resolve, reject, enqueuedAtMs: Date.now() });
			this.drain();
		});
	}

	private drain() {
		while (this.running < this.concurrency && this.queue.length > 0) {
			const item = this.queue.shift();
			if (!item) {
				return;
			}
			this.running += 1;
			const startedAtMs = Date.now();
			const waitMs = startedAtMs - item.enqueuedAtMs;
			(async () => {
				try {
					const result = await item.task();
					item.resolve(result);
				} catch (error) {
					item.reject(error);
				} finally {
					const runMs = Date.now() - startedAtMs;
					this.running -= 1;
					if (this.logTimings) {
						console.info(
							`[queue:${this.name}] waitMs=${waitMs} runMs=${runMs} running=${this.running} queued=${this.queue.length}`,
						);
					}
					this.drain();
				}
			})();
		}
	}
}
