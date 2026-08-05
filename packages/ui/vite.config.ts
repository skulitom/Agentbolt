import { fileURLToPath, URL } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const SERVER = "http://127.0.0.1:4517";

// The shell is served from / and routes client-side off location.pathname,
// so `base` stays "/" and every non-asset path falls through to index.html.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@agentui/shared": fileURLToPath(new URL("../shared/src/index.ts", import.meta.url)),
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": SERVER,
      // No "/p": preview modules are served by the PREVIEW origin (shell port
      // + 1), which the frame reaches directly via `previewOrigin` from
      // /api/session/:id. Proxying it here would only recreate the shell-origin
      // copy that was removed for lacking the preview origin's CSP.
      "/vendor": SERVER,
      "/ws": { target: SERVER, ws: true },
    },
  },
});
