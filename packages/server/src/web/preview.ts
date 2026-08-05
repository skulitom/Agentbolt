import crypto from "node:crypto";
import path from "node:path";
import fsp from "node:fs/promises";
import express, { type Express } from "express";
import type { SessionManager } from "../core/session-manager.js";
import { webRoot } from "../core/paths.js";
import { isAllowedHostHeader } from "./security.js";
import { accessGuard } from "./access.js";
import { log } from "../log.js";

/**
 * The preview origin (port P+1).
 *
 * Everything an agent writes runs here, isolated from the shell by being a
 * genuinely different origin. Two controls do the real work:
 *
 *  - `sandbox="allow-scripts allow-same-origin"` on the parent's iframe. Sounds
 *    contradictory, but the pair is only dangerous when the frame shares the
 *    embedder's origin (it could then reach `frameElement` and strip its own
 *    sandbox). Cross-origin, `frameElement` is null. Meanwhile same-origin *to
 *    itself* is what lets `import()` and import maps work at all.
 *  - CSP `default-src 'none'` + `connect-src 'self'`. Against LLM-authored code
 *    that might be carrying a prompt injection, this — not the sandbox — is what
 *    prevents the module from phoning home with whatever it can see.
 */
export function createPreviewApp(sessions: SessionManager): Express {
  const app = express();
  app.disable("x-powered-by");

  app.use((req, res, next) => {
    if (!isAllowedHostHeader(req.headers.host)) {
      res.status(403).type("text/plain").send("AgentUI preview only serves this machine.\n");
      return;
    }
    next();
  });
  // A separate ORIGIN but the same HOST, and cookies are scoped by host rather
  // than by port — so the one pairing that unlocked the shell also unlocks
  // this, with no second key for the user to carry.
  app.use(accessGuard);

  // The agent's module. Never cached — ui_update rewrites it and the frame must
  // pick up the new one on reload.
  app.get("/p/:id.mjs", async (req, res) => {
    const session = sessions.get(String(req.params["id"]));
    const module = await session?.previewModule();
    res.setHeader("Content-Type", "text/javascript; charset=utf-8");
    res.setHeader("Cache-Control", "no-store");
    res.setHeader("Content-Security-Policy", "default-src 'none'");
    if (!module) {
      res.status(404).send("// no preview module for this session\n");
      return;
    }
    res.send(module);
  });

  app.use("/vendor", express.static(path.join(webRoot(), "vendor"), { immutable: true, maxAge: "1y" }));
  // Built-in preview helpers, importable as `agentui` via the import map.
  app.use("/agentui", express.static(path.join(webRoot(), "agentui"), { maxAge: "1h" }));

  app.get("/preview.html", async (_req, res) => {
    // A fresh nonce per response. Required, not optional: an inline import map
    // is a <script> element and is blocked by a strict script-src, and external
    // import maps do not exist — `src` is forbidden on them by spec.
    const nonce = crypto.randomBytes(24).toString("base64");
    res.setHeader(
      "Content-Security-Policy",
      [
        "default-src 'none'",
        `script-src 'self' 'nonce-${nonce}'`,
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' data: blob:",
        "font-src 'self' data:",
        "connect-src 'self'",
        "worker-src blob:",
        "frame-ancestors http://127.0.0.1:* http://localhost:*",
        "base-uri 'none'",
        "form-action 'none'",
      ].join("; "),
    );
    res.setHeader("Cache-Control", "no-store");
    res.type("text/html");

    try {
      const template = await fsp.readFile(path.join(webRoot(), "preview.html"), "utf8");
      res.send(template.replaceAll("__NONCE__", nonce));
    } catch (error) {
      log.error("preview.html missing from the build", error);
      res.status(500).send("<!doctype html><p>preview host page missing");
    }
  });

  app.use((_req, res) => res.status(404).type("text/plain").send("not found\n"));
  return app;
}
