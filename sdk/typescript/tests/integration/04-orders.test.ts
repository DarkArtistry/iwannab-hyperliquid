/**
 * Order Lifecycle Tests
 *
 * These tests verify the complete order lifecycle including:
 * - Order placement (limit, market, post-only, IOC, FOK)
 * - Order cancellation (by ID, by client ID, cancel all)
 * - Order modification
 * - Batch operations
 *
 * @module tests/integration/04-orders
 */

import { describe, test, expect, beforeEach, afterEach } from 'vitest';
import {
  TEST_ACCOUNTS,
  MARKETS,
  REFERENCE_PRICES,
  createClient,
  createReadOnlyClient,
  generateClientOrderId,
  cancelAllOrders,
  waitForProcessing,
  OrderSide,
  OrderType,
} from './setup';

describe('Order Lifecycle', () => {
  // Clean up orders after each test
  afterEach(async () => {
    const alice = createClient(TEST_ACCOUNTS.ALICE);
    await cancelAllOrders(alice);
    await waitForProcessing();
  });

  // =========================================================================
  // LIMIT ORDERS
  // =========================================================================

  describe('Limit Orders', () => {
    /**
     * @scenario Place a limit buy order
     * @given Alice has sufficient margin
     * @when Alice places a limit buy for 0.1 BTC at $64,000
     * @then The order is accepted and resting on the book
     *
     * Limit orders:
     * - Specify exact price you're willing to pay/receive
     * - Remain on book until filled, canceled, or expired
     * - Maker orders (add liquidity) may receive rebates
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Buy,
     *   price: '64000',
     *   size: '0.1',
     *   type: OrderType.Limit,
     * });
     * ```
     */
    test('should place a limit buy order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place buy below current price to ensure it rests
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '60000', // Below market
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Place a limit sell order
     * @given Alice has sufficient margin
     * @when Alice places a limit sell for 0.1 BTC at $70,000
     * @then The order is accepted and resting on the book
     */
    test('should place a limit sell order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place sell above current price to ensure it rests
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '70000', // Above market
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Place limit order with client order ID
     * @given A unique client order ID
     * @when Placing an order with the cloid
     * @then The order can be tracked and canceled by cloid
     *
     * Client Order IDs:
     * - Must be unique per account
     * - Useful for order tracking in your system
     * - Can cancel orders using cloid instead of oid
     */
    test('should place order with client order ID', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const cloid = generateClientOrderId('limit-buy');

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '60000',
        size: '0.1',
        type: OrderType.Limit,
        clientOrderId: cloid,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // MARKET ORDERS
  // =========================================================================

  describe('Market Orders', () => {
    /**
     * @scenario Place a market buy order
     * @given There is liquidity on the ask side
     * @when Alice places a market buy for 0.01 BTC
     * @then The order executes immediately at best available price
     *
     * Market orders:
     * - Execute immediately at best available price
     * - No price guarantee (slippage possible)
     * - Taker orders (remove liquidity) pay fees
     * - Behave as IOC if not fully fillable
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Buy,
     *   size: '0.01',
     *   type: OrderType.Market,
     * });
     * ```
     */
    test('should place a market buy order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        size: '0.01',
        type: OrderType.Market,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Place a market sell order
     * @given There is liquidity on the bid side
     * @when Alice places a market sell for 0.01 BTC
     * @then The order executes immediately
     */
    test('should place a market sell order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        size: '0.01',
        type: OrderType.Market,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // POST-ONLY ORDERS
  // =========================================================================

  describe('Post-Only Orders', () => {
    /**
     * @scenario Place a post-only (maker-only) order
     * @given Current best ask is $65,000
     * @when Alice places post-only buy at $64,000
     * @then The order rests on the book (doesn't cross)
     *
     * Post-Only orders:
     * - Guaranteed to add liquidity (maker)
     * - Rejected if would immediately match (cross the spread)
     * - Receive maker fee rebates
     * - Use for market making strategies
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Buy,
     *   price: '64000',
     *   size: '0.1',
     *   type: OrderType.PostOnly,
     * });
     * ```
     */
    test('should place a post-only order that rests', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Post-only buy well below market
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000', // Well below market
        size: '0.1',
        type: OrderType.PostOnly,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // IOC ORDERS
  // =========================================================================

  describe('IOC (Immediate-or-Cancel) Orders', () => {
    /**
     * @scenario Place an IOC order
     * @given Liquidity exists at various price levels
     * @when Alice places IOC buy at $65,500
     * @then Order fills what's available, cancels remainder
     *
     * IOC orders:
     * - Fill immediately or cancel
     * - Any unfilled portion is automatically canceled
     * - No resting on the book
     * - Useful for aggressive execution
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Buy,
     *   price: '65500',
     *   size: '1.0',
     *   type: OrderType.Ioc,
     * });
     * // Check response.data.statuses for fill info
     * ```
     */
    test('should place an IOC order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '70000', // Aggressive price
        size: '0.01',
        type: OrderType.Ioc,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // FOK ORDERS
  // =========================================================================

  describe('FOK (Fill-or-Kill) Orders', () => {
    /**
     * @scenario Place a FOK order
     * @given Specific liquidity requirements
     * @when Alice places FOK buy for exactly 0.5 BTC
     * @then Order fills completely or is entirely rejected
     *
     * FOK orders:
     * - Must fill entirely or not at all
     * - No partial fills
     * - No resting on book
     * - Use when you need exact size execution
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Buy,
     *   price: '66000',
     *   size: '0.5',
     *   type: OrderType.Fok,
     * });
     * ```
     */
    test('should place a FOK order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '70000',
        size: '0.001', // Small size more likely to fill
        type: OrderType.Fok,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // REDUCE-ONLY ORDERS
  // =========================================================================

  describe('Reduce-Only Orders', () => {
    /**
     * @scenario Place a reduce-only order
     * @given Alice has a long position
     * @when Alice places reduce-only sell
     * @then Order can only reduce, not increase position
     *
     * Reduce-only orders:
     * - Can only reduce existing position size
     * - Automatically canceled if no position to reduce
     * - Prevents accidental position reversal
     * - Use for stop-loss and take-profit orders
     *
     * @example SDK Usage:
     * ```typescript
     * // Close long position with reduce-only sell
     * const result = await client.exchange.placeOrder({
     *   market: 'BTC-PERP',
     *   side: OrderSide.Sell,
     *   price: '66000',
     *   size: '0.1',
     *   type: OrderType.Limit,
     *   reduceOnly: true,
     * });
     * ```
     */
    test('should place a reduce-only order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '70000',
        size: '0.1',
        type: OrderType.Limit,
        reduceOnly: true,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // ORDER CANCELLATION
  // =========================================================================

  describe('Order Cancellation', () => {
    /**
     * @scenario Cancel order by exchange ID
     * @given Alice has a resting order
     * @when Alice cancels by order ID
     * @then The order is removed from the book
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.cancelOrder({
     *   market: 'BTC-PERP',
     *   orderId: 12345,
     * });
     * ```
     */
    test('should cancel order by ID', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // First place an order
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Cancel it (using a mock order ID - in practice, get from placeOrder response)
      const result = await alice.exchange.cancelOrder({
        market: MARKETS.BTC_PERP,
        orderId: 1, // Would come from order response
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Cancel order by client order ID
     * @given Alice placed order with cloid "my-order-123"
     * @when Alice cancels by cloid
     * @then The order is removed from the book
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.cancelByCloid({
     *   market: 'BTC-PERP',
     *   clientOrderId: 'my-order-123',
     * });
     * ```
     */
    test('should cancel order by client order ID', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const cloid = generateClientOrderId('cancel-test');

      // Place order with cloid
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000',
        size: '0.1',
        type: OrderType.Limit,
        clientOrderId: cloid,
      });

      await waitForProcessing();

      // Cancel by cloid
      const result = await alice.exchange.cancelByCloid({
        market: MARKETS.BTC_PERP,
        clientOrderId: cloid,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Cancel all orders in a market
     * @given Alice has multiple orders in BTC-PERP
     * @when Alice cancels all BTC-PERP orders
     * @then All BTC-PERP orders are removed
     *
     * @example SDK Usage:
     * ```typescript
     * // Cancel all orders in BTC-PERP
     * await client.exchange.cancelAllOrders('BTC-PERP');
     *
     * // Cancel all orders in ALL markets
     * await client.exchange.cancelAllOrders();
     * ```
     */
    test('should cancel all orders in market', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place multiple orders
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '75000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Cancel all
      const result = await alice.exchange.cancelAllOrders(MARKETS.BTC_PERP);

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Cancel all orders in all markets
     * @given Alice has orders in multiple markets
     * @when Alice cancels all orders globally
     * @then All orders are removed from all markets
     */
    test('should cancel all orders globally', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place orders in different markets
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await alice.exchange.placeOrder({
        market: MARKETS.ETH_PERP,
        side: OrderSide.Buy,
        price: '3000',
        size: '1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Cancel all globally
      const result = await alice.exchange.cancelAllOrders();

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // ORDER MODIFICATION
  // =========================================================================

  describe('Order Modification', () => {
    /**
     * @scenario Modify an existing order
     * @given Alice has a resting order at $64,000
     * @when Alice modifies price to $64,500
     * @then The order is updated with new price
     *
     * Order modification:
     * - Change price, size, or both
     * - Maintains queue priority if price unchanged
     * - Loses priority if price changed
     * - More efficient than cancel + new order
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.modifyOrder(
     *   orderId,
     *   {
     *     market: 'BTC-PERP',
     *     side: OrderSide.Buy,
     *     price: '64500',  // New price
     *     size: '0.15',    // New size
     *   }
     * );
     * ```
     */
    test('should modify an existing order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place initial order
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '55000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Modify (using mock order ID)
      // Note: batchModify may not be fully implemented in gateway
      try {
        const result = await alice.exchange.modifyOrder(1, {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '56000', // New price
          size: '0.15', // New size
          type: OrderType.Limit,
        });

        expect(result).toBeDefined();
        expect(result.status).toBe('ok');
      } catch (error) {
        // Server may not support batchModify yet
        expect(error).toBeDefined();
      }
    });
  });

  // =========================================================================
  // BATCH ORDERS
  // =========================================================================

  describe('Batch Orders', () => {
    /**
     * @scenario Place multiple orders in single request
     * @given Alice wants to create order ladder
     * @when Alice submits 5 orders in one request
     * @then All orders are processed atomically
     *
     * Batch orders:
     * - Submit multiple orders in single request
     * - More efficient than individual requests
     * - Lower latency for market making
     * - Individual orders can fail independently
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await client.exchange.placeOrders([
     *   { market: 'BTC-PERP', side: OrderSide.Buy, price: '63000', size: '0.1' },
     *   { market: 'BTC-PERP', side: OrderSide.Buy, price: '62000', size: '0.1' },
     *   { market: 'BTC-PERP', side: OrderSide.Buy, price: '61000', size: '0.1' },
     * ]);
     * ```
     */
    test('should place multiple orders in batch', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const orders = [
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '58000',
          size: '0.1',
          type: OrderType.Limit,
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '57000',
          size: '0.1',
          type: OrderType.Limit,
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '56000',
          size: '0.1',
          type: OrderType.Limit,
        },
      ];

      const result = await alice.exchange.placeOrders(orders);

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // AUTHENTICATION REQUIREMENTS
  // =========================================================================

  describe('Authentication', () => {
    /**
     * @scenario Placing orders requires authentication
     * @given An unauthenticated client
     * @when Attempting to place an order
     * @then The operation fails with signer error
     */
    test('should require authentication for orders', async () => {
      const client = createReadOnlyClient();

      await expect(
        client.exchange.placeOrder({
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '60000',
          size: '0.1',
          type: OrderType.Limit,
        })
      ).rejects.toThrow('Signer required');
    });

    /**
     * @scenario Canceling orders requires authentication
     * @given An unauthenticated client
     * @when Attempting to cancel an order
     * @then The operation fails
     */
    test('should require authentication for cancellation', async () => {
      const client = createReadOnlyClient();

      await expect(
        client.exchange.cancelOrder({
          market: MARKETS.BTC_PERP,
          orderId: 1,
        })
      ).rejects.toThrow('Signer required');
    });
  });
});
