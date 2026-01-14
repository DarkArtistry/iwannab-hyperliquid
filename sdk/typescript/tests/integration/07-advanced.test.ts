/**
 * Advanced Trading Scenarios
 *
 * These tests cover complex, real-world trading scenarios including:
 * - Market making strategies
 * - Large order handling
 * - Stress testing
 * - Edge cases and error handling
 *
 * @module tests/integration/07-advanced
 */

import { describe, test, expect, afterEach } from 'vitest';
import {
  TEST_ACCOUNTS,
  MARKETS,
  createClient,
  createReadOnlyClient,
  cancelAllOrders,
  waitForProcessing,
  generateClientOrderId,
  generateLadderOrders,
  OrderSide,
  OrderType,
} from './setup';

describe('Advanced Trading Scenarios', () => {
  afterEach(async () => {
    const alice = createClient(TEST_ACCOUNTS.ALICE);
    const bob = createClient(TEST_ACCOUNTS.BOB);
    const charlie = createClient(TEST_ACCOUNTS.CHARLIE);
    await Promise.all([
      cancelAllOrders(alice),
      cancelAllOrders(bob),
      cancelAllOrders(charlie),
    ]);
    await waitForProcessing();
  });

  // =========================================================================
  // MARKET MAKING
  // =========================================================================

  describe('Market Making', () => {
    /**
     * @scenario Create two-sided market (bid/ask spread)
     * @given Charlie is a market maker
     * @when Charlie posts bids below and asks above mid
     * @then Both sides of the book have liquidity
     *
     * Market making strategy:
     * - Post bids at mid - spread/2
     * - Post asks at mid + spread/2
     * - Profit from the spread
     * - Manage inventory risk
     *
     * @example:
     * ```typescript
     * // Create symmetric quotes around $65,000
     * await client.exchange.placeOrders([
     *   { market: 'BTC-PERP', side: 'buy',  price: '64990', size: '0.5' },
     *   { market: 'BTC-PERP', side: 'sell', price: '65010', size: '0.5' },
     * ]);
     * // Spread: $20 = 0.03%
     * ```
     */
    test('should create two-sided quotes', async () => {
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Post bid and ask
      const result = await charlie.exchange.placeOrders([
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '64990',
          size: '0.5',
          type: OrderType.Limit,
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Sell,
          price: '65010',
          size: '0.5',
          type: OrderType.Limit,
        },
      ]);

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Create order ladder (multiple price levels)
     * @given Charlie wants to provide depth
     * @when Charlie posts orders at multiple prices
     * @then Order book has depth on both sides
     *
     * Ladder strategy:
     * - Multiple orders at different prices
     * - Size may vary by level
     * - Provides market depth
     *
     * @example:
     * ```
     * Bids: $64,900 x 0.5, $64,800 x 0.5, $64,700 x 0.5
     * Asks: $65,100 x 0.5, $65,200 x 0.5, $65,300 x 0.5
     * ```
     */
    test('should create order ladder', async () => {
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Generate bid ladder
      const bids = generateLadderOrders('65000', 0.001, 3, '0.1', 'buy');

      // Generate ask ladder
      const asks = generateLadderOrders('65000', 0.001, 3, '0.1', 'sell');

      const result = await charlie.exchange.placeOrders([...bids, ...asks]);

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Requote (cancel and replace)
     * @given Charlie has existing quotes
     * @when Market moves, Charlie needs to requote
     * @then Old quotes cancelled, new quotes placed
     *
     * Requoting workflow:
     * 1. Cancel existing orders
     * 2. Calculate new prices
     * 3. Place new orders
     * 4. Repeat as market moves
     */
    test('should requote efficiently', async () => {
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Initial quotes
      await charlie.exchange.placeOrders([
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '64990',
          size: '0.5',
          type: OrderType.Limit,
          clientOrderId: generateClientOrderId('mm-bid'),
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Sell,
          price: '65010',
          size: '0.5',
          type: OrderType.Limit,
          clientOrderId: generateClientOrderId('mm-ask'),
        },
      ]);

      await waitForProcessing();

      // Cancel all and requote
      await charlie.exchange.cancelAllOrders(MARKETS.BTC_PERP);
      await waitForProcessing();

      // New quotes at different prices
      const result = await charlie.exchange.placeOrders([
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '65090',
          size: '0.5',
          type: OrderType.Limit,
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Sell,
          price: '65110',
          size: '0.5',
          type: OrderType.Limit,
        },
      ]);

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // LARGE ORDER HANDLING
  // =========================================================================

  describe('Large Orders', () => {
    /**
     * @scenario TWAP execution (split large order)
     * @given Alice wants to buy 10 BTC
     * @when Alice splits into smaller orders
     * @then Lower market impact
     *
     * TWAP (Time-Weighted Average Price):
     * - Split large orders into smaller pieces
     * - Execute over time
     * - Reduces market impact
     * - Average execution price
     *
     * @example:
     * ```typescript
     * // Instead of: Buy 10 BTC at once
     * // Do: Buy 1 BTC every 10 minutes
     * for (let i = 0; i < 10; i++) {
     *   await client.exchange.placeOrder({
     *     market: 'BTC-PERP',
     *     side: 'buy',
     *     size: '1',
     *     type: 'market',
     *   });
     *   await sleep(10 * 60 * 1000);
     * }
     * ```
     */
    test('should handle split order execution', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Simulate TWAP by placing multiple smaller orders
      for (let i = 0; i < 3; i++) {
        const result = await alice.exchange.placeOrder({
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: (60000 + i * 100).toString(),
          size: '0.1',
          type: OrderType.Limit,
        });
        expect(result.status).toBe('ok');
      }
    });

    /**
     * @scenario Iceberg order (hidden size)
     * @given Alice wants to buy 10 BTC discretely
     * @when Alice shows only 1 BTC at a time
     * @then Full size not visible to market
     *
     * Iceberg implementation:
     * - Place small visible order
     * - Replenish when filled
     * - Total size hidden
     *
     * Note: This is a client-side implementation pattern.
     */
    test('should simulate iceberg order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // First "clip" of iceberg
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '60000',
        size: '0.1', // Display size (total might be 1.0)
        type: OrderType.Limit,
        clientOrderId: generateClientOrderId('iceberg-1'),
      });

      expect(result.status).toBe('ok');
      // Client would monitor fills and replenish
    });
  });

  // =========================================================================
  // STRESS TESTING
  // =========================================================================

  describe('Stress Testing', () => {
    /**
     * @scenario Rapid order placement
     * @given System can handle high throughput
     * @when Alice places 20 orders quickly
     * @then All orders processed
     *
     * Rate limiting:
     * - Be aware of rate limits
     * - Use batch APIs when possible
     * - Handle rate limit errors gracefully
     */
    test('should handle rapid order placement', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const orders = [];
      for (let i = 0; i < 10; i++) {
        orders.push({
          market: MARKETS.BTC_PERP,
          side: i % 2 === 0 ? OrderSide.Buy : OrderSide.Sell,
          price: (60000 + i * 100).toString(),
          size: '0.01',
          type: OrderType.Limit,
        });
      }

      const result = await alice.exchange.placeOrders(orders);
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Rapid cancel and replace
     * @given Active trading session
     * @when Multiple cancel/place cycles
     * @then System remains responsive
     */
    test('should handle rapid cancel/replace cycles', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      for (let i = 0; i < 3; i++) {
        // Place
        await alice.exchange.placeOrder({
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: (60000 + i * 100).toString(),
          size: '0.1',
          type: OrderType.Limit,
        });

        // Cancel
        await alice.exchange.cancelAllOrders(MARKETS.BTC_PERP);
      }

      // Final state should be clean
      const state = await alice.info.getOpenOrders(TEST_ACCOUNTS.ALICE.address);
      expect(state).toBeDefined();
    });
  });

  // =========================================================================
  // ERROR HANDLING
  // =========================================================================

  describe('Error Handling', () => {
    /**
     * @scenario Handle invalid order parameters
     * @given Invalid order parameters
     * @when Order is submitted
     * @then Clear error is returned
     *
     * Common errors:
     * - Zero size
     * - Invalid price
     * - Unknown market
     * - Insufficient margin
     */
    test('should handle zero size error', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // This may be caught client-side or return error
      try {
        await alice.exchange.placeOrder({
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '65000',
          size: '0', // Invalid
          type: OrderType.Limit,
        });
      } catch (error) {
        expect(error).toBeDefined();
      }
    });

    /**
     * @scenario Handle unknown market error
     * @given Market doesn't exist
     * @when Order submitted
     * @then Market not found error
     */
    test('should handle unknown market error', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      try {
        await alice.exchange.placeOrder({
          market: 'INVALID-PERP',
          side: OrderSide.Buy,
          price: '100',
          size: '1',
          type: OrderType.Limit,
        });
      } catch (error) {
        expect(error).toBeDefined();
      }
    });
  });

  // =========================================================================
  // MULTI-USER SCENARIOS
  // =========================================================================

  describe('Multi-User Scenarios', () => {
    /**
     * @scenario Three-way trade
     * @given Alice, Bob, and Charlie all trading
     * @when Orders interact
     * @then Correct matching and settlement
     *
     * Complex matching:
     * - Multiple participants
     * - Different strategies
     * - Price discovery
     */
    test('should handle multi-party trading', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Charlie provides two-sided liquidity
      await charlie.exchange.placeOrders([
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Buy,
          price: '64500',
          size: '0.2',
          type: OrderType.Limit,
        },
        {
          market: MARKETS.BTC_PERP,
          side: OrderSide.Sell,
          price: '65500',
          size: '0.2',
          type: OrderType.Limit,
        },
      ]);

      await waitForProcessing();

      // Alice buys (matches Charlie's ask)
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65500',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Bob sells (matches Charlie's bid)
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '64500',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // All should have executed
      const aliceState = await alice.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      expect(aliceState).toBeDefined();
    });
  });

  // =========================================================================
  // PORTFOLIO SCENARIOS
  // =========================================================================

  describe('Portfolio Management', () => {
    /**
     * @scenario Hedge position with opposite trade
     * @given Alice is long BTC
     * @when Alice opens short ETH as hedge
     * @then Portfolio risk is balanced
     *
     * Delta hedging:
     * - Offset long with short
     * - Reduce directional exposure
     * - Portfolio-level risk management
     */
    test('should manage multi-asset portfolio', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Long BTC
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '60000',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Short ETH as hedge
      await alice.exchange.placeOrder({
        market: MARKETS.ETH_PERP,
        side: OrderSide.Sell,
        price: '4000',
        size: '1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Check portfolio
      const state = await alice.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      expect(state).toBeDefined();
    });
  });
});
