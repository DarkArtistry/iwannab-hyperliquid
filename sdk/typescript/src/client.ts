/**
 * Main HyperCore client
 */

import { InfoClient } from './info';
import { ExchangeClient } from './exchange';
import { WebSocketClient } from './websocket';
import { Signer } from './signing';
import type { HyperCoreConfig } from './types';
import { DEFAULT_BASE_URL, DEFAULT_WS_URL, DEFAULT_CHAIN_ID } from './constants';

export class HyperCore {
  readonly config: Required<HyperCoreConfig>;
  readonly info: InfoClient;
  readonly exchange: ExchangeClient;
  readonly ws: WebSocketClient;
  readonly signer: Signer | null;

  constructor(config: HyperCoreConfig = {}) {
    this.config = {
      baseUrl: config.baseUrl ?? DEFAULT_BASE_URL,
      wsUrl: config.wsUrl ?? DEFAULT_WS_URL,
      privateKey: config.privateKey ?? '',
      chainId: config.chainId ?? DEFAULT_CHAIN_ID,
      timeout: config.timeout ?? 30000,
    };

    // Initialize signer if private key provided
    this.signer = this.config.privateKey
      ? new Signer(this.config.privateKey, this.config.chainId)
      : null;

    // Initialize sub-clients
    this.info = new InfoClient(this.config.baseUrl, this.config.timeout);
    this.exchange = new ExchangeClient(
      this.config.baseUrl,
      this.signer,
      this.config.timeout
    );
    this.ws = new WebSocketClient(this.config.wsUrl);
  }

  /**
   * Get the signer's address
   */
  get address(): string | null {
    return this.signer?.address ?? null;
  }

  /**
   * Check if client is authenticated (has signer)
   */
  get isAuthenticated(): boolean {
    return this.signer !== null;
  }

  /**
   * Connect to WebSocket
   */
  async connect(): Promise<void> {
    await this.ws.connect();
  }

  /**
   * Disconnect from WebSocket
   */
  disconnect(): void {
    this.ws.disconnect();
  }

  /**
   * Subscribe to market data
   */
  subscribeToMarket(
    market: string,
    callbacks: {
      onTrade?: (trade: unknown) => void;
      onL2Book?: (book: unknown) => void;
    }
  ): void {
    if (callbacks.onTrade) {
      this.ws.subscribe({ type: 'trades', coin: market }, callbacks.onTrade);
    }
    if (callbacks.onL2Book) {
      this.ws.subscribe({ type: 'l2Book', coin: market }, callbacks.onL2Book);
    }
  }

  /**
   * Subscribe to user updates (requires authentication)
   */
  subscribeToUser(callback: (update: unknown) => void): void {
    if (!this.address) {
      throw new Error('Cannot subscribe to user updates without authentication');
    }
    this.ws.subscribeUser(this.address, callback);
  }
}
