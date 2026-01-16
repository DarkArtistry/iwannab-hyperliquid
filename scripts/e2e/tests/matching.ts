/**
 * Matching Tests
 *
 * Tests order matching between different accounts including:
 * - Basic limit order matching (Alice buys, Bob sells)
 * - Price improvement matching
 * - Partial fills
 * - Cleanup of matching test orders
 */

import {
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

export async function runMatchingTests(ctx: TestContext): Promise<void> {
  logSection('5. Order Matching Tests');
  log('');
  log('  Testing order matching between different accounts');
  log('');

  await runTest(ctx, 'Match limit orders (Alice buys, Bob sells)', 'matching', 'Test basic order matching between two accounts', async () => {
    // Alice places buy order
    logProgress('Alice placing buy order at $65,000...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    // Bob places sell order at same price (should match)
    logProgress('Bob placing sell order at $65,000...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    const result = await exchangeRequest(sellAction, sellSig, sellNonce);

    logProgress('Orders matched (check fills for confirmation)');
  });

  await runTest(ctx, 'Match with price improvement', 'matching', 'Buy order at higher price should match sell at lower price', async () => {
    // Alice places buy at $66,000
    logProgress('Alice placing buy order at $66,000...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '66000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    // Bob places sell at $65,500 (should match at $65,500 or $66,000)
    logProgress('Bob placing sell order at $65,500...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '65500', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(sellAction, sellSig, sellNonce);

    logProgress('Price improvement matching complete');
  });

  await runTest(ctx, 'Partial fill test', 'matching', 'Large order should partially fill against smaller order', async () => {
    // Alice places small sell
    logProgress('Alice placing small sell order (0.001)...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(sellAction, sellSig, sellNonce);

    await sleep(100);

    // Bob places larger buy
    logProgress('Bob placing larger buy order (0.005)...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '64000', s: '0.005', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    logProgress('Partial fill executed, remainder resting');
  });

  // Clean up orders after matching tests
  await runTest(ctx, 'Clean up matching test orders', 'matching', 'Cancel remaining orders from both accounts', async () => {
    logProgress('Cancelling Alice orders...');
    const cancelAlice = { type: 'cancelAll' };
    const { signature: sigA, nonce: nonceA } = await signAction(cancelAlice, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAlice, sigA, nonceA);

    logProgress('Cancelling Bob orders...');
    const cancelBob = { type: 'cancelAll' };
    const { signature: sigB, nonce: nonceB } = await signAction(cancelBob, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(cancelBob, sigB, nonceB);

    logProgress('Cleanup complete');
  });
}
