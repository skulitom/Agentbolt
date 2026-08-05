import { create } from "zustand";
import { defaultsFromSpec, type InterfaceSpec, type SessionStatus, type ServerMessage, type Values } from "@agentui/shared";
import type { ConnectionStatus, SessionTransport } from "./transport.js";

interface SessionStore {
  sessionId: string | undefined;
  spec: InterfaceSpec | undefined;
  values: Values;
  status: SessionStatus;
  revision: number;
  lastEventSeq: number;
  hasPreview: boolean;
  previewOrigin: string | undefined;
  choice: string | undefined;
  connection: ConnectionStatus;
  previewError: { message: string; stack?: string } | undefined;
  transport: SessionTransport | undefined;
  sentCount: number;

  /**
   * Controls the human has actually engaged with. Not derivable from values —
   * someone may deliberately set a control back to its default — and the whole
   * point is to let an agent distinguish a decision from an untouched default.
   */
  touched: Set<string>;

  /**
   * Who wrote `values` last, read by PreviewFrame at post time so the preview
   * can animate agent pushes while user drags track 1:1 under the finger.
   *
   * The distinction is GESTURE vs JUMP, not literally who acted: reset is a
   * click, the pointer is nowhere near the control, and animating the revert
   * is desirable — so reset counts as "agent" alongside init, spec_updated and
   * values_pushed. Written in the same set() as every values write, never on
   * its own, so the pair can never be observed torn.
   */
  valuesSource: "user" | "agent";

  /**
   * Presentation-only trail of the last values_pushed: pushSeq bumps once per
   * push and pushedIds carries which controls actually changed, so their cards
   * can flash. Deliberately NOT part of the value pipeline — nothing here
   * touches `touched` or generates interim traffic.
   */
  pushSeq: number;
  pushedIds: ReadonlySet<string>;

  attach(transport: SessionTransport, sessionId: string): void;
  applyServerMessage(message: ServerMessage): void;
  setLocalValue(id: string, value: Values[string]): void;
  /** Restore one control to the agent's recommended value and mark it untouched. */
  resetControl(id: string): void;
  resetAll(): void;
  commit(): void;
  sendComment(text: string): void;
  pressAction(actionId: string): void;
  finalize(): void;
  setPreviewError(error: { message: string; stack?: string } | undefined): void;
}

/**
 * Three-tier value propagation — the core UX/noise tradeoff of the whole tool:
 *
 *  1. local state updates every frame, so the preview is live at 60fps;
 *  2. `interim` frames go over the wire on a 100ms trailing throttle, keeping
 *     ui_get_state fresh without spamming;
 *  3. `committed` fires on pointer release / blur, and is the ONLY thing that
 *     wakes a waiting agent.
 *
 * If tier 2 ever became a wake reason, one slider drag would wake the agent
 * hundreds of times and the whole async model would collapse.
 */
const INTERIM_THROTTLE_MS = 100;

