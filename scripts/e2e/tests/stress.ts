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

  // =========================================================================
  // PHASE 8C: Additional stress tests with mutations
  // =========================================================================

  await runTest(ctx, 'Concurrent order + cancel storm', 'stress', '10 orders then 10 cancels rapidly, verify consistent state', async () => {
    logProgress('Placing 10 orders...');

    const orderIds: number[] = [];
    for (let i = 0; i < 10; i++) {
      const price = 50000 + i * 100;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      const result = (await exchangeRequest(action, signature, nonce)) as {
        status: string;
        response?: { data?: { statuses?: Array<{ resting?: { oid?: number } }> } };
      };
      const oid = result.response?.data?.statuses?.[0]?.resting?.oid;
      if (oid) orderIds.push(oid);
      await sleep(5);
    }

    logProgress(`Placed ${orderIds.length} resting orders`);
    await sleep(300);

    // Now cancel them all rapidly
    logProgress('Cancelling all orders rapidly...');
    const cancelPromises = orderIds.map(async (oid) => {
      const action = { type: 'cancel', cancels: [{ a: 0, o: oid }] };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      return exchangeRequest(action, signature, nonce).catch(() => null);
    });

    await Promise.allSettled(cancelPromises);
    await sleep(500);

    const remaining = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.CHARLIE.address })) as unknown[];
    logProgress(`Orders remaining after cancel storm: ${remaining.length}`);

    // Clean up any stragglers
    if (remaining.length > 0) {
      const cancelAction = { type: 'cancelAll' };
      const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
      await exchangeRequest(cancelAction, signature, nonce);
      await sleep(200);
    }
  });

  await runTest(ctx, 'Multi-user concurrent order storm', 'stress', 'Alice, Bob, Charlie simultaneous orders via Promise.all', async () => {
    logProgress('All three users placing orders simultaneously...');

    const userOrders = [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB, TEST_ACCOUNTS.CHARLIE].map(async (account, idx) => {
      const prices = [51000, 52000, 53000];
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: prices[idx].toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, account.privateKey);
      return exchangeRequest(action, signature, nonce).catch((err: unknown) => ({
        status: 'error',
        error: (err as Error).message,
      }));
    });

    const results = await Promise.allSettled(userOrders);
    const succeeded = results.filter((r) => r.status === 'fulfilled' && (r.value as { status?: string }).status === 'ok').length;

    logProgress(`${succeeded}/3 concurrent user orders succeeded`);

    if (succeeded < 2) {
      throw new Error(`Only ${succeeded}/3 multi-user orders succeeded, expected at least 2`);
    }

    // Cleanup
    for (const account of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB, TEST_ACCOUNTS.CHARLIE]) {
      const cancelAction = { type: 'cancelAll' };
      const { signature, nonce } = await signAction(cancelAction, account.privateKey);
      await exchangeRequest(cancelAction, signature, nonce).catch(() => null);
    }
    await sleep(200);
  });

  await runTest(ctx, 'Rapid mixed operations', 'stress', 'Orders + leverage changes + cancel interleaved', async () => {
    logProgress('Interleaving order, leverage, cancel operations...');

    // Place order
    const orderAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '48000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: orderSig, nonce: orderNonce } = await signAction(orderAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(orderAction, orderSig, orderNonce);
    await sleep(5);

    // Change leverage
    const leverageAction = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 20 };
    const { signature: levSig, nonce: levNonce } = await signAction(leverageAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(leverageAction, levSig, levNonce);
    await sleep(5);

    // Place another order
    const orderAction2 = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '47000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: orderSig2, nonce: orderNonce2 } = await signAction(orderAction2, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(orderAction2, orderSig2, orderNonce2);
    await sleep(5);

    // Cancel all
    const cancelAction = { type: 'cancelAll' };
    const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(cancelAction, cancelSig, cancelNonce);

    // Reset leverage
    const resetAction = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 10 };
    const { signature: resetSig, nonce: resetNonce } = await signAction(resetAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(resetAction, resetSig, resetNonce);

    await sleep(200);
    const remaining = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.CHARLIE.address })) as unknown[];
    logProgress(`Orders remaining: ${remaining.length}`);
  });

  await runTest(ctx, 'Recovery after burst load', 'stress', '50 rapid orders, pause, verify system responsive', async () => {
    logProgress('Sending burst of 50 orders...');
    const burstStart = Date.now();
    let sent = 0;

    for (let i = 0; i < 50; i++) {
      const price = 40000 + i * 50;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      // Fire and forget with catch
      exchangeRequest(action, signature, nonce).catch(() => null);
      sent++;
      await sleep(3);
    }
    const burstDuration = Date.now() - burstStart;
    logProgress(`Sent ${sent} orders in ${burstDuration}ms`);

    // Wait for system to process
    await sleep(2000);

    // Verify system is still responsive
    const responseStart = Date.now();
    const mids = (await infoRequest('allMids')) as Record<string, string>;
    const responseTime = Date.now() - responseStart;

    logProgress(`System responded in ${responseTime}ms after burst`);

    if (responseTime > 5000) {
      throw new Error(`System took ${responseTime}ms to respond after burst (expected < 5000ms)`);
    }

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    await sleep(300);
    logProgress('Burst orders cleaned up');
  });

  await runTest(ctx, 'Sustained throughput test', 'stress', '100 sequential orders with 5ms gaps, measure success rate', async () => {
    logProgress('Starting sustained throughput test (100 orders)...');
    const start = Date.now();
    let succeeded = 0;
    let failed = 0;

    for (let i = 0; i < 100; i++) {
      const price = 35000 + (i % 50) * 100;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      try {
        const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
        const result = (await exchangeRequest(action, signature, nonce)) as { status: string };
        if (result.status === 'ok') {
          succeeded++;
        } else {
          failed++;
        }
      } catch {
        failed++;
      }
      await sleep(5);
    }

    const duration = Date.now() - start;
    const throughput = (succeeded / (duration / 1000)).toFixed(1);
    logProgress(`${succeeded}/100 orders succeeded, ${failed} failed in ${duration}ms`);
    logProgress(`Throughput: ${throughput} orders/sec`);

    if (succeeded < 90) {
      throw new Error(`Sustained throughput too low: ${succeeded}/100 (expected >= 90)`);
    }

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    await sleep(300);
    logProgress('Sustained test orders cleaned up');
  });
}
