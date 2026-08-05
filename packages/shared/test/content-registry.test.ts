import { describe, expect, it } from "vitest";
import { CONTENT_BLOCKS, CONTENT_KINDS, ContentBlock, contentBehaviourFor, type ContentKind } from "../src/spec/content-blocks.js";
import { parseSpec } from "../src/spec.js";

/**
 * Same contract as the control registry, for the five content kinds. The two
 * behaviours worth pinning hardest are the ones that used to be hand-coded
 * outside this file entirely:
 *
 *   - `localPath`, which was `block.kind === "image" && block.path` inline in
 *     session-manager — a second path-carrying kind would have been skipped,
 *     and its asset silently never copied
 *   - `needsLookingAt`, which was `block.kind !== "markdown" || block.bind`
 *     inline in the inbox — a fourth independent answer to "is this visual"
 */

const FIXTURES: { [K in ContentKind]: Record<string, unknown> } = {
  markdown: { kind: "markdown", value: "just prose" },
  code: { kind: "code", value: "const x = 1;", language: "ts" },
  diff: { kind: "diff", value: "@@ -1 +1 @@\n-a\n+b" },
  sample: {
    kind: "sample",
    label: "Sessions",
    source: "read just now",
    rule: "idle_h > stale_hours",
    rows: [
      { label: "a", idle_h: 1 },
      { label: "b", idle_h: 9 },
    ],
  },
  image: { kind: "image", path: "C:/tmp/shot.png" },
};

/** sample's rule names a control, so it needs one to parse. */
function blockFor(kind: ContentKind) {
  const parsed = parseSpec({
    title: "T",
    content: [FIXTURES[kind]],
    controls: [{ id: "stale_hours", label: "Stale after", type: "number", default: 4 }],
  });
  if (!parsed.ok) throw new Error(`${kind} fixture does not parse:\n${parsed.message}`);
  return parsed.spec.content[0]!;
}

describe("the content registry is total over the union", () => {
  it("has an entry for every kind in the ContentBlock union", () => {
    const declared = ContentBlock.options.map((option) => option.shape.kind.value).sort();
    expect(Object.keys(CONTENT_BLOCKS).sort()).toEqual(declared);
  });

  it("covers every kind in this file's fixture table", () => {
    expect(Object.keys(FIXTURES).sort()).toEqual([...CONTENT_KINDS].sort());
  });

  it("no longer has an html kind, which never rendered", () => {
    expect(CONTENT_KINDS).not.toContain("html");
  });
});

describe("localPath finds every asset the server must copy", () => {
  it("reports an image's path", () => {
    const block = blockFor("image");
    expect(contentBehaviourFor(block).localPath?.(block)).toBe("C:/tmp/shot.png");
  });

  it("reports nothing for a dataUri image, which needs no copying", () => {
    const parsed = parseSpec({
      title: "T",
      content: [{ kind: "image", dataUri: "data:image/png;base64,iVBORw0KGgo=" }],
    });
    if (!parsed.ok) throw new Error(parsed.message);
    const block = parsed.spec.content[0]!;
    expect(contentBehaviourFor(block).localPath?.(block)).toBeUndefined();
  });

  it("is declared by exactly the kinds that carry a path", () => {
    // The guard against a future path-carrying kind being added and quietly
    // missed by the asset copier.
    const withPath = CONTENT_KINDS.filter((kind) => CONTENT_BLOCKS[kind].localPath !== undefined);
    expect(withPath).toEqual(["image"]);
  });

  it("pairs localPath with withLocalPath, so a findable asset is a rewritable one", () => {
    // Declaring one without the other would mean the copier knows how to FIND
    // an asset it then cannot point at, and the block would keep its absolute
    // local path after the file had already been copied.
    for (const kind of CONTENT_KINDS) {
      const behaviour = CONTENT_BLOCKS[kind];
      expect(behaviour.localPath === undefined, kind).toBe(behaviour.withLocalPath === undefined);
    }
  });

  it("rewrites an image to the served URL and drops the absolute path", () => {
    const block = blockFor("image");
    const rewritten = contentBehaviourFor(block).withLocalPath?.(block, "/s/abc/assets/0.png");
    expect(rewritten).toEqual({ kind: "image", path: "/s/abc/assets/0.png" });
  });
});

describe("needsLookingAt is false only for prose", () => {
  it.each(CONTENT_KINDS)("%s", (kind) => {
    const block = blockFor(kind);
    expect(contentBehaviourFor(block).needsLookingAt(block)).toBe(kind !== "markdown");
  });

  it("flips to true for markdown once it is bound, because the text now moves", () => {
    const parsed = parseSpec({
      title: "T",
      content: [{ kind: "markdown", bind: true, value: "install {{pkg}}" }],
      controls: [{ id: "pkg", label: "Package", type: "text", default: "agentui" }],
    });
    if (!parsed.ok) throw new Error(parsed.message);
    const block = parsed.spec.content[0]!;
    expect(contentBehaviourFor(block).needsLookingAt(block)).toBe(true);
  });
});

describe("illustrates reports the controls a block draws", () => {
  it("names the control a sample rule points at", () => {
    const block = blockFor("sample");
    expect(contentBehaviourFor(block).illustrates?.(block)).toEqual(["stale_hours"]);
  });

  it("names the controls a bound block interpolates", () => {
    const parsed = parseSpec({
      title: "T",
      content: [{ kind: "code", bind: true, value: 'import "{{pkg}}" // {{other}}' }],
      controls: [
        { id: "pkg", label: "Package", type: "text", default: "agentui" },
        { id: "other", label: "Other", type: "text", default: "" },
      ],
    });
    if (!parsed.ok) throw new Error(parsed.message);
    const block = parsed.spec.content[0]!;
    expect(contentBehaviourFor(block).illustrates?.(block)).toEqual(["pkg", "other"]);
  });

  it("names nothing for an UNBOUND block, where braces are literal", () => {
    const parsed = parseSpec({
      title: "T",
      content: [{ kind: "markdown", value: "{{ .Values.image }} stays as written" }],
      controls: [{ id: "image", label: "Image", type: "text", default: "" }],
    });
    if (!parsed.ok) throw new Error(parsed.message);
    const block = parsed.spec.content[0]!;
    expect(contentBehaviourFor(block).illustrates?.(block)).toEqual([]);
  });
});
