import { roundForDisplay } from "@agentui/shared";

/**
 * Float maths leaks artefacts like 0.30000000000000004 into the readout. Shared
 * with the preview frame via vocab.js so one value never prints two ways on one
 * screen — the shell used to round to 4 decimals and the frame to 3.
 */
export const formatNumber = roundForDisplay;

export function truncate(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}
