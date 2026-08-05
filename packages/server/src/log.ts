import { Writable } from "node:stream";

// STDOUT IS SACRED: it carries the MCP stdio transport and nothing else.
// Every diagnostic in this package goes to stderr through here. Never
// console.log — the ESLint config bans it in packages/server for this reason.
type Level = "debug" | "info" | "warn" | "error";

const ORDER: Record<Level, number> = { debug: 10, info: 20, warn: 30, error: 40 };

function threshold(): number {
  const raw = (process.env["AGENT_UI_LOG_LEVEL"] ?? "info").toLowerCase();
  // "silent" exists for tests: best-effort writes legitimately fail when a temp
  // dir is torn down mid-flight, and error spam in a green run teaches you to
  // stop reading errors.
  if (raw === "silent") return Number.POSITIVE_INFINITY;
  return ORDER[raw as Level] ?? ORDER.info;
}

function emit(level: Level, msg: string, extra?: unknown): void {
  if (ORDER[level] < threshold()) return;
  const line = `[agentui] ${level.toUpperCase()} ${msg}`;
  if (extra === undefined) process.stderr.write(line + "\n");
  else process.stderr.write(`${line} ${safe(extra)}\n`);
}

function safe(value: unknown): string {
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export const log = {
  debug: (msg: string, extra?: unknown) => emit("debug", msg, extra),
  info: (msg: string, extra?: unknown) => emit("info", msg, extra),
  warn: (msg: string, extra?: unknown) => emit("warn", msg, extra),
  error: (msg: string, extra?: unknown) => emit("error", msg, extra),
};

/**
 * Hard guarantee that nothing but the MCP transport writes to stdout.
 *
 * A lint rule only covers our own source; this also catches a dependency that
 * decides to print a deprecation notice, which would corrupt every JSON-RPC
 * message that follows and surface as a baffling protocol error.
 *
 * Call once, before connecting the transport, and hand the returned Writable to
 * `new StdioServerTransport(process.stdin, thatWritable)` — it is the only
 * channel left that can reach the real stdout.
 */
export function protectStdout(): Writable {
  const realWrite = process.stdout.write.bind(process.stdout);

  for (const method of ["log", "info", "warn", "error", "debug", "trace", "dir"] as const) {
    console[method] = (...args: unknown[]) => {
      emit(method === "error" ? "error" : "warn", `console.${method} (redirected)`, args.map(String).join(" "));
    };
  }

  process.stdout.write = ((chunk: unknown): boolean => {
    emit("error", "blocked a stray stdout write (would corrupt the MCP stream)", String(chunk).slice(0, 200));
    return true;
  }) as typeof process.stdout.write;

  return new Writable({
    write(chunk: unknown, _encoding, callback) {
      try {
        realWrite(chunk as string | Uint8Array);
        callback();
      } catch (error) {
        callback(error as Error);
      }
    },
  });
}
