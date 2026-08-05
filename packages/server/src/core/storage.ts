import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { log } from "../log.js";

/**
 * Durable, crash-safe writes for session state.
 *
 * On Windows, `rename` over an existing file fails with EPERM/EBUSY whenever
 * something else momentarily holds a handle — antivirus scanners and search
 * indexers do this constantly, and it is intermittent enough that it would
 * otherwise show up as a mystery bug months later. Every atomic write retries
 * with a short backoff before giving up.
 */

const RETRY_CODES = new Set(["EPERM", "EBUSY", "EACCES", "ENOENT"]);
const ATTEMPTS = 4;
const BACKOFF_MS = 50;

function errorCode(error: unknown): string | undefined {
  return typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : undefined;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export async function ensureDir(dir: string): Promise<void> {
  await fsp.mkdir(dir, { recursive: true });
}

/** Write via a temp file + rename so a reader never observes a partial file. */
export async function writeFileAtomic(filePath: string, contents: string): Promise<void> {
  await ensureDir(path.dirname(filePath));
  const tmp = `${filePath}.tmp`;

  let lastError: unknown;
  for (let attempt = 1; attempt <= ATTEMPTS; attempt++) {
    try {
      await fsp.writeFile(tmp, contents, "utf8");
      await fsp.rename(tmp, filePath);
      return;
    } catch (error) {
      lastError = error;
      const code = errorCode(error);
      if (!code || !RETRY_CODES.has(code) || attempt === ATTEMPTS) break;
      log.debug(`atomic write retry ${attempt}/${ATTEMPTS} (${code})`, filePath);
      await sleep(BACKOFF_MS * attempt);
    }
  }

  await fsp.rm(tmp, { force: true }).catch(() => {});
  throw lastError;
}

export async function writeJsonAtomic(filePath: string, value: unknown): Promise<void> {
  await writeFileAtomic(filePath, JSON.stringify(value, null, 2) + "\n");
}

export async function readJson<T>(filePath: string): Promise<T | undefined> {
  try {
    return JSON.parse(await fsp.readFile(filePath, "utf8")) as T;
  } catch (error) {
    const code = errorCode(error);
    if (code === "ENOENT") return undefined;
    // A corrupt file must never take the whole server down at boot — hydration
    // logs and skips instead.
    log.warn("could not read json", { filePath, error: String(error) });
    return undefined;
  }
}

/** Appends one newline-terminated line. Used for the event log. */
export async function appendLine(filePath: string, line: string): Promise<void> {
  await ensureDir(path.dirname(filePath));
  await fsp.appendFile(filePath, line.endsWith("\n") ? line : line + "\n", "utf8");
}

export async function readLines(filePath: string): Promise<string[]> {
  try {
    const raw = await fsp.readFile(filePath, "utf8");
    return raw.split(/\r?\n/).filter((line) => line.trim().length > 0);
  } catch (error) {
    if (errorCode(error) === "ENOENT") return [];
    throw error;
  }
}

export function existsSync(target: string): boolean {
  return fs.existsSync(target);
}

/**
 * Marks a data directory as self-ignoring the first time we create it.
 *
 * The server writes into whatever project an agent happens to be working in, so
 * it must never make someone else's repo dirty. A `.gitignore` containing `*`
 * inside our own folder ignores it without touching the project's own file.
 */
export async function ensureSelfIgnoringDir(dir: string): Promise<void> {
  const marker = path.join(dir, ".gitignore");
  if (fs.existsSync(marker)) return;
  await ensureDir(dir);
  await fsp
    .writeFile(marker, "# AgentUI runtime data — ignored wholesale so it never dirties your repo.\n*\n", "utf8")
    .catch((error: unknown) => log.warn("could not write .agent-ui/.gitignore", error));
}

/**
 * A debounced writer: coalesces bursts (slider drags) into at most one write per
 * interval, but `flush()` forces an immediate write for events that matter —
 * a commit, a comment, finalize.
 */
export class DebouncedJsonWriter {
  #timer: NodeJS.Timeout | undefined;
  #pending: unknown;
  #writing: Promise<void> | undefined;

  constructor(
    private readonly filePath: string,
    private readonly intervalMs = 250,
  ) {}

  schedule(value: unknown): void {
    this.#pending = value;
    if (this.#timer) return;
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      void this.flush();
    }, this.intervalMs);
    this.#timer.unref?.();
  }

  async flush(value?: unknown): Promise<void> {
    if (value !== undefined) this.#pending = value;
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = undefined;
    }
    const payload = this.#pending;
    if (payload === undefined) return;
    this.#pending = undefined;

    // Serialize writes so a flush during an in-flight write cannot interleave.
    const previous = this.#writing ?? Promise.resolve();
    this.#writing = previous
      .catch(() => {})
      .then(() => writeJsonAtomic(this.filePath, payload))
      .catch((error: unknown) => log.error("state write failed", { filePath: this.filePath, error: String(error) }));
    await this.#writing;
  }

  dispose(): void {
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = undefined;
    }
  }
}
