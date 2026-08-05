import { describe, expect, it } from "vitest";
import { applyTransform, collectPlaceholders, interpolate } from "../src/bind.js";
import { parseSpec, type Control } from "../src/spec.js";

function controls(input: unknown[]): Control[] {
  const parsed = parseSpec({ title: "T", controls: input });
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.spec.controls;
}

const pkg = controls([{ id: "pkg", label: "Package", type: "text", default: "agentui-mcp" }]);

describe("collectPlaceholders", () => {
  it("finds ids and transforms", () => {
    expect(collectPlaceholders("import x from {{pkg}}; new {{pkg:pascal}}()")).toEqual([
      { raw: "{{pkg}}", id: "pkg", transform: undefined },
      { raw: "{{pkg:pascal}}", id: "pkg", transform: "pascal" },
    ]);
  });

  it("tolerates inner whitespace", () => {
    expect(collectPlaceholders("{{ pkg }}")[0]?.id).toBe("pkg");
  });

  it("collects an unknown transform, so validation can name it", () => {
    expect(collectPlaceholders("{{pkg:shouty}}")[0]?.transform).toBe("shouty");
  });
});

/**
 * The reason there is no escape syntax: everything that is not unmistakably a
 * placeholder is left alone, so the templating languages an agent is likely to
 * be showing you survive untouched. An escape hatch nobody uses correctly is
 * the single biggest source of breakage in systems like this.
 */
describe("interpolate leaves other brace syntaxes alone", () => {
  for (const text of [
    "${{ github.token }}",
    "{{#each items}}",
    "{{ .Values.image }}",
    "{{ user | upper }}",
    '{{ template "chart.name" . }}',
    "{{}}",
    "{ pkg }",
  ]) {
    it(text, () => {
      expect(interpolate(text, pkg, { pkg: "agentui" })).toBe(text);
    });
  }

  it("leaves a placeholder naming no control exactly as written", () => {
    expect(interpolate("{{other}}", pkg, { pkg: "agentui" })).toBe("{{other}}");
  });

  it("leaves an unknown transform alone rather than guessing", () => {
    expect(interpolate("{{pkg:shouty}}", pkg, { pkg: "agentui" })).toBe("{{pkg:shouty}}");
  });
});

describe("interpolate", () => {
  it("substitutes the live value, and falls back to the default", () => {
    expect(interpolate('import "{{pkg}}"', pkg, { pkg: "ask-human" })).toBe('import "ask-human"');
    expect(interpolate('import "{{pkg}}"', pkg, {})).toBe('import "agentui-mcp"');
  });

  it("writes a number bare, so the unit lives in the template where you can see it", () => {
    const ms = controls([{ id: "ms", label: "Timeout", type: "slider", min: 0, max: 9000, default: 3000 }]);
    expect(interpolate("timeout: {{ms}}ms", ms, { ms: 250 })).toBe("timeout: 250ms");
  });

  it("gives a select's VALUE, not its label — that is what makes option values be the snippet", () => {
    const hint = controls([
      {
        id: "hint",
        label: "Follow-up",
        type: "select",
        options: [
          { value: "", label: "no follow-up" },
          { value: "  Re-run with --force.", label: "suggest --force" },
        ],
        default: "",
      },
    ]);
    expect(interpolate("x\n{{hint}}", hint, { hint: "  Re-run with --force." })).toBe("x\n  Re-run with --force.");
    expect(interpolate("x\n{{hint}}", hint, { hint: "" })).toBe("x\n");
  });

  it("refuses to render a list or a boolean, leaving the placeholder visible", () => {
    const flag = controls([{ id: "flag", label: "Flag", type: "toggle", default: true }]);
    expect(interpolate("{{flag}}", flag, { flag: true })).toBe("{{flag}}");
  });
});

describe("transforms", () => {
  it("re-cases across separators and camelCase humps", () => {
    expect(applyTransform("agentui-mcp", "pascal")).toBe("AgentuiMcp");
    expect(applyTransform("agentui-mcp", "camel")).toBe("agentuiMcp");
    expect(applyTransform("agentui-mcp", "kebab")).toBe("agentui-mcp");
    expect(applyTransform("ask human", "kebab")).toBe("ask-human");
    expect(applyTransform("askHumanNow", "kebab")).toBe("ask-human-now");
    expect(applyTransform("ask_human", "pascal")).toBe("AskHuman");
  });

  it("hands back anything with no word characters unchanged", () => {
    expect(applyTransform("---", "pascal")).toBe("---");
  });
});
