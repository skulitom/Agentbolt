import os from "node:os";
import path from "node:path";
import fs from "node:fs";
import type { NextFunction, Request, Response } from "express";
import { customAlphabet } from "nanoid";
import { dataDir } from "../core/paths.js";
import { log } from "../log.js";

/**
 * Reaching the widgets from another device on your network.
 *
 * Everything else in this server assumes loopback means you. That assumption is
 * doing ALL the security work — there is no login, because until now there did
 * not need to be one. The moment the socket answers to anything but 127.0.0.1
 * that assumption is gone, and someone on the same wifi can read whatever the
 * agent put in a widget (diffs, file contents, screenshots) and, worse, ANSWER
 * for you: a decision they made arrives at your agent as if it were yours.
 *
 * So this is off unless asked for, and when asked for it carries a token.
 *
 * What it deliberately is NOT: encrypted. Traffic is plain HTTP, because a
 * self-signed certificate on a LAN address produces a full-page security
 * warning on exactly the phone this exists to serve. That is a fine trade on a
 * home network and a bad one on cafe or hotel wifi, and `describeExposure()`
 * says so out loud rather than leaving it to be discovered.
 */

/** Off, or bound to every interface so loopback keeps working unchanged. */
export type AccessMode = "loopback" | "lan";

const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

/** The raw opt-in: "lan" to auto-detect, or an explicit address to pin. */
function requestedHost(): string {
  return (process.env["AGENT_UI_HOST"] ?? "").trim().toLowerCase();
}

export function accessMode(): AccessMode {
  const raw = requestedHost();
  if (raw === "lan" || raw === "0.0.0.0") return "lan";
  // An explicit address is also an opt-in, and it is the escape hatch for when
  // the ranking below guesses wrong — which it will, on a machine with a VPN.
  return IPV4.test(raw) && raw !== "127.0.0.1" ? "lan" : "loopback";
}

/**
 * What to pass to `server.listen`.
 *
 * "0.0.0.0" rather than the LAN address specifically, so that turning this on
 * ADDS reachability instead of moving it — every existing 127.0.0.1 URL, and
 * every test that assumes one, keeps working untouched.
 */
export function bindHost(): string {
  return accessMode() === "lan" ? "0.0.0.0" : "127.0.0.1";
}

/**
 * The address a phone should actually dial.
 *
 * Picks the first non-internal IPv4. A machine with several (docker, VPN, a
 * second NIC) can have more than one plausible answer and this cannot know
 * which network the phone is on, so `lanAddresses()` returns all of them and
 * the startup line prints them; this one is just the best guess for the URL.
 */
/**
 * Interfaces that are technically "not internal" but are not the network your
 * phone is on: VPN tunnels, hypervisor switches, container bridges.
 *
 * This matters more than it sounds. The first version of this took the first
 * non-internal IPv4 and confidently advertised 10.5.0.2 — a NordVPN tunnel —
 * on a machine whose actual LAN address was 192.168.1.112. A phone would never
 * have reached it, and the failure looks like "the feature is broken" rather
 * than "it picked the wrong card".
 */
const NOT_A_LAN = /vpn|nordlynx|wireguard|wg\d|tailscale|zerotier|hamachi|tap-|tun\d|ppp|utun|vethernet|hyper-v|wsl|docker|virtualbox|vmware|vboxnet|bridge\d|loopback/i;

/** 192.168 first: it is what home routers hand out, and this is for phones. */
function rank(address: string, interfaceName: string): number {
  let score = 0;
  if (NOT_A_LAN.test(interfaceName)) score += 100;
  if (address.startsWith("192.168.")) score -= 10;
  else if (/^172\.(1[6-9]|2\d|3[01])\./.test(address)) score -= 5;
  else if (address.startsWith("10.")) score -= 2;
  // Link-local is what you get when DHCP failed; never the best answer.
  if (address.startsWith("169.254.")) score += 50;
  return score;
}

