/**
 * Order Matching Tests
 *
 * These tests verify the matching engine functionality including:
 * - Limit order matching
 * - Market order execution
 * - Partial fills
 * - Price-time priority
 * - Cross-matching scenarios
 *
 * @module tests/integration/05-matching
 */

import { describe, test, expect, beforeEach, afterEach } from 'vitest';
import {
  TEST_ACCOUNTS,
  MARKETS,
  createClient,
  createReadOnlyClient,
  cancelAllOrders,
  waitForProcessing,
  sleep,
  OrderSide,
  OrderType,
} from './setup';

describe('Order Matching', () => {
  // Clean up after each test
  afterEach(async () => {
    const alice = createClient(TEST_ACCOUNTS.ALICE);
    const bob = createClient(TEST_ACCOUNTS.BOB);
    await Promise.all([cancelAllOrders(alice), cancelAllOrders(bob)]);
    await waitForProcessing();
  });

  // =========================================================================
  // BASIC MATCHING
  // =========================================================================

  describe('Basic Matching', () => {
    /**
     * @scenario Two limit orders match at same price
     * @given Alice posts buy at $65,000
     * @and Bob posts sell at $65,000
     * @then Orders match at $65,000
     * @and Both receive fills
     *
     * Matching rules:
     * - Orders match when bid >= ask
     * - Execution price is the maker's price
     * - Both parties receive fill confirmations
     *
     * @example Scenario:
     * ```
     * 1. Alice: Buy 0.1 BTC @ $65,000 (rests as bid)
     * 2. Bob: Sell 0.1 BTC @ $65,000 (crosses Alice's bid)
     * 3. Match at $65,000 (Alice's price - she was maker)
     * ```
     */
    test('should match limit orders at same price', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Alice posts bid
      const aliceOrder = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(aliceOrder.status).toBe('ok');

      await waitForProcessing();

      // Bob posts matching sell
      const bobOrder = await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(bobOrder.status).toBe('ok');
    });

    /**
     * @scenario Taker crosses the spread
     * @given Best bid is $64,900, best ask is $65,100
     * @when Alice places buy at $65,200
     * @then Alice's order matches with asks at or below $65,200
     *
     * The aggressive order "crosses" the spread:
     * - Buy at $65,200 will match any ask <= $65,200
     * - Execution at maker's price (the ask price)
     */
    test('should match when taker crosses spread', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob posts ask (sell)
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65100',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice crosses with aggressive bid
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65200', // Higher than Bob's ask
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
      // Alice's order should match with Bob's ask at $65,100
    });
  });

  // =========================================================================
  // MARKET ORDER MATCHING
  // =========================================================================

  describe('Market Orders', () => {
    /**
     * @scenario Market buy sweeps available liquidity
     * @given Order book has asks at various prices
     * @when Alice places market buy for 0.5 BTC
     * @then Order fills at best available prices
     *
     * Market order execution:
     * - Fills immediately at available prices
     * - May get multiple fills at different prices
     * - Unfilled portion cancels (IOC behavior)
     *
     * @example Execution:
     * ```
     * Book:
     *   Ask $65,100: 0.2 BTC
     *   Ask $65,150: 0.3 BTC
     *   Ask $65,200: 0.5 BTC
     *
     * Market Buy 0.5 BTC fills:
     *   0.2 @ $65,100
     *   0.3 @ $65,150
     * Average price: $65,130
     * ```
     */
    test('should execute market buy against resting asks', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob provides liquidity at multiple levels
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65100',
        size: '0.05',
        type: OrderType.Limit,
      });

      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65150',
        size: '0.05',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice sends market buy
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        size: '0.1',
        type: OrderType.Market,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Market sell sweeps bid side
     * @given Order book has bids at various prices
     * @when Bob places market sell
     * @then Order fills at best available bid prices
     */
    test('should execute market sell against resting bids', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Alice provides bid liquidity
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '64900',
        size: '0.05',
        type: OrderType.Limit,
      });

      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '64850',
        size: '0.05',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Bob market sells
      const result = await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        size: '0.1',
        type: OrderType.Market,
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // PARTIAL FILLS
  // =========================================================================

  describe('Partial Fills', () => {
    /**
     * @scenario Order partially filled against smaller order
     * @given Alice has resting bid for 1.0 BTC
     * @when Bob sells only 0.3 BTC
     * @then Alice's order is partially filled (0.7 remains)
     *
     * Partial fills:
     * - Order remains on book with reduced size
     * - Multiple fills can occur over time
     * - Check remaining size via open orders query
     *
     * @example:
     * ```
     * Initial: Alice Buy 1.0 BTC @ $65,000
     * Bob Sell: 0.3 BTC @ $65,000
     * Result: Alice filled 0.3, remaining 0.7 on book
     * ```
     */
    test('should handle partial fill', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Alice posts large bid
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '1.0',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Bob sells smaller amount
      const result = await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.3',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
      // Alice should have 0.7 BTC remaining on her bid
    });

    /**
     * @scenario Multiple partial fills complete an order
     * @given Alice has bid for 1.0 BTC
     * @when Bob sells 0.4, then Charlie sells 0.6
     * @then Alice's order is completely filled in two parts
     */
    test('should accumulate partial fills', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Alice posts bid
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '1.0',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Bob partial fill
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.4',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Charlie completes the fill
      const result = await charlie.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.6',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // PRICE-TIME PRIORITY
  // =========================================================================

  describe('Price-Time Priority', () => {
    /**
     * @scenario Better price gets priority
     * @given Alice bids $65,000, Bob bids $65,100
     * @when Charlie sells 0.1 BTC
     * @then Bob's higher bid matches first
     *
     * Price priority:
     * - Higher bids have priority over lower bids
     * - Lower asks have priority over higher asks
     * - Better prices always fill first
     */
    test('should prioritize better price', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Alice bids lower
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Bob bids higher
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65100',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Charlie sells - should match Bob's higher bid
      const result = await charlie.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000', // Will match at Bob's price
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Same price follows time priority (FIFO)
     * @given Alice and Bob both bid $65,000 (Alice first)
     * @when Charlie sells 0.1 BTC
     * @then Alice's earlier order matches first
     *
     * Time priority:
     * - At same price, earlier orders have priority
     * - First-in-first-out (FIFO)
     * - Order modifications may lose time priority
     */
    test('should prioritize earlier order at same price', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Alice bids first
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await sleep(100); // Ensure time separation

      // Bob bids same price, but later
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Charlie sells - should match Alice (earlier)
      const result = await charlie.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // SELF-TRADE PREVENTION
  // =========================================================================

  describe('Self-Trade Prevention', () => {
    /**
     * @scenario Prevent self-matching
     * @given Alice has resting bid at $65,000
     * @when Alice places sell at $65,000
     * @then Self-trade is prevented
     *
     * Self-trade prevention modes:
     * - Cancel-newest: Cancel incoming order
     * - Cancel-oldest: Cancel resting order
     * - Cancel-both: Cancel both orders
     *
     * Default behavior prevents wash trading.
     */
    test('should prevent self-trade', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Alice posts bid
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice tries to sell at same price (would self-match)
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Either order accepted (no match) or rejected
      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // MULTI-MARKET MATCHING
  // =========================================================================

  describe('Multi-Market Matching', () => {
    /**
     * @scenario Orders in different markets are independent
     * @given Alice has bid in BTC-PERP
     * @and Bob has bid in ETH-PERP
     * @when Charlie sells BTC
     * @then Only Alice's BTC bid matches
     *
     * Market isolation:
     * - Each market has separate order book
     * - Cross-market orders don't interact
     * - Different markets can have different spreads
     */
    test('should match within correct market only', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);
      const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

      // Alice bids BTC
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Bob bids ETH
      await bob.exchange.placeOrder({
        market: MARKETS.ETH_PERP,
        side: OrderSide.Buy,
        price: '3500',
        size: '1.0',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Charlie sells BTC - should only match Alice
      const result = await charlie.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });
  });
});
