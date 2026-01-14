/**
 * Info API client for read-only queries
 */

import type {
  AccountState,
  Fill,
  FundingRate,
  L2Book,
  MarketMeta,
  Order,
  ApiError,
} from './types';

export class InfoClient {
  private baseUrl: string;
  private timeout: number;

  constructor(baseUrl: string, timeout: number = 30000) {
    this.baseUrl = baseUrl;
    this.timeout = timeout;
  }

  private async request<T>(body: Record<string, unknown>): Promise<T> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(`${this.baseUrl}/info`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: controller.signal,
      });

      const data = await response.json();

      if (data.status === 'err') {
        const error = data as ApiError;
        throw new Error(`API Error ${error.code}: ${error.message}`);
      }

      return data as T;
    } finally {
      clearTimeout(timeoutId);
    }
  }

  /**
   * Get exchange metadata (markets, fees, etc.)
   */
  async getMeta(): Promise<{ universe: MarketMeta[] }> {
    return this.request({ type: 'meta' });
  }

  /**
   * Get all mid prices
   */
  async getAllMids(): Promise<Record<string, string>> {
    return this.request({ type: 'allMids' });
  }

  /**
   * Get L2 order book
   */
  async getL2Book(coin: string, nSigFigs?: number): Promise<L2Book> {
    return this.request({
      type: 'l2Book',
      coin,
      nSigFigs: nSigFigs ?? 5,
    });
  }

  /**
   * Get account state (positions, margin, etc.)
   */
  async getAccountState(user: string): Promise<AccountState> {
    return this.request({
      type: 'clearinghouseState',
      user,
    });
  }

  /**
   * Get open orders for a user
   */
  async getOpenOrders(user: string): Promise<Order[]> {
    return this.request({
      type: 'openOrders',
      user,
    });
  }

  /**
   * Get user's trade history
   */
  async getUserFills(
    user: string,
    options?: {
      startTime?: number;
      endTime?: number;
      aggregateByTime?: boolean;
    }
  ): Promise<Fill[]> {
    return this.request({
      type: 'userFills',
      user,
      ...options,
    });
  }

  /**
   * Get funding rate history for a market
   */
  async getFundingHistory(
    coin: string,
    options?: {
      startTime?: number;
      endTime?: number;
    }
  ): Promise<FundingRate[]> {
    return this.request({
      type: 'fundingHistory',
      coin,
      ...options,
    });
  }

  /**
   * Get user's funding payment history
   */
  async getUserFundingHistory(
    user: string,
    options?: {
      startTime?: number;
      endTime?: number;
    }
  ): Promise<Array<{
    time: number;
    coin: string;
    usdc: string;
    szi: string;
    fundingRate: string;
  }>> {
    return this.request({
      type: 'userFundingHistory',
      user,
      ...options,
    });
  }

  /**
   * Get recent trades for a market
   */
  async getRecentTrades(
    coin: string,
    limit?: number
  ): Promise<Array<{
    coin: string;
    side: 'B' | 'A';
    px: string;
    sz: string;
    time: number;
    hash: string;
  }>> {
    return this.request({
      type: 'recentTrades',
      coin,
      limit: limit ?? 100,
    });
  }

  /**
   * Get candles/OHLCV data
   */
  async getCandles(
    coin: string,
    interval: '1m' | '5m' | '15m' | '1h' | '4h' | '1d',
    options?: {
      startTime?: number;
      endTime?: number;
    }
  ): Promise<Array<{
    t: number;
    o: string;
    h: string;
    l: string;
    c: string;
    v: string;
  }>> {
    return this.request({
      type: 'candleSnapshot',
      coin,
      interval,
      ...options,
    });
  }
}