/**
 * Every address a phone could plausibly dial, best guess first.
 *
 * Returns all of them rather than one, because this cannot know which network
 * the phone is on — so the startup banner prints the alternatives and
 * AGENT_UI_HOST=<address> pins one when the guess is wrong.
 */
export function lanAddresses(): string[] {
  const found: Array<{ address: string; score: number }> = [];
  for (const [name, entries] of Object.entries(os.networkInterfaces())) {
    for (const entry of entries ?? []) {
      // `family` is the string "IPv4" on modern Node and the number 4 on some
      // older builds; accept both rather than depend on which one is running.
      const isV4 = entry.family === "IPv4" || (entry.family as unknown as number) === 4;
      if (isV4 && !entry.internal) found.push({ address: entry.address, score: rank(entry.address, name) });
    }
  }
  found.sort((a, b) => a.score - b.score);
  const ordered = found.map((entry) => entry.address);

  // An explicitly pinned address wins outright, even one this did not find —
  // the whole point of pinning is that the machine knows something we do not.
  const pinned = requestedHost();
  if (IPV4.test(pinned)) return [pinned, ...ordered.filter((address) => address !== pinned)];
  return ordered;
}

export function lanAddress(): string | undefined {
  return lanAddresses()[0];
}

/* ------------------------------------------------------------------ token */

// No look-alikes: this gets typed on a phone keyboard at least once, and
// "was that a 1 or an l" is a bad first experience of a security feature.
const newToken = customAlphabet("abcdefghjkmnpqrstuvwxyz23456789", 12);

export const TOKEN_COOKIE = "agentui_key";
export const TOKEN_QUERY = "k";
export const TOKEN_HEADER = "x-agentui-key";

let cached: string | undefined;

/**
 * Stable across restarts, so a URL you sent to your phone keeps working and can
 * be bookmarked. Regenerating per process would mean re-pairing every time the
 * agent restarts, which is often.
 */
export function accessToken(): string {
  if (cached) return cached;
  const file = path.join(dataDir(), "access-token");
  try {
    const existing = fs.readFileSync(file, "utf8").trim();
    if (existing) return (cached = existing);
  } catch {
    /* first run */
  }
  const token = newToken();
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    // 0600: the token is the only thing standing between the network and your
    // widgets, and the data dir is not otherwise secret.
    fs.writeFileSync(file, `${token}\n`, { mode: 0o600 });
  } catch (error) {
    log.warn("could not persist the access token; it will change on restart", { error: String(error) });
  }
  return (cached = token);
}

/** Test seam: forget the cached token so a fresh data dir is picked up. */
export function resetAccessTokenCache(): void {
  cached = undefined;
}

const LOOPBACK = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);

function hostnameOf(hostHeader: string | undefined): string | undefined {
  if (!hostHeader) return undefined;
  const match = /^(\[[^\]]+\]|[^:]+)(?::\d+)?$/.exec(hostHeader.trim());
  return match?.[1]?.toLowerCase();
}

/** Addressed to this machine BY loopback — the case that needs no token. */
export function isLoopbackRequest(hostHeader: string | undefined): boolean {
  const hostname = hostnameOf(hostHeader);
  return Boolean(hostname && LOOPBACK.has(hostname));
}

function presentedToken(req: Request): string | undefined {
  const query = req.query?.[TOKEN_QUERY];
  if (typeof query === "string" && query) return query;

  const header = req.headers[TOKEN_HEADER];
  if (typeof header === "string" && header) return header;

  return readCookie(req.headers.cookie, TOKEN_COOKIE);
}

export function readCookie(header: string | undefined, name: string): string | undefined {
  if (!header) return undefined;
  for (const part of header.split(";")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    if (part.slice(0, eq).trim() === name) return decodeURIComponent(part.slice(eq + 1).trim());
  }
  return undefined;
}

/**
 * Constant-time-ish compare. The token is not a password and an attacker on
 * your LAN has better options, but comparing secrets with `===` in a request
 * handler is the kind of thing that is embarrassing to explain later.
 */
