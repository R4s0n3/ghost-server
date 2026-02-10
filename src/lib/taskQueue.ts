type Task<T> = () => Promise<T>;

type QueueItem<T> = {
	task: Task<T>;
	resolve: (value: T) => void;
	reject: (reason?: unknown) => void;
};

export class TaskQueue {
	private running = 0;
	private readonly queue: QueueItem<any>[] = [];

	constructor(private readonly concurrency: number, private readonly name = "queue") {
		if (!Number.isFinite(concurrency) || concurrency <= 0) {
			throw new Error(`Invalid concurrency for ${name}: ${concurrency}`);
		}
	}

	run<T>(task: Task<T>): Promise<T> {
		return new Promise((resolve, reject) => {
			this.queue.push({ task, resolve, reject });
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
			(async () => {
				try {
					const result = await item.task();
					item.resolve(result);
				} catch (error) {
					item.reject(error);
				} finally {
					this.running -= 1;
					this.drain();
				}
			})();
		}
	}
}
