import type http from "node:http";
import type { Duplex } from "node:stream";
import { WebSocketServer, type WebSocket } from "ws";
import { ClientMessage, WS_PATH, type ServerMessage, type SessionEvent } from "@agentui/shared";
import type { Outbound, SessionManager } from "../core/session-manager.js";
import { candidateOriginsAllProjects } from "../core/ports.js";
import { isAllowedHostHeader } from "./security.js";
import { log } from "../log.js";

const ALLOWED_ORIGINS = new Set(candidateOriginsAllProjects());

interface Client {
  socket: WebSocket;
  sessionId?: string;
}

/**
 * WebSocket fan-out for session pages.
 *
 * Security note: WebSocket handshakes are NOT covered by CORS — any page you
 * visit can open a socket to localhost and start talking. Browsers do always
 * send `Origin` on the handshake, so requiring it to be one of ours when
 * present blocks hostile pages, while allowing it to be absent keeps
 * non-browser clients (tests, scripts) working, which cannot be a drive-by
 * vector precisely because a page cannot omit it.
 */
export function attachWebSocket(server: http.Server, sessions: SessionManager): void {
  const wss = new WebSocketServer({ noServer: true });
  const clients = new Set<Client>();

  server.on("upgrade", (req, socket: Duplex, head) => {
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    if (url.pathname !== WS_PATH) {
      socket.destroy();
      return;
    }
    if (!isAllowedHostHeader(req.headers.host)) {
      log.warn("ws upgrade rejected: bad Host", req.headers.host);
      socket.write("HTTP/1.1 403 Forbidden\r\n\r\n");
      socket.destroy();
      return;
    }
    const origin = req.headers.origin;
    if (origin !== undefined && !ALLOWED_ORIGINS.has(origin)) {
      log.warn("ws upgrade rejected: bad Origin", origin);
      socket.write("HTTP/1.1 403 Forbidden\r\n\r\n");
      socket.destroy();
      return;
    }
    wss.handleUpgrade(req, socket, head, (ws) => wss.emit("connection", ws, req));
  });

  wss.on("connection", (socket: WebSocket) => {
    const client: Client = { socket };
    clients.add(client);

    socket.on("message", (raw) => {
      void handleRaw(client, raw);
    });

    socket.on("close", () => {
      clients.delete(client);
      if (client.sessionId) {
        sessions.get(client.sessionId)?.events.append("client_disconnected");
      }
    });

    socket.on("error", (error) => log.debug("ws error", String(error)));
  });

  async function handleRaw(client: Client, raw: unknown): Promise<void> {
    let parsed: unknown;
    try {
      parsed = JSON.parse(String(raw));
    } catch {
      send(client, { type: "error", message: "malformed json" });
      return;
    }

    // The page is reachable by anything that can talk to loopback, so every
    // frame is schema-validated before it reaches the session.
    const result = ClientMessage.safeParse(parsed);
    if (!result.success) {
      send(client, { type: "error", message: `invalid message: ${result.error.issues[0]?.message ?? "unknown"}` });
      return;
    }
    const message = result.data;

    if (message.type === "hello") {
      const session = sessions.get(message.sessionId);
      if (!session) {
        send(client, { type: "error", message: `unknown session ${message.sessionId}` });
        return;
      }
      client.sessionId = message.sessionId;
      session.events.append("client_connected");
      send(client, sessions.initMessage(session));
      return;
    }

    if (!client.sessionId) {
      send(client, { type: "error", message: "send hello first" });
      return;
    }

    const outbound = await sessions.handleClientMessage(client.sessionId, message);
    dispatch(client, outbound);
  }

  function dispatch(sender: Client, outbound: Outbound[]): void {
    for (const { to, message } of outbound) {
      if (to === "sender") {
        send(sender, message);
        continue;
      }
      for (const client of clients) {
        if (client.sessionId !== sender.sessionId) continue;
        if (to === "others" && client === sender) continue;
        send(client, message);
      }
    }
  }

  function send(client: Client, message: ServerMessage): void {
    if (client.socket.readyState !== client.socket.OPEN) return;
    try {
      client.socket.send(JSON.stringify(message));
    } catch (error) {
      log.debug("ws send failed", String(error));
    }
  }

  /** Pushes a server-originated change (ui_update, close) to a session's tabs. */
  function broadcast(sessionId: string, message: ServerMessage): void {
    for (const client of clients) {
      if (client.sessionId === sessionId) send(client, message);
    }
  }

  function connectedClients(sessionId: string): number {
    let count = 0;
    for (const client of clients) if (client.sessionId === sessionId) count++;
    return count;
  }

  broadcaster = { broadcast, connectedClients, totalClients: () => clients.size };

  /** Mirror every event to attached tabs, so a second tab stays in step. */
  function relayEvent(sessionId: string, event: SessionEvent): void {
    broadcast(sessionId, { type: "event", event });
  }
  eventRelay = relayEvent;
}

interface Broadcaster {
  broadcast(sessionId: string, message: ServerMessage): void;
  connectedClients(sessionId: string): number;
  totalClients(): number;
}

// Set once when the socket layer attaches; the MCP tools use it to push
// ui_update / close notifications out to open tabs.
let broadcaster: Broadcaster | undefined;
let eventRelay: ((sessionId: string, event: SessionEvent) => void) | undefined;

export function broadcastToSession(sessionId: string, message: ServerMessage): void {
  broadcaster?.broadcast(sessionId, message);
}

export function connectedClientCount(sessionId: string): number {
  return broadcaster?.connectedClients(sessionId) ?? 0;
}

/**
 * Whether the user has AgentUI open anywhere at all.
 *
 * This is what decides between "open a tab" and "let the tab they already have
 * surface it". Three questions producing three tabs is the difference between a
 * tool an agent reaches for and one it avoids.
 */
export function anyClientConnected(): number {
  return broadcaster?.totalClients() ?? 0;
}

export function relaySessionEvent(sessionId: string, event: SessionEvent): void {
  eventRelay?.(sessionId, event);
}
