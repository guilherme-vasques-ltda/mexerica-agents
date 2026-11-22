const DEFAULT_MAX_RETRY_DELAY_MS = 60_000;

interface ProviderRetryOptions {
	maxRetries?: number;
	maxRetryDelayMs?: number;
	signal?: AbortSignal;
	/** The HTTP statuses that may be retried; when present it is the whole policy. */
	retryStatuses?: readonly number[];
	/** Whether an error with no HTTP status (a connection failure) is retried. Default: true. */
	retryWithoutStatus?: boolean;
	/** Wait budget shared across requests; a delay past `remainingMs` fails instead of waiting. */
	retryBudget?: { remainingMs: number };
}

interface ProviderError extends Error {
	status: number | undefined;
	headers: Headers | undefined;
}

function isProviderError(error: unknown): error is ProviderError {
	if (!(error instanceof Error) || !("status" in error) || !("headers" in error)) return false;
	return (
		(error.status === undefined || typeof error.status === "number") &&
		(error.headers === undefined || error.headers instanceof Headers)
	);
}

/** Mirrors the pinned OpenAI/Anthropic SDK retry policy; review when either SDK is upgraded. */
function isRetryableProviderError(
	error: ProviderError,
	retryWithoutStatus = true,
	retryStatuses?: readonly number[],
): boolean {
	const shouldRetry = error.headers?.get("x-should-retry");
	if (shouldRetry === "false") return false;

	// No status means the request failed before a response: the provider may have processed it.
	if (error.status === undefined) return retryWithoutStatus;
	// An explicit list is the whole policy: the header cannot add a status the caller did not allow.
	if (retryStatuses !== undefined) return retryStatuses.includes(error.status);
	if (shouldRetry === "true") return true;
	return error.status === 408 || error.status === 409 || error.status === 429 || error.status >= 500;
}

function validateServerRetryDelayMs(
	delayMs: number,
	maxRetryDelayMs: number | undefined,
	providerErrorMessage: string,
): number {
	const maxDelayMs = maxRetryDelayMs ?? DEFAULT_MAX_RETRY_DELAY_MS;
	if (maxDelayMs > 0 && delayMs > maxDelayMs) {
		throw new Error(
			`Server requested ${Math.ceil(delayMs / 1000)}s retry delay (max: ${Math.ceil(maxDelayMs / 1000)}s). ${providerErrorMessage}`,
		);
	}
	return delayMs;
}

function getRetryDelayMs(error: ProviderError, retryIndex: number, maxRetryDelayMs: number | undefined): number {
	const retryAfterMs = error.headers?.get("retry-after-ms");
	if (retryAfterMs) {
		const value = Number.parseFloat(retryAfterMs);
		if (Number.isFinite(value)) return validateServerRetryDelayMs(value, maxRetryDelayMs, error.message);
	}

	const retryAfter = error.headers?.get("retry-after");
	if (retryAfter) {
		const seconds = Number.parseFloat(retryAfter);
		const delayMs = Number.isNaN(seconds) ? Date.parse(retryAfter) - Date.now() : seconds * 1000;
		if (Number.isFinite(delayMs)) return validateServerRetryDelayMs(delayMs, maxRetryDelayMs, error.message);
	}

	const exponentialDelay = Math.min(0.5 * 2 ** retryIndex, 8) * 1000;
	return exponentialDelay * (1 - Math.random() * 0.25);
}

function createAbortError(): Error {
	const error = new Error("Request aborted");
	error.name = "AbortError";
	return error;
}

function abortableSleep(ms: number, signal?: AbortSignal): Promise<void> {
	return new Promise((resolve, reject) => {
		if (signal?.aborted) {
			reject(createAbortError());
			return;
		}

		const onAbort = () => {
			clearTimeout(timeout);
			reject(createAbortError());
		};
		const timeout = setTimeout(
			() => {
				signal?.removeEventListener("abort", onAbort);
				resolve();
			},
			Math.max(0, ms),
		);
		signal?.addEventListener("abort", onAbort, { once: true });
	});
}

/**
 * Reproduce the retry behavior used by the OpenAI and Anthropic SDKs while making
 * their backoff sleep interruptible. Their built-in retry timers ignore the
 * request AbortSignal, so callers must invoke the SDK with `maxRetries: 0` and
 * wrap the request with this helper. Provider-requested delays above
 * `maxRetryDelayMs` fail immediately (60 seconds by default); set it to zero to
 * disable the limit.
 */
export async function retryProviderRequest<T>(
	request: () => Promise<T>,
	options: ProviderRetryOptions = {},
): Promise<T> {
	const maxRetries = options.maxRetries ?? 0;
	let retriesRemaining = maxRetries;

	for (;;) {
		try {
			// Each retry is a fresh SDK request, so X-Stainless-Retry-Count remains zero.
			return await request();
		} catch (error) {
			if (options.signal?.aborted) throw createAbortError();
			if (
				retriesRemaining <= 0 ||
				!isProviderError(error) ||
				!isRetryableProviderError(error, options.retryWithoutStatus, options.retryStatuses)
			)
				throw error;

			const retryIndex = maxRetries - retriesRemaining;
			retriesRemaining--;
			// A server date already in the past can make the delay negative; the wait is never negative.
			const delayMs = Math.max(0, getRetryDelayMs(error, retryIndex, options.maxRetryDelayMs));
			// The turn's shared budget is spent, not renewed: a delay past it ends the request.
			if (options.retryBudget) {
				if (delayMs > options.retryBudget.remainingMs) throw error;
				options.retryBudget.remainingMs -= delayMs;
			}
			await abortableSleep(delayMs, options.signal);
		}
	}
}
