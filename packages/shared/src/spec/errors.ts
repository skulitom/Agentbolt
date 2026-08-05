import type { z } from "zod";

/**
 * Error reporting for spec validation.
 *
 * Separate from the schemas because everything here is about the MESSAGE, and
 * because `Ctx` has to be importable by the per-type validators without
 * dragging the whole `InterfaceSpec` in behind it — that would be a cycle.
 */

/**
 * Structural, so the per-type validators do not depend on zod's internal ctx
 * type. They receive this and can be called from a plain test with a stub.
 */
export interface Ctx {
  addIssue(issue: { code: "custom"; path: Array<string | number>; message: string }): void;
}

const DOC_HINT =
  'See the agentui skill for a worked example, or call ui_schema({ of: "control" | "content" | "view" | "spec" }) for the exact shape.';

/**
 * Turns zod issues into per-path one-liners a model can act on directly.
 * This is the difference between an agent self-correcting in one retry and
 * flailing through five.
 */
export function formatSpecError(error: z.ZodError): string {
  const lines = error.issues.map((issue) => {
    const path = issue.path.length ? formatPath(issue.path) : "(root)";
    return `  ${path}: ${issue.message}`;
  });
  const unique = [...new Set(lines)];
  return [`Invalid widget spec (${unique.length} problem${unique.length === 1 ? "" : "s"}):`, ...unique, DOC_HINT].join(
    "\n",
  );
}

function formatPath(path: ReadonlyArray<PropertyKey>): string {
  let out = "";
  for (const segment of path) {
    if (typeof segment === "number") out += `[${segment}]`;
    else out += out ? `.${String(segment)}` : String(segment);
  }
  return out;
}

/** Describes a received value in words, for "expected X, got Y" messages. */
export function describeValue(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return `an array of ${value.length}`;
  if (typeof value === "string") return `the string ${JSON.stringify(value)}`;
  return typeof value;
}
