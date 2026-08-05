import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import {
  accessMode,
  accessToken,
  bindHost,
  describeExposure,
  exposureBanner,
  isLoopbackRequest,
  readCookie,
  resetAccessTokenCache,
  tokenMatches,
  upgradeIsAllowed,
} from "../src/web/access.js";

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

/**
 * Reaching the widgets from a phone means the socket stops answering only to
 * loopback — and loopback was doing ALL the security work, because there is no
 * login anywhere else in this server.
 *
 * So the tests that matter here are the negative ones. Someone on the same wifi
 * who gets in can read whatever the agent attached (diffs, file contents,
 * screenshots) and, worse, ANSWER for you: their decision arrives at your agent
 * as if it were yours.
 */
let dataDir: string;

beforeEach(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-access-"));
  process.env["AGENT_UI_DATA_DIR"] = dataDir;
  delete process.env["AGENT_UI_HOST"];
  resetAccessTokenCache();
});

afterEach(async () => {
  delete process.env["AGENT_UI_DATA_DIR"];
  delete process.env["AGENT_UI_HOST"];
  resetAccessTokenCache();
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

describe("it is off unless asked for", () => {
  it("defaults to loopback, so nothing changes for anyone who did not opt in", () => {
    expect(accessMode()).toBe("loopback");
    expect(bindHost()).toBe("127.0.0.1");
    expect(describeExposure(4520).mode).toBe("loopback");
    expect(exposureBanner(4520)).toEqual([]);
  });

  it("binds every interface rather than only the LAN one, so loopback keeps working", () => {
    process.env["AGENT_UI_HOST"] = "lan";
    // Turning this on must ADD reachability, not move it — every existing
    // 127.0.0.1 URL and every test that assumes one has to keep working.
    expect(bindHost()).toBe("0.0.0.0");
  });

  it("ignores a value it does not understand instead of guessing", () => {
    process.env["AGENT_UI_HOST"] = "yes-please";
    expect(accessMode()).toBe("loopback");
  });
});

describe("the token", () => {
  it("survives a restart, so a link sent to a phone keeps working", async () => {
    const first = accessToken();
    resetAccessTokenCache();
    expect(accessToken()).toBe(first);
    // And it is actually on disk, not just memoised.
    const onDisk = await fsp.readFile(path.join(dataDir, "access-token"), "utf8");
    expect(onDisk.trim()).toBe(first);
  });

  it("avoids look-alike characters, because it gets typed on a phone", () => {
    expect(accessToken()).toMatch(/^[abcdefghjkmnpqrstuvwxyz23456789]{12}$/);
  });

  it("rejects a near miss, a prefix and an empty presentation", () => {
    const token = accessToken();
    expect(tokenMatches(token, token)).toBe(true);
    expect(tokenMatches(token.slice(0, -1) + "z", token)).toBe(false);
    expect(tokenMatches(token.slice(0, 6), token)).toBe(false);
    expect(tokenMatches("", token)).toBe(false);
    expect(tokenMatches(undefined, token)).toBe(false);
  });
});

describe("who needs a key", () => {
  it("does not ask loopback for one", () => {
    for (const host of ["127.0.0.1:4520", "localhost:4520", "[::1]:4520"]) {
      expect(isLoopbackRequest(host), host).toBe(true);
      expect(upgradeIsAllowed(host, undefined), host).toBe(true);
    }
  });

  it("refuses a network address outright when access was never switched on", () => {
    // Belt and braces: we did not bind a routable address, so this should be
    // unreachable — but a future misconfiguration must fail closed.
    expect(isLoopbackRequest("192.168.1.5:4520")).toBe(false);
    expect(upgradeIsAllowed("192.168.1.5:4520", `agentui_key=${accessToken()}`)).toBe(false);
  });

  it("refuses a network websocket with no cookie, a wrong one, or another key's", () => {
    process.env["AGENT_UI_HOST"] = "lan";
    const host = "192.168.1.5:4520";
    expect(upgradeIsAllowed(host, undefined)).toBe(false);
    expect(upgradeIsAllowed(host, "agentui_key=nope")).toBe(false);
    expect(upgradeIsAllowed(host, "other=x; agentui_key=wrongwrongwro")).toBe(false);
  });

  it("accepts a network websocket carrying the real key", () => {
    process.env["AGENT_UI_HOST"] = "lan";
    expect(upgradeIsAllowed("192.168.1.5:4520", `agentui_key=${accessToken()}`)).toBe(true);
    // ...including alongside other cookies, which is the normal case.
    expect(upgradeIsAllowed("192.168.1.5:4520", `theme=dark; agentui_key=${accessToken()}; x=1`)).toBe(true);
  });
});

describe("cookie parsing", () => {
  it("finds a value among others and tolerates spacing", () => {
    expect(readCookie("a=1; agentui_key=abc; b=2", "agentui_key")).toBe("abc");
    expect(readCookie("agentui_key=abc", "agentui_key")).toBe("abc");
    expect(readCookie(undefined, "agentui_key")).toBeUndefined();
  });

  it("does not match a cookie whose name merely ends the same way", () => {
    // `not_agentui_key` must not satisfy a lookup for `agentui_key`.
    expect(readCookie("not_agentui_key=abc", "agentui_key")).toBeUndefined();
  });
});

describe("what the user is told", () => {
  it("points at the inbox, so nobody types a ten-character session id", () => {
    process.env["AGENT_UI_HOST"] = "lan";
    const exposure = describeExposure(4520);
    if (!exposure.pairingUrl) return; // CI box with no non-internal IPv4
    expect(exposure.pairingUrl).toMatch(/^http:\/\/\d+\.\d+\.\d+\.\d+:4520\/\?k=[a-z2-9]{12}$/);
  });

  it("says out loud that this is plain HTTP and what that means", () => {
    process.env["AGENT_UI_HOST"] = "lan";
    const banner = exposureBanner(4520).join("\n");
    expect(banner).toMatch(/PLAIN HTTP/);
    expect(banner).toMatch(/answer them as you/);
    expect(banner).toMatch(/not on cafe or hotel wifi/i);
  });
});

/**
 * The trusted-origin set is what the cross-origin write guard checks against,
 * so widening it is not a small thing.
 *
 * The first version of this widened it whenever the MACHINE had a LAN address,
 * rather than when the SERVER had agreed to answer the network — which handed
 * every install a larger trust set to buy a feature almost none of them had
 * switched on. ports.test.ts caught it; this pins it.
 */
describe("the trusted origin set", () => {
  it("stays loopback-only until access is switched on", async () => {
    const { candidateOriginsAllProjects } = await import("../src/core/ports.js");
    expect(candidateOriginsAllProjects().every((o) => o.includes("127.0.0.1"))).toBe(true);
  });

  it("admits this machine's own addresses once it is", async () => {
    process.env["AGENT_UI_HOST"] = "lan";
    const { candidateOriginsAllProjects, lanAddresses } = await import("../src/core/ports.js").then(async (ports) => ({
      ...ports,
      lanAddresses: (await import("../src/web/access.js")).lanAddresses,
    }));
    const addresses = lanAddresses();
    if (!addresses.length) return; // CI box with no non-internal IPv4
    const origins = candidateOriginsAllProjects();
    expect(origins.some((o) => o.includes(addresses[0]!))).toBe(true);
    // ...and never at the cost of loopback, which is still the normal path.
    expect(origins.some((o) => o.includes("127.0.0.1"))).toBe(true);
  });
});

/**
 * Which address to advertise.
 *
 * The first version took the first non-internal IPv4 and confidently printed
 * 10.5.0.2 — a NordVPN tunnel — on a machine whose real LAN address was
 * 192.168.1.112. A phone would never have reached it, and the failure reads as
 * "this feature is broken" rather than "it picked the wrong card".
 */
describe("picking an address a phone can actually dial", () => {
  it("pins an explicit address, and treats giving one as the opt-in", async () => {
    process.env["AGENT_UI_HOST"] = "192.168.1.112";
    const { accessMode, lanAddresses, describeExposure } = await import("../src/web/access.js");
    expect(accessMode()).toBe("lan");
    expect(lanAddresses()[0]).toBe("192.168.1.112");
    expect(describeExposure(4520).pairingUrl).toContain("192.168.1.112");
  });

  it("pins an address even when this machine does not report it", async () => {
    // The escape hatch has to work on the machine that needs it — which is
    // exactly the one whose interfaces we are reading wrongly.
    process.env["AGENT_UI_HOST"] = "10.99.99.99";
    const { lanAddresses } = await import("../src/web/access.js");
    expect(lanAddresses()[0]).toBe("10.99.99.99");
  });

  it("never treats 127.0.0.1 as an opt-in", async () => {
    process.env["AGENT_UI_HOST"] = "127.0.0.1";
    const { accessMode } = await import("../src/web/access.js");
    expect(accessMode()).toBe("loopback");
  });
});
