/**
 * Stress & Performance Tests
 *
 * Tests for system behavior under load.
 */

import {
  MARKETS,
  TEST_ACCOUNTS,
  infoRequest,
  exchangeRequest,
  signAction,
  runTest,
  sleep,
  logSection,
  log,
  logProgress,
} from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runStressTests(ctx: TestContext): Promise<void> {
  logSection('13. Stress & Performance Tests');
  log('');
  log('  Testing system under load');
  log('');

  await runTest(ctx, 'Rapid order placement', 'stress', 'Place multiple orders in quick succession', async () => {
    logProgress('Placing 10 orders rapidly...');
    const orderPromises: Promise<unknown>[] = [];

    // Sign each order sequentially with enough delay to ensure unique nonces.
    // Nonces are Date.now() in milliseconds — need sufficient gap to avoid duplicates.
    for (let i = 0; i < 10; i++) {
      const price = 60000 + i * 100;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      // Wrap in catch to prevent unhandled promise rejection before allSettled runs
      orderPromises.push(exchangeRequest(action, signature, nonce).catch((err: unknown) => ({ status: 'error', error: (err as Error).message })));
      if (i < 9) await sleep(5); // 5ms gap to ensure unique Date.now() nonces
    }

    const results = await Promise.allSettled(orderPromises);
    const succeeded = results.filter((r) => {
      if (r.status !== 'fulfilled') return false;
      const val = r.value as { status?: string; error?: string; response?: { data?: { statuses?: Array<{ resting?: unknown; filled?: unknown; error?: string }> } } };
      if (val.status === 'error') return false; // caught error from .catch()
      if (val.status !== 'ok') return false;
      const statuses = val.response?.data?.statuses || [];
      return statuses.some((s) => s.resting || s.filled);
    }).length;
    const failed = 10 - succeeded;
    logProgress(`${succeeded}/10 orders succeeded, ${failed} failed`);

    if (succeeded < 9) {
      throw new Error(`Only ${succeeded}/10 rapid orders succeeded, expected at least 9`);
    }

    // Verify orders actually appear in the book
    await sleep(300);
    const openOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.CHARLIE.address })) as unknown[];
    logProgress(`${openOrders.length} orders visible in book`);
    if (openOrders.length < succeeded - 1) {
      throw new Error(`Expected at least ${succeeded - 1} orders in book, found ${openOrders.length}`);
    }

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    logProgress('Orders cleaned up');
  });

  await runTest(ctx, 'Concurrent API requests', 'stress', 'Make multiple API requests simultaneously', async () => {
    logProgress('Making 20 concurrent requests...');
    const requests = [];

    for (let i = 0; i < 20; i++) {
      requests.push(infoRequest('allMids'));
    }

    const results = await Promise.allSettled(requests);
    const successful = results.filter((r) => {
      if (r.status !== 'fulfilled') return false;
      // Validate response is an object with market prices
      const mids = r.value as Record<string, string>;
      return typeof mids === 'object' && mids !== null;
    }).length;
    logProgress(`${successful}/20 requests returned valid data`);

    if (successful < 19) {
      throw new Error(`Only ${successful}/20 concurrent requests succeeded, expected at least 19`);
    }
  });

  await runTest(ctx, 'Large orderbook query', 'stress', 'Query full orderbook depth', async () => {
    logProgress('Fetching deep orderbook...');
    const start = Date.now();
    const book = await infoRequest('l2Book', { coin: MARKETS.BTC_PERP, nSigFigs: 5 }) as { levels?: Array<Array<{ px: string; sz: string }>> };
    const duration = Date.now() - start;
    if (!book.levels) {
      throw new Error('Orderbook response missing levels field');
    }
    if (!Array.isArray(book.levels) || book.levels.length !== 2) {
      throw new Error(`Expected levels to be [bids, asks], got ${Array.isArray(book.levels) ? book.levels.length : typeof book.levels}`);
    }
    const [bids, asks] = book.levels;
    if (!Array.isArray(bids) || !Array.isArray(asks)) throw new Error('Bids and asks must be arrays');
    logProgress(`Orderbook: ${bids.length} bids, ${asks.length} asks, fetched in ${duration}ms`);
  });
}
