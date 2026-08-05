import { describe, expect, it } from "vitest";
import type { NextFunction, Request, Response } from "express";
import { crossOriginWriteGuard, isAllowedHostHeader, trustOwnOrigins } from "../src/web/security.js";
import { PORT_BASE } from "../src/core/ports.js";

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

/**
 * The CSRF guard on mutating API routes.
 *
 * hostGuard cannot stop a cross-site form POST — the browser sets Host from the
 * *target* URL, so it always looks local. What a page cannot forge is Origin,
 * which browsers attach to every cross-origin POST. The guard's policy mirrors
 * the WebSocket handshake: foreign Origin rejected, our own or absent allowed.
 */

function run(method: string, origin?: string): { status: number | undefined; passed: boolean } {
  let status: number | undefined;
  let passed = false;

  const req = {
    method,
    path: "/api/sessions/stale/close",
    headers: origin === undefined ? {} : { origin },
  } as unknown as Request;

  const res = {
    status(code: number) {
      status = code;
      return this;
    },
    type() {
      return this;
    },
    send() {
      return this;
    },
  } as unknown as Response;

  crossOriginWriteGuard(req, res, (() => {
    passed = true;
  }) as NextFunction);

  return { status, passed };
}

describe("crossOriginWriteGuard", () => {
  it("allows writes from our own pages", () => {
    const { passed } = run("POST", `http://127.0.0.1:${PORT_BASE}`);
    expect(passed).toBe(true);
  });

  /**
   * AGENT_UI_PORT can bind us outside the derived range. The guard must still
   * accept our own page, or the shell would 403 against its own server.
   */
  it("trusts a port this instance actually bound, even outside the derived range", () => {
    expect(run("POST", "http://127.0.0.1:4517").passed).toBe(false);
    trustOwnOrigins(4517);
    expect(run("POST", "http://127.0.0.1:4517").passed).toBe(true);
  });

  it("allows any port in the derived range, since the shell may have moved", () => {
    const { passed } = run("POST", `http://127.0.0.1:${PORT_BASE + 6}`);
    expect(passed).toBe(true);
  });

  it("allows origin-less non-browser clients (curl, tests)", () => {
    const { passed } = run("POST");
    expect(passed).toBe(true);
  });

  it("rejects a cross-site form POST", () => {
    const { status, passed } = run("POST", "https://evil.example");
    expect(passed).toBe(false);
    expect(status).toBe(403);
  });

  it("rejects an opaque 'null' origin (sandboxed page, data: URL)", () => {
    const { status, passed } = run("POST", "null");
    expect(passed).toBe(false);
    expect(status).toBe(403);
  });

  it("rejects a DELETE from a foreign origin", () => {
    const { status, passed } = run("DELETE", "https://evil.example");
    expect(passed).toBe(false);
    expect(status).toBe(403);
  });

  it("leaves reads alone — GETs pass regardless of origin", () => {
    const { passed } = run("GET", "https://evil.example");
    expect(passed).toBe(true);
  });
});

describe("isAllowedHostHeader", () => {
  it("accepts localhost forms", () => {
    for (const host of ["127.0.0.1:4517", "localhost:4517", "localhost", "[::1]:4517"]) {
      expect(isAllowedHostHeader(host)).toBe(true);
    }
  });

  it("rejects rebound hostnames", () => {
    for (const host of ["evil.example", "evil.example:4517", "127.0.0.1.evil.example"]) {
      expect(isAllowedHostHeader(host)).toBe(false);
    }
  });
});
