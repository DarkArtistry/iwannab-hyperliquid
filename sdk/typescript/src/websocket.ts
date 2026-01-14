/**
 * WebSocket client for real-time data feeds
 */

import WebSocket from 'ws';
import type { WsSubscription, WsMessage } from './types';

type MessageHandler = (data: unknown) => void;

export class WebSocketClient {
  private url: string;
  private ws: WebSocket | null = null;
  private subscriptions: Map<string, Set<MessageHandler>> = new Map();
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;
  private pingInterval: NodeJS.Timeout | null = null;
  private isConnecting = false;

  constructor(url: string) {
    this.url = url;
  }

  /**
   * Connect to WebSocket server
   */
  async connect(): Promise<void> {
    if (this.ws?.readyState === WebSocket.OPEN) {
      return;
    }

    if (this.isConnecting) {
      return new Promise((resolve) => {
        const checkConnection = setInterval(() => {
          if (this.ws?.readyState === WebSocket.OPEN) {
            clearInterval(checkConnection);
            resolve();
          }
        }, 100);
      });
    }

    this.isConnecting = true;

    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);

      this.ws.onopen = () => {
        this.isConnecting = false;
        this.reconnectAttempts = 0;
        this.startPingInterval();

        // Resubscribe to all channels
        for (const [channel] of this.subscriptions) {
          this.sendSubscription(JSON.parse(channel));
        }

        resolve();
      };

      this.ws.onclose = () => {
        this.isConnecting = false;
        this.stopPingInterval();
        this.handleReconnect();
      };

      this.ws.onerror = (error) => {
        this.isConnecting = false;
        if (this.reconnectAttempts === 0) {
          reject(error);
        }
      };

      this.ws.onmessage = (event) => {
        this.handleMessage(event.data as string);
      };
    });
  }

  /**
   * Disconnect from WebSocket server
   */
  disconnect(): void {
    this.stopPingInterval();
    this.subscriptions.clear();

    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  /**
   * Subscribe to a channel
   */
  subscribe(subscription: WsSubscription, handler: MessageHandler): void {
    const key = JSON.stringify(subscription);

    if (!this.subscriptions.has(key)) {
      this.subscriptions.set(key, new Set());
      this.sendSubscription(subscription);
    }

    this.subscriptions.get(key)!.add(handler);
  }

  /**
   * Unsubscribe from a channel
   */
  unsubscribe(subscription: WsSubscription, handler?: MessageHandler): void {
    const key = JSON.stringify(subscription);
    const handlers = this.subscriptions.get(key);

    if (!handlers) return;

    if (handler) {
      handlers.delete(handler);
      if (handlers.size === 0) {
        this.subscriptions.delete(key);
        this.sendUnsubscription(subscription);
      }
    } else {
      this.subscriptions.delete(key);
      this.sendUnsubscription(subscription);
    }
  }

  /**
   * Subscribe to user updates
   */
  subscribeUser(user: string, handler: MessageHandler): void {
    this.subscribe({ type: 'user', user }, handler);
  }

  /**
   * Subscribe to all mid prices
   */
  subscribeAllMids(handler: MessageHandler): void {
    this.subscribe({ type: 'allMids' }, handler);
  }

  /**
   * Subscribe to L2 order book
   */
  subscribeL2Book(coin: string, handler: MessageHandler): void {
    this.subscribe({ type: 'l2Book', coin }, handler);
  }

  /**
   * Subscribe to trades
   */
  subscribeTrades(coin: string, handler: MessageHandler): void {
    this.subscribe({ type: 'trades', coin }, handler);
  }

  /**
   * Subscribe to candles
   */
  subscribeCandles(
    coin: string,
    interval: '1m' | '5m' | '15m' | '1h' | '4h' | '1d',
    handler: MessageHandler
  ): void {
    this.subscribe({ type: 'candle', coin, interval }, handler);
  }

  private sendSubscription(subscription: WsSubscription): void {
    this.send({
      method: 'subscribe',
      subscription,
    });
  }

  private sendUnsubscription(subscription: WsSubscription): void {
    this.send({
      method: 'unsubscribe',
      subscription,
    });
  }

  private send(message: Record<string, unknown>): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data) as WsMessage;

      // Find matching subscription handlers
      for (const [key, handlers] of this.subscriptions) {
        const subscription = JSON.parse(key) as WsSubscription;

        if (this.matchesSubscription(message, subscription)) {
          for (const handler of handlers) {
            try {
              handler(message.data);
            } catch (error) {
              console.error('Handler error:', error);
            }
          }
        }
      }
    } catch (error) {
      console.error('Failed to parse message:', error);
    }
  }

  private matchesSubscription(message: WsMessage, subscription: WsSubscription): boolean {
    // Match channel type
    if (message.channel !== subscription.type) {
      return false;
    }

    // For coin-specific subscriptions, check coin matches
    if ('coin' in subscription && typeof message.data === 'object' && message.data !== null) {
      const data = message.data as Record<string, unknown>;
      if ('coin' in data && data.coin !== subscription.coin) {
        return false;
      }
    }

    return true;
  }

  private handleReconnect(): void {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.error('Max reconnect attempts reached');
      return;
    }

    this.reconnectAttempts++;
    const delay = this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1);

    setTimeout(() => {
      console.log(`Reconnecting (attempt ${this.reconnectAttempts})...`);
      this.connect().catch(console.error);
    }, delay);
  }

  private startPingInterval(): void {
    this.pingInterval = setInterval(() => {
      this.send({ method: 'ping' });
    }, 30000);
  }

  private stopPingInterval(): void {
    if (this.pingInterval) {
      clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }
}