export function tokenMatches(presented: string | undefined, expected: string): boolean {
  if (!presented || presented.length !== expected.length) return false;
  let diff = 0;
  for (let i = 0; i < expected.length; i++) diff |= presented.charCodeAt(i) ^ expected.charCodeAt(i);
  return diff === 0;
}

/**
 * The gate for anything not arriving over loopback.
 *
 * Loopback returns immediately, so with LAN access off this middleware is a
 * single Set lookup and the server behaves exactly as it did before.
 */
export function accessGuard(req: Request, res: Response, next: NextFunction): void {
  if (isLoopbackRequest(req.headers.host)) {
    next();
    return;
  }

  if (accessMode() !== "lan") {
    // Unreachable in practice — we did not bind a routable address — but a
    // future misconfiguration should fail closed rather than fall through.
    res.status(403).type("text/plain").send("AgentUI is not accepting connections from the network.\n");
    return;
  }

  if (!tokenMatches(presentedToken(req), accessToken())) {
    log.warn("rejected network request without a valid key", { host: req.headers.host, path: req.path });
    res
      .status(401)
      .type("text/html")
      .send(
        `<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1">` +
          `<body style="font:16px/1.5 system-ui;padding:2rem;max-width:32rem">` +
          `<h1 style="font-size:1.2rem">This link needs its key</h1>` +
          `<p>Open the address AgentUI printed when it started — it ends in <code>?k=…</code>.</p>` +
          `</body>`,
      );
    return;
  }

  // Re-issued on every accepted request so the key survives in the browser
  // without having to stay in the URL — assets, the WebSocket upgrade and any
  // link the user taps all carry it from here on.
  //
  // Not `Secure`, because this is plain HTTP by design (see the header). Cookies
  // are scoped by host and NOT by port, which is what makes one pairing also
  // cover the preview origin on port + 1.
  res.cookie?.(TOKEN_COOKIE, accessToken(), {
    httpOnly: true,
    sameSite: "lax",
    path: "/",
    maxAge: 30 * 24 * 60 * 60 * 1000,
  });
  next();
}

/** Same check, for the WebSocket upgrade, which is not an express request. */
export function upgradeIsAllowed(hostHeader: string | undefined, cookieHeader: string | undefined): boolean {
  if (isLoopbackRequest(hostHeader)) return true;
  if (accessMode() !== "lan") return false;
  return tokenMatches(readCookie(cookieHeader, TOKEN_COOKIE), accessToken());
}

/* ---------------------------------------------------------------- surface */

export interface Exposure {
  mode: AccessMode;
  addresses: string[];
  /** The one URL to type on a phone. Undefined when access is loopback-only. */
  pairingUrl?: string;
}

export function describeExposure(port: number): Exposure {
  if (accessMode() !== "lan") return { mode: "loopback", addresses: [] };
  const addresses = lanAddresses();
  const first = addresses[0];
  return {
    mode: "lan",
    addresses,
    // The INBOX, not a session: it lists whatever is open, so nobody has to
    // type a ten-character session id on a phone keyboard.
    ...(first ? { pairingUrl: `http://${first}:${port}/?${TOKEN_QUERY}=${accessToken()}` } : {}),
  };
}

/** The startup banner. Says the quiet part, because the trade is real. */
export function exposureBanner(port: number): string[] {
  const exposure = describeExposure(port);
  if (exposure.mode !== "lan") return [];
  if (!exposure.pairingUrl) {
    return ["AGENT_UI_HOST=lan is set, but this machine has no non-internal IPv4 address to advertise."];
  }
  return [
    "Open on another device on this network:",
    `  ${exposure.pairingUrl}`,
    ...(exposure.addresses.length > 1
      ? [`  (other addresses on this machine: ${exposure.addresses.slice(1).join(", ")})`]
      : []),
    "",
    "This is PLAIN HTTP and anyone on this network who has that key can read your",
    "widgets and answer them as you. Fine at home; not on cafe or hotel wifi.",
  ];
}
