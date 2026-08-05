import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { parseSpec, type InterfaceSpec } from "@agentui/shared";
import { Library } from "../src/core/library.js";

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

let root: string;
let library: Library;

function spec(title: string, ids: string[]): InterfaceSpec {
  const parsed = parseSpec({
    title,
    controls: ids.map((id) => ({ id, label: id, type: "slider", min: 0, max: 1, default: 0.5 })),
  });
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.spec;
}

beforeEach(async () => {
  root = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-lib-"));
  process.env["AGENT_UI_LIBRARY_DIR"] = path.join(root, "user");
  process.env["AGENT_UI_DATA_DIR"] = path.join(root, "project");
  library = new Library();
});

afterEach(async () => {
  delete process.env["AGENT_UI_LIBRARY_DIR"];
  delete process.env["AGENT_UI_DATA_DIR"];
  await fsp.rm(root, { recursive: true, force: true }).catch(() => {});
});

async function save(slug: string, name: string, extra: Partial<Parameters<Library["save"]>[0]> = {}) {
  return library.save({
    slug,
    name,
    spec: spec(name, ["intensity", "duration"]),
    scope: "user",
    overwrite: false,
    ...extra,
  });
}

describe("library", () => {
  it("saves and loads a widget", async () => {
    await save("muzzle-flash", "Muzzle flash tuning", { description: "VFX timing", triggers: ["vfx"] });
    const found = await library.load("muzzle-flash");
    expect(found?.widget.name).toBe("Muzzle flash tuning");
    expect(found?.scope).toBe("user");
  });

  it("refuses to clobber an existing widget, and suggests a free name", async () => {
    await save("muzzle-flash", "First");
    await expect(save("muzzle-flash", "Second")).rejects.toThrow(/already exists/);
    expect(await library.freeSlug("muzzle-flash")).toBe("muzzle-flash-2");
  });

  it("overwrites when asked, keeping use history", async () => {
    await save("w", "First");
    await library.recordUse("w");
    await save("w", "Second", { overwrite: true });
    const found = await library.load("w");
    expect(found?.widget.name).toBe("Second");
    expect(found?.widget.uses).toBe(1);
  });

  it("remembers where the user left it, so reuse resumes rather than restarts", async () => {
    await save("w", "W");
    await library.recordUse("w", { intensity: 0.87 });
    const found = await library.load("w");
    expect(found?.widget.lastValues).toEqual({ intensity: 0.87 });
    expect(found?.widget.uses).toBe(1);
    expect(found?.widget.lastUsedAt).toBeTruthy();
  });

  it("finds widgets by name, tag and trigger phrase", async () => {
    await save("muzzle-flash", "Muzzle flash tuning", { description: "VFX timing", tags: ["vfx"], triggers: ["gun fire"] });
    await save("palette-picker", "Palette picker", { description: "colour schemes" });

    expect((await library.search("muzzle")).results[0]?.slug).toBe("muzzle-flash");
    expect((await library.search("vfx")).results[0]?.slug).toBe("muzzle-flash");
    expect((await library.search("gun fire")).results[0]?.slug).toBe("muzzle-flash");
    expect((await library.search("colour")).results[0]?.slug).toBe("palette-picker");
  });

  it("tolerates near-misses rather than requiring exact words", async () => {
    await save("muzzle-flash", "Muzzle flash tuning", { triggers: ["muzzle flash"] });
    expect((await library.search("muzzleflash")).results[0]?.slug).toBe("muzzle-flash");
  });

  it("flags a hand-authored spec that duplicates a saved one", async () => {
    await save("muzzle-flash", "Muzzle flash tuning");
    // Same controls, different title — an agent rebuilding what it already has.
    const similar = await library.findSimilar(spec("Gun flash settings", ["intensity", "duration"]));
    expect(similar?.summary.slug).toBe("muzzle-flash");
  });

  it("does not flag something genuinely different", async () => {
    await save("muzzle-flash", "Muzzle flash tuning");
    expect(await library.findSimilar(spec("Deploy approval", ["region", "replicas"]))).toBeUndefined();
  });

  it("caps the agent-facing index so it cannot bloat every prompt", async () => {
    for (let i = 0; i < 20; i++) await save(`widget-${i}`, `Widget ${i}`, { description: `number ${i}` });
    const index = await library.indexForToolDescription(5);
    const lines = index.split("\n").filter((l) => l.startsWith("  ") && !l.includes("more —"));
    expect(lines).toHaveLength(5);
    expect(index).toContain("15 more");
  });

  it("ranks pinned widgets into the index ahead of unused ones", async () => {
    for (let i = 0; i < 5; i++) await save(`widget-${i}`, `Widget ${i}`);
    await save("obscure", "Obscure thing");
    await library.setPinned("obscure", true);

    const index = await library.indexForToolDescription(2);
    expect(index.split("\n")[1]).toContain("obscure");
  });

  it("returns an empty index when nothing is saved, adding nothing to the prompt", async () => {
    expect(await library.indexForToolDescription()).toBe("");
  });

  it("lets a project widget shadow a global one of the same name", async () => {
    await save("shared", "Global version");
    await library.save({ slug: "shared", name: "Project version", spec: spec("p", ["a"]), scope: "project", overwrite: false });
    const found = await library.load("shared");
    expect(found?.widget.name).toBe("Project version");
    expect(found?.scope).toBe("project");
  });

  it("skips a corrupt widget instead of breaking the whole library", async () => {
    await save("good", "Good");
    const badDir = path.join(root, "user", "bad");
    await fsp.mkdir(badDir, { recursive: true });
    await fsp.writeFile(path.join(badDir, "widget.json"), "{ not valid json", "utf8");

    const all = await library.all();
    expect(all.map((w) => w.widget.slug)).toEqual(["good"]);
  });

  it("deletes a widget", async () => {
    await save("temp", "Temp");
    expect(await library.remove("temp")).toBe(true);
    expect(await library.load("temp")).toBeUndefined();
  });
});
