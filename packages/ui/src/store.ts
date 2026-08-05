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
            revision: message.revision,
            hasPreview: message.hasPreview,
            previewError: undefined,
          });
          break;
        case "values_pushed":
          set({ values: message.values as Values });
          break;
        case "finalized":
          set({ status: "finalized", choice: message.choice });
          break;
        case "closed":
          set({ status: "closed" });
          break;
        case "event":
          set({ lastEventSeq: message.event.seq });
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
        return { values: { ...state.values, [id]: recommended }, touched };
      });
      get().commit();
    },

    resetAll() {
      const { spec, status } = get();
      if (!spec || status !== "active") return;
      set({ values: defaultsFromSpec(spec), touched: new Set() });
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

/** Session id from /s/<id>; undefined on the dashboard and library routes. */
export function sessionIdFromPath(pathname = window.location.pathname): string | undefined {
  const match = /^\/s\/([a-z0-9]+)\/?$/i.exec(pathname);
  return match?.[1];
}
