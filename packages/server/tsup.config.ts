import { defineConfig } from "tsup";

// Two entry points: the MCP stdio server (what agents launch) and the CLI
// (`agentui install`). @agentui/shared is bundled in rather than published,
// which is why nothing here emits .d.ts files.
export default defineConfig({
  entry: {
    mcp: "src/mcp.ts",
    cli: "src/cli.ts",
  },
  format: ["esm"],
  target: "node20",
  platform: "node",
  outDir: "dist",
  clean: true,
  sourcemap: true,
  dts: false,
  // Both entries are bin targets that need a shebang, and esbuild applies the
  // banner to every output file — so no splitting, keeping each entry
  // self-contained rather than emitting shared chunks with stray shebangs.
  splitting: false,
  treeshake: true,
  noExternal: ["@agentui/shared"],
  banner: { js: "#!/usr/bin/env node" },
});
