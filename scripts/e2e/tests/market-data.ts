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
    const meta = (await infoRequest('meta')) as { universe?: unknown[] };
    if (!meta.universe) throw new Error('Missing universe in metadata');
    logProgress(`Found ${meta.universe.length} markets`);
  });

  await runTest(ctx, 'Get all mid prices', 'market-data', 'Retrieve current mid prices for all markets', async () => {
    logProgress('Fetching mid prices...');
    const mids = (await infoRequest('allMids')) as Record<string, string>;
    const count = Object.keys(mids).length;
    logProgress(`Got prices for ${count} markets`);
    if (mids['BTC-PERP']) {
      logProgress(`BTC-PERP mid: $${mids['BTC-PERP']}`);
    }
  });

  await runTest(ctx, 'Get L2 orderbook (BTC-PERP)', 'market-data', 'Retrieve level 2 orderbook with bid/ask depth', async () => {
    logProgress('Fetching BTC-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.BTC_PERP })) as {
      levels?: [unknown[], unknown[]];
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    const [bids, asks] = book.levels;
    logProgress(`Orderbook: ${bids.length} bids, ${asks.length} asks`);
  });

  await runTest(ctx, 'Get L2 orderbook (ETH-PERP)', 'market-data', 'Retrieve ETH-PERP orderbook depth', async () => {
    logProgress('Fetching ETH-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.ETH_PERP })) as {
      levels?: [unknown[], unknown[]];
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    logProgress('ETH-PERP orderbook retrieved');
  });

  await runTest(ctx, 'Get recent trades', 'market-data', 'Retrieve recent trade history for a market', async () => {
    logProgress('Fetching recent trades...');
    const trades = (await infoRequest('recentTrades', { coin: MARKETS.BTC_PERP })) as unknown[];
    logProgress(`Found ${trades.length} recent trades`);
  });

  await runTest(ctx, 'Get funding rates', 'market-data', 'Retrieve current funding rate for perpetual markets', async () => {
    logProgress('Fetching funding rates...');
    const funding = await infoRequest('fundingHistory', { coin: MARKETS.BTC_PERP });
    if (!Array.isArray(funding)) {
      throw new Error(`Expected array for fundingHistory, got ${typeof funding}`);
    }
    logProgress(`Funding rate data retrieved: ${funding.length} entries`);
  });

  await runTest(ctx, 'Get candles (1h)', 'market-data', 'Retrieve OHLCV candlestick data', async () => {
    logProgress('Fetching 1h candles...');
    const candles = await infoRequest('candleSnapshot', {
      coin: MARKETS.BTC_PERP,
      interval: '1h',
      startTime: Date.now() - 86400000,
      endTime: Date.now(),
    });
    if (!Array.isArray(candles)) {
      throw new Error(`Expected array for candleSnapshot, got ${typeof candles}`);
    }
    logProgress(`Candle data retrieved: ${candles.length} candles`);
  });
}
