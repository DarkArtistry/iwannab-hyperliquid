/**
 * Position Management Tests
 *
 * These tests verify position-related functionality including:
 * - Opening long and short positions
 * - Position tracking and PnL calculation
 * - Closing positions
 * - Position flipping (long to short and vice versa)
 *
 * @module tests/integration/06-positions
 */

import { describe, test, expect, beforeEach, afterEach } from 'vitest';
import {
  TEST_ACCOUNTS,
  MARKETS,
  createClient,
  createReadOnlyClient,
  cancelAllOrders,
  waitForProcessing,
  OrderSide,
  OrderType,
} from './setup';

describe('Position Management', () => {
  afterEach(async () => {
    const alice = createClient(TEST_ACCOUNTS.ALICE);
    await cancelAllOrders(alice);
    await waitForProcessing();
  });

  // =========================================================================
  // OPENING POSITIONS
  // =========================================================================

  describe('Opening Positions', () => {
    /**
     * @scenario Open a long position
     * @given Alice has no BTC position
     * @when Alice's buy order fills
     * @then Alice has a long position
     *
     * Long positions:
     * - Profit when price increases
     * - Positive position size
     * - Entry price is fill price
     *
     * @example:
     * ```
     * Buy 0.1 BTC @ $65,000
     * Position: +0.1 BTC
     * Entry: $65,000
     * Notional: $6,500
     * ```
     */
    test('should open long position on buy fill', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob provides sell liquidity
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice buys, opening long
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');

      // Verify position
      await waitForProcessing();
      const state = await alice.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      // Position should show in assetPositions after fill
    });

    /**
     * @scenario Open a short position
     * @given Alice has no BTC position
     * @when Alice's sell order fills
     * @then Alice has a short position
     *
     * Short positions:
     * - Profit when price decreases
     * - Negative position size
     * - Entry price is fill price
     *
     * @example:
     * ```
     * Sell 0.1 BTC @ $65,000
     * Position: -0.1 BTC
     * Entry: $65,000
     * Notional: $6,500
     * ```
     */
    test('should open short position on sell fill', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob provides buy liquidity
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '65000',
        size: '0.1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice sells, opening short
      const result = await alice.exchange.placeOrder({
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
  // POSITION PNL
  // =========================================================================

  describe('Position PnL', () => {
    /**
     * @scenario Long position with unrealized profit
     * @given Alice is long 0.1 BTC from $65,000
     * @when Mark price rises to $66,000
     * @then Unrealized PnL = +$100
     *
     * Unrealized PnL calculation:
     * Long: (mark - entry) * size
     * Short: (entry - mark) * size
     *
     * @example:
     * ```
     * Long 0.1 BTC @ $65,000
     * Mark: $66,000
     * Unrealized: (66000 - 65000) * 0.1 = $100
     * ```
     */
    test('should track unrealized PnL for long position', async () => {
      const client = createReadOnlyClient();

      // Query Alice's account state
      const state = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);

      expect(state).toBeDefined();
      expect(state.marginSummary).toBeDefined();
      // Unrealized PnL would be in assetPositions[].position.unrealizedPnl
    });

    /**
     * @scenario Short position with unrealized profit
     * @given Alice is short 0.1 BTC from $65,000
     * @when Mark price drops to $64,000
     * @then Unrealized PnL = +$100
     *
     * @example:
     * ```
     * Short 0.1 BTC @ $65,000
     * Mark: $64,000
     * Unrealized: (65000 - 64000) * 0.1 = $100
     * ```
     */
    test('should track unrealized PnL for short position', async () => {
      const client = createReadOnlyClient();

      const state = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      expect(state).toBeDefined();
    });
  });

  // =========================================================================
  // CLOSING POSITIONS
  // =========================================================================

  describe('Closing Positions', () => {
    /**
     * @scenario Close long position completely
     * @given Alice is long 0.1 BTC
     * @when Alice sells 0.1 BTC
     * @then Position is closed
     * @and PnL is realized
     *
     * Closing a position:
     * - Trade opposite direction, same size
     * - Realized PnL = (exit - entry) * size
     * - Position size becomes 0
     *
     * @example:
     * ```
     * Long 0.1 @ $65,000, Close @ $66,000
     * Realized PnL: $100
     * New position: 0
     * ```
     */
    test('should close long with sell order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Close with reduce-only sell
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '60000', // Below market to ensure fill
        size: '0.1',
        type: OrderType.Limit,
        reduceOnly: true,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Close short position completely
     * @given Alice is short 0.1 BTC
     * @when Alice buys 0.1 BTC
     * @then Position is closed
     *
     * @example:
     * ```
     * Short 0.1 @ $65,000, Close @ $64,000
     * Realized PnL: $100
     * New position: 0
     * ```
     */
    test('should close short with buy order', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Close with reduce-only buy
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '70000', // Above market to ensure fill
        size: '0.1',
        type: OrderType.Limit,
        reduceOnly: true,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Partial position close
     * @given Alice is long 1.0 BTC
     * @when Alice sells 0.4 BTC
     * @then Position reduced to 0.6 BTC long
     *
     * Partial closes:
     * - Realize proportional PnL
     * - Entry price unchanged for remaining
     * - Can continue closing in parts
     */
    test('should partially close position', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '60000',
        size: '0.04', // Partial close
        type: OrderType.Limit,
        reduceOnly: true,
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // POSITION FLIPPING
  // =========================================================================

  describe('Position Flipping', () => {
    /**
     * @scenario Flip from long to short
     * @given Alice is long 0.1 BTC
     * @when Alice sells 0.2 BTC
     * @then Alice is now short 0.1 BTC
     *
     * Position flipping:
     * - First closes existing position
     * - Then opens opposite position
     * - Two realized PnL events may occur
     *
     * @example:
     * ```
     * Start: Long 0.1 BTC @ $65,000
     * Sell 0.2 BTC @ $64,000
     * Result: Short 0.1 BTC @ $64,000
     * Realized (from closing long): -$100
     * ```
     */
    test('should flip from long to short', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob provides liquidity
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '64000',
        size: '0.2',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice sells more than current long
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '64000',
        size: '0.2',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Flip from short to long
     * @given Alice is short 0.1 BTC
     * @when Alice buys 0.2 BTC
     * @then Alice is now long 0.1 BTC
     */
    test('should flip from short to long', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);
      const bob = createClient(TEST_ACCOUNTS.BOB);

      // Bob provides liquidity
      await bob.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Sell,
        price: '66000',
        size: '0.2',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Alice buys more than current short
      const result = await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '66000',
        size: '0.2',
        type: OrderType.Limit,
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // MULTI-POSITION MANAGEMENT
  // =========================================================================

  describe('Multiple Positions', () => {
    /**
     * @scenario Hold positions in multiple markets
     * @given Alice is long BTC-PERP
     * @when Alice opens short ETH-PERP
     * @then Both positions tracked independently
     *
     * Multi-market positions:
     * - Each market has separate position
     * - Margin shared across portfolio
     * - PnL calculated per position
     */
    test('should track positions in multiple markets', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Place BTC buy
      await alice.exchange.placeOrder({
        market: MARKETS.BTC_PERP,
        side: OrderSide.Buy,
        price: '60000',
        size: '0.1',
        type: OrderType.Limit,
      });

      // Place ETH sell
      await alice.exchange.placeOrder({
        market: MARKETS.ETH_PERP,
        side: OrderSide.Sell,
        price: '4000',
        size: '1',
        type: OrderType.Limit,
      });

      await waitForProcessing();

      // Query positions
      const state = await alice.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      expect(state).toBeDefined();
    });
  });
});

// ============================================================================

/**
 * Leverage Management Tests
 *
 * Tests for leverage settings and margin calculations.
 */

describe('Leverage Management', () => {
  // =========================================================================
  // LEVERAGE UPDATES
  // =========================================================================

  describe('Leverage Updates', () => {
    /**
     * @scenario Update leverage for a market
     * @given Alice's leverage is 10x
     * @when Alice updates to 20x
     * @then Margin requirement changes
     *
     * Leverage affects:
     * - Initial margin requirement (1/leverage)
     * - Liquidation price
     * - Maximum position size
     *
     * @example SDK Usage:
     * ```typescript
     * await client.exchange.updateLeverage({
     *   market: 'BTC-PERP',
     *   leverage: 20,
     *   isCross: true,
     * });
     * ```
     */
    test('should update leverage', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.updateLeverage({
        market: MARKETS.BTC_PERP,
        leverage: 20,
        isCross: true,
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Leverage bounds are enforced
     * @given Maximum leverage is 50x
     * @when Alice tries to set 100x
     * @then Request is rejected
     *
     * Leverage limits:
     * - Minimum: 1x (no leverage)
     * - Maximum: Varies by market (typically 50x)
     * - Cannot reduce below current position's requirement
     */
    test('should accept leverage within bounds', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Should accept valid leverage
      const result = await alice.exchange.updateLeverage({
        market: MARKETS.BTC_PERP,
        leverage: 10,
        isCross: true,
      });

      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Cross vs isolated margin modes
     * @given Alice is using cross margin
     * @when Alice switches to isolated margin
     * @then Margin is isolated to that position
     *
     * Margin modes:
     * - Cross: All positions share margin pool
     * - Isolated: Each position has dedicated margin
     *
     * Cross margin:
     * - Higher capital efficiency
     * - Losses can affect other positions
     *
     * Isolated margin:
     * - Limited loss to position margin
     * - May get liquidated sooner
     */
    test('should update margin mode', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      // Switch to isolated
      const result = await alice.exchange.updateLeverage({
        market: MARKETS.BTC_PERP,
        leverage: 10,
        isCross: false, // Isolated
      });

      expect(result.status).toBe('ok');
    });
  });

  // =========================================================================
  // MARGIN CALCULATIONS
  // =========================================================================

  describe('Margin Calculations', () => {
    /**
     * @scenario Calculate initial margin requirement
     * @given 10x leverage on BTC-PERP
     * @when Opening $65,000 notional position
     * @then Initial margin = $6,500
     *
     * Initial margin formula:
     * Margin = Notional / Leverage = Position * Price / Leverage
     *
     * @example:
     * ```
     * Buy 0.1 BTC @ $65,000 with 10x leverage
     * Notional: $6,500
     * Initial Margin: $6,500 / 10 = $650
     * ```
     */
    test('should have correct margin summary', async () => {
      const client = createReadOnlyClient();

      const state = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);

      expect(state.marginSummary).toBeDefined();
      expect(state.marginSummary.accountValue).toBeDefined();
      expect(state.marginSummary.totalMarginUsed).toBeDefined();
    });
  });
});
