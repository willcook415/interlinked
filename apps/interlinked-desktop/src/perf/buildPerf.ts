type BuildPerfDetails = Record<string, unknown>;

type BuildPerfLogOptions = {
  throttleMs?: number;
};

type BuildPerfMeasureOptions = BuildPerfLogOptions & {
  minDurationMs?: number;
};

const PREFIX = "[build-perf]";
const DEFAULT_MIN_DURATION_MS = 6;

let eventSeq = 0;
const lastLogAtByKey = new Map<string, number>();

function nowMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function shouldEmit(key: string, throttleMs?: number): boolean {
  if (!throttleMs || throttleMs <= 0) return true;
  const now = nowMs();
  const prev = lastLogAtByKey.get(key) ?? -Infinity;
  if (now - prev < throttleMs) return false;
  lastLogAtByKey.set(key, now);
  return true;
}

function detailSuffix(details?: BuildPerfDetails): string {
  if (!details) return "";
  try {
    return ` ${JSON.stringify(details)}`;
  } catch {
    return "";
  }
}

export function buildPerfEvent(
  label: string,
  details?: BuildPerfDetails,
  options?: BuildPerfLogOptions
): void {
  const key = `event:${label}`;
  if (!shouldEmit(key, options?.throttleMs)) return;
  eventSeq += 1;
  console.info(`${PREFIX} #${eventSeq} ${label}${detailSuffix(details)}`);
}

export function buildPerfMeasure<T>(
  label: string,
  run: () => T,
  details?: BuildPerfDetails,
  options?: BuildPerfMeasureOptions
): T {
  const started = nowMs();
  try {
    return run();
  } finally {
    const elapsedMs = nowMs() - started;
    if (elapsedMs >= (options?.minDurationMs ?? DEFAULT_MIN_DURATION_MS)) {
      const key = `measure:${label}`;
      if (shouldEmit(key, options?.throttleMs)) {
        eventSeq += 1;
        const elapsedText = elapsedMs.toFixed(2);
        console.info(`${PREFIX} #${eventSeq} ${label}: ${elapsedText}ms${detailSuffix(details)}`);
      }
    }
  }
}

export async function buildPerfMeasureAsync<T>(
  label: string,
  run: () => Promise<T>,
  details?: BuildPerfDetails,
  options?: BuildPerfMeasureOptions
): Promise<T> {
  const started = nowMs();
  try {
    return await run();
  } finally {
    const elapsedMs = nowMs() - started;
    if (elapsedMs >= (options?.minDurationMs ?? DEFAULT_MIN_DURATION_MS)) {
      const key = `measure:${label}`;
      if (shouldEmit(key, options?.throttleMs)) {
        eventSeq += 1;
        const elapsedText = elapsedMs.toFixed(2);
        console.info(`${PREFIX} #${eventSeq} ${label}: ${elapsedText}ms${detailSuffix(details)}`);
      }
    }
  }
}

