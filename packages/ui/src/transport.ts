import type { ClientMessage, ServerMessage } from "@agentui/shared";

export type ConnectionStatus = "connecting" | "open" | "reconnecting" | "closed";

/**
 * How the shell talks to its session.
 *
 * Nothing in the UI may reference `WebSocket` directly — it goes through this
 * interface. Today there is exactly one implementation. If the inline-widget
 * spike ever passes, an MCP-Apps view plugs in a second implementation here and
 * no component changes; that hedge costs almost nothing now and would be
 * expensive to retrofit once components have bound to a socket.
 */
export interface SessionTransport {
  readonly status: ConnectionStatus;
  send(message: ClientMessage): void;
  onMessage(handler: (message: ServerMessage) => void): () => void;
  onStatus(handler: (status: ConnectionStatus) => void): () => void;
  close(): void;
}

/** Reconnecting WebSocket transport. The server's state always wins on reconnect. */
export class WebSocketTransport implements SessionTransport {
  #socket: WebSocket | undefined;
  #status: ConnectionStatus = "connecting";
  #messageHandlers = new Set<(message: ServerMessage) => void>();
  #statusHandlers = new Set<(status: ConnectionStatus) => void>();
  #attempt = 0;
  #closedByUs = false;
  #queue: ClientMessage[] = [];
  #retryTimer: number | undefined;

  constructor(
    private readonly url: string,
    private readonly sessionId: string,
    private readonly lastSeq: () => number,
  ) {
    this.#connect();
  }

  get status(): ConnectionStatus {
    return this.#status;
  }

  #setStatus(status: ConnectionStatus): void {
    if (this.#status === status) return;
    this.#status = status;
    for (const handler of this.#statusHandlers) handler(status);
  }

  #connect(): void {
    const socket = new WebSocket(this.url);
    this.#socket = socket;

    socket.addEventListener("open", () => {
      this.#attempt = 0;
      this.#setStatus("open");
      // Always re-introduce ourselves; the server replies with an authoritative
      // `init`, so a tab that missed events while offline resynchronises rather
      // than silently drifting.
      socket.send(JSON.stringify({ type: "hello", sessionId: this.sessionId, lastSeq: this.lastSeq() }));
      for (const queued of this.#queue.splice(0)) socket.send(JSON.stringify(queued));
    });

    socket.addEventListener("message", (event) => {
      let parsed: ServerMessage;
      try {
        parsed = JSON.parse(String(event.data)) as ServerMessage;
      } catch {
        return;
      }
      for (const handler of this.#messageHandlers) handler(parsed);
    });

    socket.addEventListener("close", () => {
      if (this.#closedByUs) {
        this.#setStatus("closed");
        return;
      }
      this.#setStatus("reconnecting");
      this.#scheduleReconnect();
    });

    socket.addEventListener("error", () => socket.close());
  }

  #scheduleReconnect(): void {
    if (this.#retryTimer !== undefined) return;
    // Exponential backoff capped at 5s — long enough not to hammer a dead
    // server, short enough that a restarted one is picked up quickly.
    const delay = Math.min(5000, 250 * 2 ** this.#attempt++);
    this.#retryTimer = window.setTimeout(() => {
      this.#retryTimer = undefined;
      this.#connect();
    }, delay);
  }

  send(message: ClientMessage): void {
    const socket = this.#socket;
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(message));
      return;
    }
    // Interim drag frames are worthless once stale — never queue them, or a
    // reconnect replays a burst of obsolete positions.
    if (message.type === "values" && message.kind === "interim") return;
    this.#queue.push(message);
  }

  onMessage(handler: (message: ServerMessage) => void): () => void {
    this.#messageHandlers.add(handler);
    return () => this.#messageHandlers.delete(handler);
  }

  onStatus(handler: (status: ConnectionStatus) => void): () => void {
    this.#statusHandlers.add(handler);
    return () => this.#statusHandlers.delete(handler);
  }

  close(): void {
    this.#closedByUs = true;
    if (this.#retryTimer !== undefined) window.clearTimeout(this.#retryTimer);
    this.#socket?.close();
  }
}

export function websocketUrlFor(): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws`;
}
