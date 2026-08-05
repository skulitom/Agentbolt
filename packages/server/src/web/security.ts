import type { NextFunction, Request, Response } from "express";
import { HOST, candidateOriginsAllProjects } from "../core/ports.js";
import { accessMode, lanAddresses } from "./access.js";
import { log } from "../log.js";

/**
 * This server listens all day on loopback while you browse the web, so it has
 * to assume any page in your browser may try to talk to it.
 *
 * Binding to 127.0.0.1 stops remote hosts, but NOT DNS rebinding: an attacker
 * resolves evil.example to 127.0.0.1 and the browser happily sends same-origin
 * requests to us with `Host: evil.example`. Checking the Host header is the
 * standard defence — a rebound request cannot forge it, because the browser
 * sets it from the URL the attacker had to use.
 */
const LOOPBACK_HOSTNAMES = ["127.0.0.1", "localhost", "[::1]", "::1"];

/**
 * Loopback always; this machine's own LAN addresses only when network access
 * was switched on.
 *
 * Deliberately the SPECIFIC addresses rather than "anything that is not a
 * name": the whole point of the check is that a rebound request cannot forge
 * the Host header, and a wildcard would give that away.
 */
function allowedHostnames(): Set<string> {
  return new Set(accessMode() === "lan" ? [...LOOPBACK_HOSTNAMES, ...lanAddresses()] : LOOPBACK_HOSTNAMES);
}

function hostnameOf(hostHeader: string | undefined): string | undefined {
  if (!hostHeader) return undefined;
  // Strip the port; keep bracketed IPv6 intact.
  const match = /^(\[[^\]]+\]|[^:]+)(?::\d+)?$/.exec(hostHeader.trim());
  return match?.[1]?.toLowerCase();
}

export function hostGuard(req: Request, res: Response, next: NextFunction): void {
  const hostname = hostnameOf(req.headers.host);
  if (hostname && allowedHostnames().has(hostname)) {
    next();
    return;
  }
  log.warn("rejected request with unexpected Host header", { host: req.headers.host, path: req.path });
  res.status(403).type("text/plain").send("AgentUI only serves requests addressed to localhost.\n");
}

export function isAllowedHostHeader(hostHeader: string | undefined): boolean {
  const hostname = hostnameOf(hostHeader);
  return Boolean(hostname && allowedHostnames().has(hostname));
}

/**
 * Every origin one of our own pages could be served from.
 *
 * Seeded with the derived port range that covers every project on this machine,
 * and extended at bind time with the ports this instance actually took. That
 * second part is load-bearing: AGENT_UI_PORT can put us outside the derived
 * range entirely, and without it the guard would reject writes from our OWN
 * page — a self-inflicted 403 that would look like a mysterious UI failure.
 */
const TRUSTED_ORIGINS = new Set(candidateOriginsAllProjects());

export function trustOwnOrigins(...ports: number[]): void {
  for (const port of ports) TRUSTED_ORIGINS.add(`http://${HOST}:${port}`);
}

/**
 * CORS for peer discovery only: one AgentUI instance's dashboard fetching
 * /api/ping from another instance's port. Nothing else is cross-origin, so the
 * allowlist is exactly our own candidate origins.
 */
export function peerCors(req: Request, res: Response, next: NextFunction): void {
  const origin = req.headers.origin;
  if (origin && TRUSTED_ORIGINS.has(origin)) {
    res.setHeader("Access-Control-Allow-Origin", origin);
    res.setHeader("Vary", "Origin");
  }
  next();
}

/**
 * CSRF guard for the mutating API routes.
 *
 * The Host check above stops DNS rebinding, but not a plain cross-site form
 * POST: a hostile page submitting a form at http://127.0.0.1:4517/api/... is a
 * "simple" request (no preflight), and its Host header is set from the target
 * URL, so it sails through hostGuard. Such a POST could close sessions — which
 * releases parked ui_wait calls as "closed" — or spawn widget sessions.
 *
 * The browser does always attach `Origin` to cross-origin POSTs (and to
 * same-origin ones), which gives the same policy the WebSocket handshake uses:
 * an Origin that is present but not ours is rejected; an absent Origin is
 * allowed, because only non-browser clients (curl, tests) can omit it and they
 * are not a drive-by vector. Reads stay unguarded — GETs are side-effect-free
 * here and cross-origin pages cannot read the responses without CORS anyway.
 */
export function crossOriginWriteGuard(req: Request, res: Response, next: NextFunction): void {
  if (req.method === "GET" || req.method === "HEAD" || req.method === "OPTIONS") {
    next();
    return;
  }
  const origin = req.headers.origin;
  if (origin === undefined || TRUSTED_ORIGINS.has(origin)) {
    next();
    return;
  }
  log.warn("rejected cross-origin write", { origin, method: req.method, path: req.path });
  res.status(403).type("text/plain").send("AgentUI only accepts writes from its own pages.\n");
}
