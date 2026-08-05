import net from "node:net";
import { log } from "../log.js";

/**
 * Port policy.
 *
 * This is an explicitly named policy rather than an inline constant because the
 * choice has a consequence beyond "find a free port": an inline MCP-Apps view
 * must declare its allowed origins in a CSP *before* it knows which port we
 * ended up on. Scanning means declaring the whole range up front; pinning means
 * failing loudly instead of moving. Browser delivery only ever wants "scan".
 */
export type PortPolicy = "scan" | "pinned";

export const DEFAULT_PORT = 4517;
export const PORT_SCAN_END = 4536;
export const HOST = "127.0.0.1";

export function portPolicy(): PortPolicy {
  return process.env["AGENT_UI_PORT_POLICY"] === "pinned" ? "pinned" : "scan";
}

export function preferredPort(): number {
  const raw = process.env["AGENT_UI_PORT"];
  const parsed = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isInteger(parsed) && parsed > 0 && parsed < 65536 ? parsed : DEFAULT_PORT;
}

/** Every origin this instance could possibly serve from, under the current policy. */
export function candidateOrigins(): string[] {
  const origins: string[] = [];
  for (const port of candidatePorts()) {
    origins.push(`http://${HOST}:${port}`, `ws://${HOST}:${port}`);
  }
  return origins;
}

export function candidatePorts(): number[] {
  const first = preferredPort();
  if (portPolicy() === "pinned") return [first];
  const ports = [first];
  for (let port = DEFAULT_PORT + 1; port <= PORT_SCAN_END; port++) {
    if (port !== first) ports.push(port);
  }
  return ports;
}

function tryListen(server: net.Server | import("node:http").Server, port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const onError = (error: NodeJS.ErrnoException) => {
      server.removeListener("listening", onListening);
      if (error.code === "EADDRINUSE" || error.code === "EACCES") resolve(false);
      else resolve(false);
    };
    const onListening = () => {
      server.removeListener("error", onError);
      resolve(true);
    };
    server.once("error", onError);
    server.once("listening", onListening);
    server.listen(port, HOST);
  });
}

/**
 * Binds to the first available port in the policy's range, on loopback only.
 *
 * Two projects with agents running means two of these processes; each gets its
 * own port, and because every tool result carries the URL, a shifted port
 * self-heals without anyone having to know it changed.
 */
export async function listenOnAvailablePort(server: import("node:http").Server): Promise<number> {
  const ports = candidatePorts();
  for (const port of ports) {
    if (await tryListen(server, port)) {
      log.info("listening", { url: `http://${HOST}:${port}` });
      return port;
    }
    log.debug("port busy, trying next", port);
  }
  throw new Error(
    portPolicy() === "pinned"
      ? `AgentUI could not bind pinned port ${ports[0]}. Free it or unset AGENT_UI_PORT_POLICY=pinned.`
      : `AgentUI could not bind any port in ${DEFAULT_PORT}-${PORT_SCAN_END}.`,
  );
}

/**
 * Binds the shell and the preview to an ADJACENT PAIR of ports (P, P+1).
 *
 * Two origins is not fussiness — it is what makes agent-authored preview code
 * both safe and functional. A single-origin iframe sandboxed without
 * `allow-same-origin` gets an *opaque* origin, and module scripts are fetched in
 * CORS mode, so `import()` of our own `/p/<id>.mjs` fails the CORS check even
 * though it sits on the very server the document came from. (The DevTools error
 * blames CSP, which sends you off loosening the one control that matters.)
 *
 * Serving the preview from a different origin lets us use
 * `sandbox="allow-scripts allow-same-origin"` — safe precisely *because* the
 * origins differ, so the frame cannot reach our DOM to strip its own sandbox —
 * and everything works normally: same-origin module fetches, import maps,
 * real URLs in DevTools. Exfiltration is stopped by CSP `connect-src 'self'`,
 * not by the sandbox.
 */
export async function listenOnAvailablePair(
  shell: import("node:http").Server,
  preview: import("node:http").Server,
): Promise<{ shellPort: number; previewPort: number }> {
  const starts = candidatePorts();
  for (const port of starts) {
    if (port + 1 > 65535) continue;
    if (!(await tryListen(shell, port))) continue;
    if (await tryListen(preview, port + 1)) {
      log.info("listening", { shell: `http://${HOST}:${port}`, preview: `http://${HOST}:${port + 1}` });
      return { shellPort: port, previewPort: port + 1 };
    }
    // Took P but not P+1: release P and try the next candidate, otherwise we
    // would strand a half-bound server.
    await new Promise<void>((resolve) => shell.close(() => resolve()));
  }
  throw new Error(`AgentUI could not bind an adjacent port pair in ${DEFAULT_PORT}-${PORT_SCAN_END}.`);
}
