/**
 * Market Data Query Tests
 *
 * These tests verify read-only market data queries including:
 * - Order book data (L2)
 * - Mid prices
 * - Market metadata
 * - Recent trades
 * - Candle data
 *
 * All these operations can be performed without authentication.
 *
 * @module tests/integration/02-market-data
 */

import { describe, test, expect } from 'vitest';
import { createReadOnlyClient, MARKETS } from './setup';

describe('Market Data Queries', () => {
  // =========================================================================
  // ORDER BOOK
  // =========================================================================

  describe('Order Book (L2)', () => {
    /**
     * @scenario Fetch BTC-PERP order book
     * @given The BTC-PERP market is active
     * @when We request the L2 order book
     * @then We receive bid and ask levels
     *
     * The L2 order book shows aggregated price levels:
     * - Bids: Buy orders (sorted highest to lowest)
     * - Asks: Sell orders (sorted lowest to highest)
     *
     * @example API Response:
     * ```json
     * {
     *   "coin": "BTC-PERP",
     *   "levels": [
     *     [["64990", "1.5", 3], ["64980", "2.0", 5]],  // Bids
     *     [["65010", "1.0", 2], ["65020", "3.0", 4]]   // Asks
     *   ]
     * }
     * ```
     */
    test('should fetch BTC-PERP order book', async () => {
      const client = createReadOnlyClient();

      const book = await client.info.getL2Book(MARKETS.BTC_PERP);

      expect(book).toBeDefined();
      expect(book.levels).toBeDefined();
      expect(Array.isArray(book.levels)).toBe(true);
      expect(book.levels.length).toBe(2); // [bids, asks]
    });

    /**
     * @scenario Fetch ETH-PERP order book
     * @given The ETH-PERP market is active
     * @when We request the L2 order book
     * @then We receive bid and ask levels
     */
    test('should fetch ETH-PERP order book', async () => {
      const client = createReadOnlyClient();

      const book = await client.info.getL2Book(MARKETS.ETH_PERP);

      expect(book).toBeDefined();
      expect(book.levels).toBeDefined();
    });

    /**
     * @scenario Order book price levels are properly sorted
     * @given The order book has multiple price levels
     * @when We examine the bid and ask arrays
     * @then Bids are sorted descending, asks are sorted ascending
     *
     * This ensures the best prices are at index 0:
     * - bids[0] = highest bid (best buy price)
     * - asks[0] = lowest ask (best sell price)
     */
    test('order book should have proper structure', async () => {
      const client = createReadOnlyClient();

      const book = await client.info.getL2Book(MARKETS.BTC_PERP);
      const [bids, asks] = book.levels;

      // If there are multiple bid levels, verify descending order
      if (bids.length >= 2) {
        const bid0 = parseFloat(bids[0].px);
        const bid1 = parseFloat(bids[1].px);
        expect(bid0).toBeGreaterThanOrEqual(bid1);
      }

      // If there are multiple ask levels, verify ascending order
      if (asks.length >= 2) {
        const ask0 = parseFloat(asks[0].px);
        const ask1 = parseFloat(asks[1].px);
        expect(ask0).toBeLessThanOrEqual(ask1);
      }
    });
  });

  // =========================================================================
  // MID PRICES
  // =========================================================================

  describe('Mid Prices', () => {
    /**
     * @scenario Fetch all market mid prices
     * @given Multiple markets are active
     * @when We request all mid prices
     * @then We receive prices for each market
     *
     * Mid price = (best bid + best ask) / 2
     * Used for:
     * - Mark price calculation
     * - PnL estimation
     * - Portfolio valuation
     *
     * @example API Response:
     * ```json
     * {
     *   "BTC-PERP": "65000.5",
     *   "ETH-PERP": "3500.25"
     * }
     * ```
     */
    test('should fetch all mid prices', async () => {
      const client = createReadOnlyClient();

      const mids = await client.info.getAllMids();

      expect(mids).toBeDefined();
      expect(typeof mids).toBe('object');
    });

    /**
     * @scenario Mid prices are reasonable values
     * @given Markets have liquidity
     * @when We fetch mid prices
     * @then Prices are positive numbers
     */
    test('mid prices should be valid numbers', async () => {
      const client = createReadOnlyClient();

      const mids = await client.info.getAllMids();

      for (const [market, price] of Object.entries(mids)) {
        const numPrice = parseFloat(price);
        expect(numPrice).toBeGreaterThan(0);
        expect(Number.isFinite(numPrice)).toBe(true);
      }
    });
  });

  // =========================================================================
  // MARKET METADATA
  // =========================================================================

  describe('Market Metadata', () => {
    /**
     * @scenario Fetch exchange metadata
     * @given The exchange is operational
     * @when We request metadata
     * @then We receive market configurations
     *
     * Metadata includes:
     * - Market names and symbols
     * - Size decimals (precision)
     * - Maximum leverage
     * - Tick size (price increment)
     * - Lot size (minimum order size)
     * - Fee structure
     *
     * @example API Response:
     * ```json
     * {
     *   "universe": [
     *     {
     *       "name": "BTC-PERP",
     *       "szDecimals": 8,
     *       "maxLeverage": 50,
     *       "tickSize": "0.1",
     *       "lotSize": "0.0001",
     *       "makerFee": "-0.0002",
     *       "takerFee": "0.0005"
     *     }
     *   ]
     * }
     * ```
     */
    test('should fetch market metadata', async () => {
      const client = createReadOnlyClient();

      const meta = await client.info.getMeta();

      expect(meta).toBeDefined();
      expect(meta.universe).toBeDefined();
      expect(Array.isArray(meta.universe)).toBe(true);
    });

    /**
     * @scenario BTC-PERP market is properly configured
     * @given BTC-PERP market exists
     * @when We fetch its configuration
     * @then It has reasonable trading parameters
     */
    test('BTC-PERP should have valid configuration', async () => {
      const client = createReadOnlyClient();

      const meta = await client.info.getMeta();
      const btc = meta.universe.find((m) => m.name === 'BTC-PERP');

      expect(btc).toBeDefined();
      if (btc) {
        expect(btc.szDecimals).toBeGreaterThan(0);
        expect(btc.maxLeverage).toBeGreaterThanOrEqual(1);
        expect(btc.maxLeverage).toBeLessThanOrEqual(100);
      }
    });

    /**
     * @scenario ETH-PERP market is properly configured
     * @given ETH-PERP market exists
     * @when We fetch its configuration
     * @then It has reasonable trading parameters
     */
    test('ETH-PERP should have valid configuration', async () => {
      const client = createReadOnlyClient();

      const meta = await client.info.getMeta();
      const eth = meta.universe.find((m) => m.name === 'ETH-PERP');

      expect(eth).toBeDefined();
      if (eth) {
        expect(eth.szDecimals).toBeGreaterThan(0);
        expect(eth.maxLeverage).toBeGreaterThanOrEqual(1);
      }
    });
  });

  // =========================================================================
  // RECENT TRADES
  // =========================================================================

  describe('Recent Trades', () => {
    /**
     * @scenario Fetch recent trades for BTC-PERP
     * @given Trades have occurred in BTC-PERP
     * @when We request recent trades
     * @then We receive an array of trade records
     *
     * Trade record includes:
     * - coin: Market symbol
     * - side: 'B' (buy) or 'A' (ask/sell)
     * - px: Execution price
     * - sz: Trade size
     * - time: Unix timestamp (ms)
     * - hash: Transaction hash
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "coin": "BTC-PERP",
     *     "side": "B",
     *     "px": "65000",
     *     "sz": "0.1",
     *     "time": 1705123456789,
     *     "hash": "0x..."
     *   }
     * ]
     * ```
     */
    test('should fetch recent trades', async () => {
      const client = createReadOnlyClient();

      const trades = await client.info.getRecentTrades(MARKETS.BTC_PERP);

      expect(trades).toBeDefined();
      expect(Array.isArray(trades)).toBe(true);
    });

    /**
     * @scenario Recent trades are properly formatted
     * @given There are recent trades
     * @when We examine trade records
     * @then Each trade has required fields
     */
    test('trade records should have valid structure', async () => {
      const client = createReadOnlyClient();

      const trades = await client.info.getRecentTrades(MARKETS.BTC_PERP, 10);

      for (const trade of trades) {
        if (trade.px) {
          expect(parseFloat(trade.px)).toBeGreaterThan(0);
        }
        if (trade.sz) {
          expect(parseFloat(trade.sz)).toBeGreaterThan(0);
        }
        if (trade.side) {
          expect(['B', 'A']).toContain(trade.side);
        }
      }
    });
  });

  // =========================================================================
  // CANDLE DATA
  // =========================================================================

  describe('Candle Data (OHLCV)', () => {
    /**
     * @scenario Fetch 1-hour candles for BTC-PERP
     * @given Historical price data exists
     * @when We request 1h candles
     * @then We receive OHLCV data
     *
     * Candle data includes:
     * - t: Time (Unix timestamp)
     * - o: Open price
     * - h: High price
     * - l: Low price
     * - c: Close price
     * - v: Volume
     *
     * Available intervals: 1m, 5m, 15m, 1h, 4h, 1d
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "t": 1705120000000,
     *     "o": "64900",
     *     "h": "65100",
     *     "l": "64800",
     *     "c": "65000",
     *     "v": "1234.56"
     *   }
     * ]
     * ```
     */
    test('should fetch 1h candles', async () => {
      const client = createReadOnlyClient();

      const candles = await client.info.getCandles(MARKETS.BTC_PERP, '1h');

      expect(candles).toBeDefined();
      expect(Array.isArray(candles)).toBe(true);
    });

    /**
     * @scenario Candle data has valid OHLC relationships
     * @given Candle data is returned
     * @when We examine each candle
     * @then High >= Open, Close, Low and Low <= Open, Close, High
     */
    test('candle OHLC values should be consistent', async () => {
      const client = createReadOnlyClient();

      const candles = await client.info.getCandles(MARKETS.BTC_PERP, '1h');

      for (const candle of candles) {
        const o = parseFloat(candle.o);
        const h = parseFloat(candle.h);
        const l = parseFloat(candle.l);
        const c = parseFloat(candle.c);

        // High should be >= all other values
        expect(h).toBeGreaterThanOrEqual(o);
        expect(h).toBeGreaterThanOrEqual(l);
        expect(h).toBeGreaterThanOrEqual(c);

        // Low should be <= all other values
        expect(l).toBeLessThanOrEqual(o);
        expect(l).toBeLessThanOrEqual(h);
        expect(l).toBeLessThanOrEqual(c);
      }
    });

    /**
     * @scenario Different candle intervals are supported
     * @given The API supports multiple intervals
     * @when We request different intervals
     * @then Each returns valid data
     */
    test('should support multiple intervals', async () => {
      const client = createReadOnlyClient();
      const intervals = ['1m', '5m', '15m', '1h', '4h', '1d'] as const;

      for (const interval of intervals) {
        const candles = await client.info.getCandles(MARKETS.BTC_PERP, interval);
        expect(candles).toBeDefined();
        expect(Array.isArray(candles)).toBe(true);
      }
    });
  });

  // =========================================================================
  // FUNDING RATES
  // =========================================================================

  describe('Funding Rates', () => {
    /**
     * @scenario Fetch funding rate history
     * @given Funding payments have occurred
     * @when We request funding history
     * @then We receive funding rate records
     *
     * Funding payments occur every 8 hours.
     * Positive rate = longs pay shorts
     * Negative rate = shorts pay longs
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "coin": "BTC-PERP",
     *     "fundingRate": "0.0001",
     *     "premium": "0.00005",
     *     "time": 1705123200000
     *   }
     * ]
     * ```
     */
    test('should fetch funding history', async () => {
      const client = createReadOnlyClient();

      const funding = await client.info.getFundingHistory(MARKETS.BTC_PERP);

      expect(funding).toBeDefined();
      expect(Array.isArray(funding)).toBe(true);
    });
  });
});
