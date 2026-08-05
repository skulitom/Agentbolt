/**
 * The spec used to be one 880-line file. It is now a directory, and this is a
 * re-export so `import … from "./spec.js"` keeps working everywhere — including
 * the eight test files that reach for it directly.
 *
 * Start at spec/index.ts for the layering.
 */
export * from "./spec/index.js";
