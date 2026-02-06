/**
 * Market Data Tests
 *
 * Tests for read-only market data queries.
 */

import { MARKETS, infoRequest, runTest, logSection, log, logProgress } from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runMarketDataTests(ctx: TestContext): Promise<void> {
  logSection('2. Market Data Tests');
  log('');
  log('  Testing read-only market data queries');
  log('');

  await runTest(ctx, 'Get exchange metadata', 'market-data', 'Retrieve exchange configuration including available markets', async () => {
    logProgress('Fetching exchange metadata...');
    const meta = (await infoRequest('meta')) as { universe?: Array<{ name?: string; szDecimals?: number; maxLeverage?: number }> };
    if (!meta.universe) throw new Error('Missing universe in metadata');
    if (meta.universe.length < 2) throw new Error(`Expected at least 2 markets (BTC-PERP, ETH-PERP), got ${meta.universe.length}`);

    // Validate market structure
    for (const market of meta.universe) {
      if (!market.name || typeof market.name !== 'string') throw new Error(`Market missing name: ${JSON.stringify(market)}`);
      if (market.szDecimals === undefined) throw new Error(`Market ${market.name} missing szDecimals`);
      if (market.maxLeverage === undefined) throw new Error(`Market ${market.name} missing maxLeverage`);
    }
    const names = meta.universe.map((m) => m.name);
    if (!names.includes('BTC-PERP')) throw new Error(`BTC-PERP not found in markets: ${names.join(', ')}`);
    if (!names.includes('ETH-PERP')) throw new Error(`ETH-PERP not found in markets: ${names.join(', ')}`);
    logProgress(`Found ${meta.universe.length} markets: ${names.join(', ')}`);
  });

  await runTest(ctx, 'Get all mid prices', 'market-data', 'Retrieve current mid prices for all markets', async () => {
    logProgress('Fetching mid prices...');
    const mids = (await infoRequest('allMids')) as Record<string, string>;
    if (typeof mids !== 'object' || mids === null) throw new Error(`Expected object for allMids, got ${typeof mids}`);
    // Validate all returned price values are valid positive numbers
    for (const [market, price] of Object.entries(mids)) {
      const parsed = parseFloat(price);
      if (isNaN(parsed) || parsed <= 0) throw new Error(`Invalid mid price for ${market}: "${price}"`);
    }
    const count = Object.keys(mids).length;
    logProgress(`Got valid prices for ${count} markets`);
    if (mids['BTC-PERP']) {
      logProgress(`BTC-PERP mid: $${mids['BTC-PERP']}`);
    }
  });

  await runTest(ctx, 'Get L2 orderbook (BTC-PERP)', 'market-data', 'Retrieve level 2 orderbook with bid/ask depth', async () => {
    logProgress('Fetching BTC-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.BTC_PERP })) as {
      levels?: Array<Array<{ px: string; sz: string; n: number }>>;
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    if (!Array.isArray(book.levels) || book.levels.length !== 2) {
      throw new Error(`Expected levels to be [bids, asks], got ${book.levels.length}-element array`);
    }
    const [bids, asks] = book.levels;
    if (!Array.isArray(bids) || !Array.isArray(asks)) throw new Error('Bids and asks must be arrays');
    // Validate entry structure for any existing levels
    for (const entry of [...bids, ...asks]) {
      if (entry.px === undefined || entry.sz === undefined) throw new Error(`Orderbook entry missing px/sz: ${JSON.stringify(entry)}`);
      if (isNaN(parseFloat(entry.px))) throw new Error(`Invalid price in orderbook: "${entry.px}"`);
      if (isNaN(parseFloat(entry.sz)) || parseFloat(entry.sz) <= 0) throw new Error(`Invalid size in orderbook: "${entry.sz}"`);
    }
    logProgress(`Orderbook: ${bids.length} bids, ${asks.length} asks`);
  });

  await runTest(ctx, 'Get L2 orderbook (ETH-PERP)', 'market-data', 'Retrieve ETH-PERP orderbook depth', async () => {
    logProgress('Fetching ETH-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.ETH_PERP })) as {
      levels?: Array<Array<{ px: string; sz: string; n: number }>>;
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    if (!Array.isArray(book.levels) || book.levels.length !== 2) {
      throw new Error(`Expected levels to be [bids, asks], got ${book.levels.length}-element array`);
    }
    const [bids, asks] = book.levels;
    if (!Array.isArray(bids) || !Array.isArray(asks)) throw new Error('Bids and asks must be arrays');
    for (const entry of [...bids, ...asks]) {
      if (entry.px === undefined || entry.sz === undefined) throw new Error(`Orderbook entry missing px/sz: ${JSON.stringify(entry)}`);
    }
    logProgress(`ETH-PERP orderbook: ${bids.length} bids, ${asks.length} asks`);
  });

  await runTest(ctx, 'Get recent trades', 'market-data', 'Retrieve recent trade history for a market', async () => {
    logProgress('Fetching recent trades...');
    const trades = (await infoRequest('recentTrades', { coin: MARKETS.BTC_PERP })) as Array<{ coin?: string; side?: string; px?: string; sz?: string; time?: number }>;
    if (!Array.isArray(trades)) throw new Error(`Expected array for recentTrades, got ${typeof trades}`);
    // Validate structure of any returned trades
    for (const trade of trades) {
      if (trade.px === undefined || trade.sz === undefined) throw new Error(`Trade missing px/sz: ${JSON.stringify(trade)}`);
      if (isNaN(parseFloat(trade.px)) || parseFloat(trade.px) <= 0) throw new Error(`Invalid trade price: "${trade.px}"`);
      if (isNaN(parseFloat(trade.sz)) || parseFloat(trade.sz) <= 0) throw new Error(`Invalid trade size: "${trade.sz}"`);
      if (trade.side !== undefined && trade.side !== 'B' && trade.side !== 'A') throw new Error(`Invalid trade side: "${trade.side}"`);
    }
    logProgress(`Found ${trades.length} valid recent trades`);
  });

  await runTest(ctx, 'Get funding rates', 'market-data', 'Retrieve current funding rate for perpetual markets', async () => {
    logProgress('Fetching funding rates...');
    const funding = (await infoRequest('fundingHistory', { coin: MARKETS.BTC_PERP })) as Array<{ coin?: string; fundingRate?: string; premium?: string; time?: number }>;
    if (!Array.isArray(funding)) {
      throw new Error(`Expected array for fundingHistory, got ${typeof funding}`);
    }
    // Validate structure of returned entries
    for (const entry of funding) {
      if (entry.fundingRate === undefined) throw new Error(`Funding entry missing fundingRate: ${JSON.stringify(entry)}`);
      if (isNaN(parseFloat(entry.fundingRate))) throw new Error(`Invalid fundingRate: "${entry.fundingRate}"`);
      if (entry.time !== undefined && (typeof entry.time !== 'number' || entry.time <= 0)) {
        throw new Error(`Invalid funding timestamp: ${entry.time}`);
      }
    }
    logProgress(`Funding rate data: ${funding.length} valid entries`);
  });

  await runTest(ctx, 'Get candles (1h)', 'market-data', 'Retrieve OHLCV candlestick data', async () => {
    logProgress('Fetching 1h candles...');
    const candles = (await infoRequest('candleSnapshot', {
      coin: MARKETS.BTC_PERP,
      interval: '1h',
      startTime: Date.now() - 86400000,
      endTime: Date.now(),
    })) as Array<{ t?: number; T?: number; o?: string; h?: string; l?: string; c?: string; v?: string }>;
    if (!Array.isArray(candles)) {
      throw new Error(`Expected array for candleSnapshot, got ${typeof candles}`);
    }
    // Validate OHLCV structure for any returned candles
    for (const candle of candles) {
      if (candle.o === undefined || candle.h === undefined || candle.l === undefined || candle.c === undefined) {
        throw new Error(`Candle missing OHLC fields: ${JSON.stringify(candle)}`);
      }
      const o = parseFloat(candle.o), h = parseFloat(candle.h), l = parseFloat(candle.l), c = parseFloat(candle.c);
      if ([o, h, l, c].some(isNaN)) throw new Error(`Non-numeric candle values: ${JSON.stringify(candle)}`);
      if (h < l) throw new Error(`Candle high (${h}) < low (${l})`);
    }
    logProgress(`Candle data: ${candles.length} valid candles`);
  });
}
