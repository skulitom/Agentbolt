import { z } from "zod";
import { InterfaceSpec } from "./spec.js";
import { SessionEvent } from "./events.js";
import { SESSION_STATUSES } from "./state.js";

/**
 * Browser <-> server messages. The server validates every inbound frame with
 * these schemas before acting on it — the page is reachable from anything that
 * can talk to localhost, so it is not trusted.
 */

const values = z.record(z.string(), z.unknown());

export const ClientMessage = z.discriminatedUnion("type", [
  z.object({ type: z.literal("hello"), sessionId: z.string(), lastSeq: z.number().int().nonnegative().optional() }),
  /**
   * `interim` is throttled drag noise and never wakes a waiting agent;
   * `committed` fires on pointer release / blur and does.
   *
   * `touched` carries the control ids the human has actually interacted with,
   * which is not derivable from the values themselves — someone may deliberately
   * set a control back to its default. Without it an agent cannot tell a real
   * decision from an untouched default and will act on values nobody chose.
   */
  z.object({
    type: z.literal("values"),
    values,
    kind: z.enum(["interim", "committed"]),
    touched: z.array(z.string()).optional(),
  }),
  z.object({ type: z.literal("comment"), text: z.string() }),
  z.object({
    type: z.literal("action"),
    actionId: z.string(),
    values: values.optional(),
    touched: z.array(z.string()).optional(),
  }),
  z.object({ type: z.literal("finalize"), values: values.optional(), touched: z.array(z.string()).optional() }),
  z.object({ type: z.literal("preview_event"), name: z.string(), data: z.unknown().optional() }),
  z.object({ type: z.literal("preview_error"), message: z.string(), stack: z.string().optional() }),
  z.object({ type: z.literal("ping") }),
]);

export type ClientMessage = z.infer<typeof ClientMessage>;

export const ServerMessage = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("init"),
    sessionId: z.string(),
    spec: InterfaceSpec,
    values,
    status: z.enum(SESSION_STATUSES),
    revision: z.number().int().nonnegative(),
    lastEventSeq: z.number().int().nonnegative(),
    hasPreview: z.boolean(),
    choice: z.string().optional(),
    /** Restores the touched/untouched distinction across a reconnect or a second tab. */
    touched: z.array(z.string()).default([]),
  }),
  z.object({
    type: z.literal("spec_updated"),
    spec: InterfaceSpec,
    values,
    revision: z.number().int().nonnegative(),
    hasPreview: z.boolean(),
  }),
  /** The agent pushed new values via ui_update. */
  z.object({ type: z.literal("values_pushed"), values }),
  z.object({ type: z.literal("finalized"), choice: z.string().optional() }),
  z.object({ type: z.literal("closed") }),
  z.object({ type: z.literal("event"), event: SessionEvent }),
  z.object({ type: z.literal("error"), message: z.string() }),
  z.object({ type: z.literal("pong") }),
]);

export type ServerMessage = z.infer<typeof ServerMessage>;

export const WS_PATH = "/ws";