export const useSession = create<SessionStore>((set, get) => {
  let interimTimer: number | undefined;
  let interimPending = false;

  function flushInterim(): void {
    interimTimer = undefined;
    if (!interimPending) return;
    interimPending = false;
    const { transport, values } = get();
    transport?.send({ type: "values", values, kind: "interim" });
  }

  function scheduleInterim(): void {
    interimPending = true;
    if (interimTimer !== undefined) return;
    interimTimer = window.setTimeout(flushInterim, INTERIM_THROTTLE_MS);
  }

  return {
    sessionId: undefined,
    spec: undefined,
    values: {},
    status: "active",
    revision: 0,
    lastEventSeq: 0,
    hasPreview: false,
    previewOrigin: undefined,
    choice: undefined,
    connection: "connecting",
    previewError: undefined,
    transport: undefined,
    sentCount: 0,
    touched: new Set<string>(),
    valuesSource: "agent",
    pushSeq: 0,
    pushedIds: new Set<string>(),

    attach(transport, sessionId) {
      set({ transport, sessionId });
      transport.onStatus((connection) => set({ connection }));
      transport.onMessage((message) => get().applyServerMessage(message));
    },

    applyServerMessage(message) {
      switch (message.type) {
        case "init":
          set({
            spec: message.spec,
            values: message.values as Values,
            valuesSource: "agent",
            // Server is authoritative, so a reconnect or a second tab restores
            // which controls were genuinely engaged with.
            touched: new Set(message.touched ?? []),
            status: message.status,
            revision: message.revision,
            lastEventSeq: message.lastEventSeq,
            hasPreview: message.hasPreview,
            choice: message.choice,
          });
          break;
        case "spec_updated":
          // A re-authored widget must not discard what the human has already
          // adjusted; the server preserved surviving values, so take both.
          set({
            spec: message.spec,
            values: message.values as Values,
            valuesSource: "agent",
            revision: message.revision,
            hasPreview: message.hasPreview,
            previewError: undefined,
          });
          break;
        case "values_pushed":
          set((state) => {
            const next = message.values as Values;
            const pushedIds = new Set<string>();
            for (const [id, value] of Object.entries(next)) {
              if (!sameValue(state.values[id], value)) pushedIds.add(id);
            }
            return { values: next, valuesSource: "agent", pushSeq: state.pushSeq + 1, pushedIds };
          });
          break;
        case "finalized":
          set({ status: "finalized", choice: message.choice });
          break;
        case "closed":
          set({ status: "closed" });
          break;
        case "event":
          // Closing is server-side only — ui_close, or the user dismissing this
          // widget from the dashboard — so the relayed event is the only word a
          // tab ever gets about it. Without this the page would keep offering
          // controls that the server has already stopped accepting.
          if (message.event.type === "closed") set({ status: "closed", lastEventSeq: message.event.seq });
          else set({ lastEventSeq: message.event.seq });
          break;
        case "error":
        case "pong":
          break;
      }
    },

    setLocalValue(id, value) {
      if (get().status !== "active") return;
      set((state) => ({
        values: { ...state.values, [id]: value },
        valuesSource: "user",
        touched: new Set(state.touched).add(id),
      }));
      scheduleInterim();
    },

    resetControl(id) {
      const { spec, status } = get();
      if (!spec || status !== "active") return;
      const recommended = defaultsFromSpec(spec)[id];
      if (recommended === undefined) return;
      set((state) => {
        const touched = new Set(state.touched);
        touched.delete(id);
        return { values: { ...state.values, [id]: recommended }, valuesSource: "agent", touched };
      });
      get().commit();
    },

    resetAll() {
      const { spec, status } = get();
      if (!spec || status !== "active") return;
      set({ values: defaultsFromSpec(spec), valuesSource: "agent", touched: new Set() });
      get().commit();
    },

    commit() {
      const { transport, values, status } = get();
      if (status !== "active") return;
      // Cancel any pending interim: the committed frame supersedes it.
      if (interimTimer !== undefined) {
        window.clearTimeout(interimTimer);
        interimTimer = undefined;
      }
      interimPending = false;
      transport?.send({ type: "values", values, kind: "committed", touched: [...get().touched] });
      set((state) => ({ sentCount: state.sentCount + 1 }));
    },

    sendComment(text) {
      const { transport, status } = get();
      if (status !== "active") return;
      transport?.send({ type: "comment", text });
      set((state) => ({ sentCount: state.sentCount + 1 }));
    },

    pressAction(actionId) {
      const { transport, values, status, touched } = get();
      if (status !== "active") return;
      transport?.send({ type: "action", actionId, values, touched: [...touched] });
    },

    finalize() {
      const { transport, values, status, touched } = get();
      if (status !== "active") return;
      transport?.send({ type: "finalize", values, touched: [...touched] });
    },

    setPreviewError(previewError) {
      set({ previewError });
    },
  };
});

/** Value equality for push-diffing; vectors arrive as fresh arrays every time. */
function sameValue(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((x, i) => Object.is(x, b[i]));
  }
  return false;
}

/** Session id from /s/<id>; undefined on the dashboard and library routes. */
export function sessionIdFromPath(pathname = window.location.pathname): string | undefined {
  const match = /^\/s\/([a-z0-9]+)\/?$/i.exec(pathname);
  return match?.[1];
}
