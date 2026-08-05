import net from "node:net";
import { dataDir } from "./paths.js";
import { accessMode, bindHost, lanAddresses } from "../web/access.js";
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

export const HOST = "127.0.0.1";

/**
 * Ports are DERIVED FROM THE PROJECT, not handed out first-come-first-served.
 *
 * With a scan-from-a-fixed-base policy, the port a project gets depends on what
 * else happened to be running when it started — so the URL moves between
 * sessions and the inbox can never be bookmarked or pinned. Hashing the project
 * path instead means a given project always lands on the same pair, and the
 * inbox becomes a fixed address you can keep open.
 *
 * Slots are even so that a project's (P, P+1) shell/preview pair can never
 * overlap another project's pair. Scanning still exists, but only as a
 * genuine-collision fallback.
 */
export const PORT_BASE = 4520;
export const PORT_SLOTS = 120; // 4520–4758, stepping by 2
export const PORT_SCAN_END = PORT_BASE + PORT_SLOTS * 2;

/** Kept for anyone with AGENT_UI_PORT unset and no project (rare). */
export const DEFAULT_PORT = PORT_BASE;

/** FNV-1a. Small, stable across runs and platforms — which is the whole point. */
function hashString(value: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < value.length; i++) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

/**
 * The port this project should get. Case-folded because Windows paths vary in
 * case between launchers and the same project must not hash two ways.
 */
export function derivedPort(projectKey: string): number {
  return PORT_BASE + (hashString(projectKey.toLowerCase()) % PORT_SLOTS) * 2;
}

export function portPolicy(): PortPolicy {
  return process.env["AGENT_UI_PORT_POLICY"] === "pinned" ? "pinned" : "scan";
}

export function preferredPort(): number {
  const raw = process.env["AGENT_UI_PORT"];
  const parsed = raw ? Number.parseInt(raw, 10) : NaN;
  if (Number.isInteger(parsed) && parsed > 0 && parsed < 65536) return parsed;
  // dataDir() already resolved which directory this instance belongs to,
  // including the home-folder fallback, so it is the honest project identity.
  return derivedPort(dataDir());
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

  // The derived port first, then every other even slot — wrapping, so a project
  // whose slot is taken lands somewhere deterministic rather than restarting the
  // search from the bottom of the range and colliding with whoever is there.
  const ports = [first];
  for (let i = 1; i < PORT_SLOTS; i++) {
    const slot = PORT_BASE + (((first - PORT_BASE) / 2 + i) % PORT_SLOTS) * 2;
    if (slot !== first) ports.push(slot);
  }
  return ports;
}

/**
 * Every origin ANY project on this machine could serve from.
 *
 * Includes the LAN addresses when network access is on, because a page served
 * to a phone has THAT origin — without it the origin guard would reject writes
 * from our own page and the widget would render but refuse every answer.
 */
export function candidateOriginsAllProjects(): string[] {
  // Gated on the MODE, not merely on the machine having a LAN address. Adding
  // these unconditionally would mean a page served from 192.168.x.y counted as
  // one of ours on a server that never agreed to answer the network — widening
  // the trust set for everybody to buy a feature almost nobody switched on.
  const hosts = accessMode() === "lan" ? [HOST, ...lanAddresses()] : [HOST];
  const origins: string[] = [];
  for (let i = 0; i < PORT_SLOTS; i++) {
    const shell = PORT_BASE + i * 2;
    for (const host of hosts) origins.push(`http://${host}:${shell}`, `http://${host}:${shell + 1}`);
  }
  return origins;
}

/**
 * Resolves false for "this port is taken, try the next one" and REJECTS for
 * anything else.
 *
 * The distinction matters more than it looks. Treating every bind error as
 * "busy" means a genuine failure — EPERM from a security product, ENOTSUP,
 * EADDRNOTAVAIL because loopback is misconfigured — is reported only after
 * scanning all 120 slots, and reported as "could not bind any port in
 * 4520-4760", which sends you hunting for a port conflict that does not exist.
 * The errno is the whole diagnosis; it must survive.
 */
function tryListen(server: net.Server | import("node:http").Server, port: number): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const onError = (error: NodeJS.ErrnoException) => {
      server.removeListener("listening", onListening);
      if (error.code === "EADDRINUSE" || error.code === "EACCES") resolve(false);
      else reject(error);
    };
    const onListening = () => {
      server.removeListener("error", onError);
      resolve(true);
    };
    server.once("error", onError);
    server.once("listening", onListening);
    // May be 0.0.0.0 when network access is switched on. HOST stays 127.0.0.1
    // for every URL we generate — turning this on ADDS reachability, it does
    // not move it.
    server.listen(port, bindHost());
  });
}

/**
 * Releases a bound server. `close` reports "not running" as an error, which is
 * never interesting here — we only ever call this on a server we just bound,
 * and on the failure paths there is a real error already in flight to report.
 */
function closeQuietly(server: import("node:http").Server): Promise<void> {
  return new Promise((resolve) => server.close(() => resolve()));
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

    // From here P is BOUND, so every exit from this block must release it —
    // including the throwing one. Without the catch, a hard failure on P+1
    // (which now rejects rather than reporting "busy") would propagate out
    // leaving a listener on P that nothing owns and nothing will close.
    let bound = false;
    try {
      bound = await tryListen(preview, port + 1);
    } catch (error) {
      await closeQuietly(shell);
      throw error;
    }
    if (bound) {
      log.info("listening", { shell: `http://${HOST}:${port}`, preview: `http://${HOST}:${port + 1}` });
      return { shellPort: port, previewPort: port + 1 };
    }
    // Took P but not P+1: release P and try the next candidate, otherwise we
    // would strand a half-bound server.
    await closeQuietly(shell);
  }
  throw new Error(`AgentUI could not bind an adjacent port pair in ${DEFAULT_PORT}-${PORT_SCAN_END}.`);
}
