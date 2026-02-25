import { WS_URL } from "./constants";

type MessageHandler = (data: unknown) => void;

export class HyperCoreWebSocket {
  private ws: WebSocket | null = null;
  private handlers: Map<string, Set<MessageHandler>> = new Map();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private pendingSubscriptions: Array<{ type: string; [key: string]: unknown }> =
    [];
  private isConnected = false;

  connect() {
    if (this.ws?.readyState === WebSocket.OPEN) return;

    this.ws = new WebSocket(WS_URL);

    this.ws.onopen = () => {
      this.isConnected = true;
      // Re-subscribe any pending subscriptions
      for (const sub of this.pendingSubscriptions) {
        this.ws!.send(
          JSON.stringify({ method: "subscribe", subscription: sub })
        );
      }
      // Start heartbeat
      this.pingTimer = setInterval(() => {
        if (this.ws?.readyState === WebSocket.OPEN) {
          this.ws.send(JSON.stringify({ method: "ping" }));
        }
      }, 30000);
    };

    this.ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        const channel = msg.channel;
        if (channel && this.handlers.has(channel)) {
          for (const handler of this.handlers.get(channel)!) {
            handler(msg.data);
          }
        }
      } catch {
        // ignore parse errors
      }
    };

    this.ws.onclose = () => {
      this.isConnected = false;
      if (this.pingTimer) clearInterval(this.pingTimer);
      // Auto-reconnect after 3s
      this.reconnectTimer = setTimeout(() => this.connect(), 3000);
    };

    this.ws.onerror = () => {
      this.ws?.close();
    };
  }

  disconnect() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.ws?.close();
    this.ws = null;
    this.isConnected = false;
  }

  subscribe(subscription: { type: string; [key: string]: unknown }) {
    // Track for reconnect
    const key = JSON.stringify(subscription);
    if (!this.pendingSubscriptions.find((s) => JSON.stringify(s) === key)) {
      this.pendingSubscriptions.push(subscription);
    }
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(
        JSON.stringify({ method: "subscribe", subscription })
      );
    }
  }

  unsubscribe(subscription: { type: string; [key: string]: unknown }) {
    const key = JSON.stringify(subscription);
    this.pendingSubscriptions = this.pendingSubscriptions.filter(
      (s) => JSON.stringify(s) !== key
    );
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(
        JSON.stringify({ method: "unsubscribe", subscription })
      );
    }
  }

  on(channel: string, handler: MessageHandler) {
    if (!this.handlers.has(channel)) {
      this.handlers.set(channel, new Set());
    }
    this.handlers.get(channel)!.add(handler);
    return () => {
      this.handlers.get(channel)?.delete(handler);
    };
  }

  get connected() {
    return this.isConnected;
  }
}

// Singleton instance
let instance: HyperCoreWebSocket | null = null;

export function getWebSocket(): HyperCoreWebSocket {
  if (!instance) {
    instance = new HyperCoreWebSocket();
  }
  return instance;
}
