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
    const orders = [];

    for (let i = 0; i < 10; i++) {
      const price = 60000 + i * 100;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      orders.push(exchangeRequest(action, signature, nonce));
    }

    await Promise.allSettled(orders);
    logProgress('All orders submitted');

    // Cleanup
    await sleep(500);
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
    const successful = results.filter((r) => r.status === 'fulfilled').length;
    logProgress(`${successful}/20 requests successful`);
  });

  await runTest(ctx, 'Large orderbook query', 'stress', 'Query full orderbook depth', async () => {
    logProgress('Fetching deep orderbook...');
    const start = Date.now();
    const book = await infoRequest('l2Book', { coin: MARKETS.BTC_PERP, nSigFigs: 5 });
    const duration = Date.now() - start;
    logProgress(`Orderbook fetched in ${duration}ms`);
  });
}
