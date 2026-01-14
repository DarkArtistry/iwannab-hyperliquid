/**
 * Exchange API client for trading operations
 */

import type { Signer } from './signing';
import type {
  OrderRequest,
  CancelRequest,
  CancelByCloidRequest,
  LeverageUpdateRequest,
  TransferRequest,
  OrderType,
} from './types';
import { MARKETS } from './constants';

interface OrderResponse {
  status: 'ok' | 'err';
  response?: {
    type: string;
    data: {
      statuses: Array<{
        resting?: { oid: number; cloid?: string };
        filled?: { totalSz: string; avgPx: string; oid: number };
        error?: string;
      }>;
    };
  };
}

export class ExchangeClient {
  private baseUrl: string;
  private signer: Signer | null;
  private timeout: number;

  constructor(baseUrl: string, signer: Signer | null, timeout: number = 30000) {
    this.baseUrl = baseUrl;
    this.signer = signer;
    this.timeout = timeout;
  }

  private async request<T>(action: Record<string, unknown>): Promise<T> {
    if (!this.signer) {
      throw new Error('Signer required for exchange operations');
    }

    const nonce = Date.now();
    const signature = await this.signer.signAction(action, nonce);

    const body = {
      action,
      nonce,
      signature,
    };

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(`${this.baseUrl}/exchange`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: controller.signal,
      });

      const data = await response.json();

      if (data.status === 'err') {
        throw new Error(`Exchange Error: ${data.response || 'Unknown error'}`);
      }

      return data as T;
    } finally {
      clearTimeout(timeoutId);
    }
  }

  private resolveMarketId(market: string | number): number {
    if (typeof market === 'number') return market;
    const id = MARKETS[market as keyof typeof MARKETS];
    if (id === undefined) {
      throw new Error(`Unknown market: ${market}`);
    }
    return id;
  }

  private orderTypeToCode(type?: OrderType): { limit: { tif: string } } | { trigger: { isMarket: boolean; triggerPx: string; tpsl: string } } {
    switch (type) {
      case 'market':
        return { trigger: { isMarket: true, triggerPx: '0', tpsl: 'tp' } };
      case 'postOnly':
        return { limit: { tif: 'Alo' } };
      case 'ioc':
        return { limit: { tif: 'Ioc' } };
      case 'fok':
        return { limit: { tif: 'Fok' } };
      case 'limit':
      default:
        return { limit: { tif: 'Gtc' } };
    }
  }

  /**
   * Place an order
   */
  async placeOrder(request: OrderRequest): Promise<OrderResponse> {
    const marketId = this.resolveMarketId(request.market);

    const order = {
      a: marketId,
      b: request.side === 'buy',
      p: request.price ?? '0',
      s: request.size,
      r: request.reduceOnly ?? false,
      t: this.orderTypeToCode(request.type as OrderType),
      c: request.clientOrderId,
    };

    return this.request({
      type: 'order',
      orders: [order],
      grouping: 'na',
    });
  }

  /**
   * Place multiple orders
   */
  async placeOrders(requests: OrderRequest[]): Promise<OrderResponse> {
    const orders = requests.map((request) => {
      const marketId = this.resolveMarketId(request.market);
      return {
        a: marketId,
        b: request.side === 'buy',
        p: request.price ?? '0',
        s: request.size,
        r: request.reduceOnly ?? false,
        t: this.orderTypeToCode(request.type as OrderType),
        c: request.clientOrderId,
      };
    });

    return this.request({
      type: 'order',
      orders,
      grouping: 'na',
    });
  }

  /**
   * Cancel an order by ID
   */
  async cancelOrder(request: CancelRequest): Promise<{ status: string }> {
    const marketId = this.resolveMarketId(request.market);

    return this.request({
      type: 'cancel',
      cancels: [{ a: marketId, o: request.orderId }],
    });
  }

  /**
   * Cancel an order by client order ID
   */
  async cancelByCloid(request: CancelByCloidRequest): Promise<{ status: string }> {
    const marketId = this.resolveMarketId(request.market);

    return this.request({
      type: 'cancelByCloid',
      cancels: [{ asset: marketId, cloid: request.clientOrderId }],
    });
  }

  /**
   * Cancel all orders in a market
   */
  async cancelAllOrders(market?: string | number): Promise<{ status: string }> {
    if (market !== undefined) {
      const marketId = this.resolveMarketId(market);
      // Cancel all in specific market
      return this.request({
        type: 'cancel',
        cancels: [{ a: marketId, o: 0 }], // 0 = all orders
      });
    }

    // Cancel all orders in all markets
    return this.request({
      type: 'cancelAll',
    });
  }

  /**
   * Update leverage for a market
   */
  async updateLeverage(request: LeverageUpdateRequest): Promise<{ status: string }> {
    const marketId = this.resolveMarketId(request.market);

    return this.request({
      type: 'updateLeverage',
      asset: marketId,
      isCross: request.isCross ?? true,
      leverage: request.leverage,
    });
  }

  /**
   * Transfer USDC to another address
   */
  async transfer(request: TransferRequest): Promise<{ status: string }> {
    return this.request({
      type: 'usdTransfer',
      destination: request.destination,
      amount: request.amount,
    });
  }

  /**
   * Withdraw USDC to L1
   */
  async withdraw(request: TransferRequest): Promise<{ status: string }> {
    return this.request({
      type: 'withdraw',
      destination: request.destination,
      amount: request.amount,
    });
  }

  /**
   * Modify an existing order
   */
  async modifyOrder(
    orderId: number,
    newOrder: Omit<OrderRequest, 'clientOrderId'>
  ): Promise<OrderResponse> {
    const marketId = this.resolveMarketId(newOrder.market);

    return this.request({
      type: 'batchModify',
      modifies: [
        {
          oid: orderId,
          order: {
            a: marketId,
            b: newOrder.side === 'buy',
            p: newOrder.price ?? '0',
            s: newOrder.size,
            r: newOrder.reduceOnly ?? false,
            t: this.orderTypeToCode(newOrder.type as OrderType),
          },
        },
      ],
    });
  }
}
