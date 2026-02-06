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
    await exchangeRequest(sellAction, sellSig, sellNonce);

    // Verify fills occurred
    await sleep(500);
    const fills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ px: string; sz: string; side: string; coin?: string }>;
    if (fills.length === 0) {
      throw new Error('No fills found after matching buy and sell at $65,000');
    }
    // Validate the most recent fill matches our order
    const latestFill = fills[0];
    const fillPrice = parseFloat(latestFill.px);
    const fillSize = parseFloat(latestFill.sz);
    if (fillPrice !== 65000) throw new Error(`Expected fill price 65000, got ${fillPrice}`);
    if (Math.abs(fillSize - 0.001) > 0.0001) throw new Error(`Expected fill size ~0.001, got ${fillSize}`);
    logProgress(`Matched at price=$${latestFill.px}, size=${latestFill.sz}, side=${latestFill.side}`);
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

    // Verify fills occurred
    await sleep(500);
    const fills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ px: string; sz: string; side: string }>;
    if (fills.length < 2) {
      throw new Error(`Expected at least 2 fills (basic + price improvement), got ${fills.length}`);
    }
    // The latest fill should be from price improvement - price should be at maker price ($66,000)
    // or within the crossing range [$65,500, $66,000]
    const latestFill = fills[0];
    const fillPrice = parseFloat(latestFill.px);
    if (fillPrice < 65500 || fillPrice > 66000) {
      throw new Error(`Price improvement fill should be in range [65500, 66000], got ${fillPrice}`);
    }
    logProgress(`Price improvement: matched at $${fillPrice} (buy@66000 vs sell@65500)`);
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

    // Verify partial fill: Bob's larger order should have a resting remainder
    await sleep(500);
    const bobOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.BOB.address })) as Array<{ sz: string; side: string; limitPx: string }>;
    const bobFills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.BOB.address })) as Array<{ px: string; sz: string }>;
    if (bobFills.length === 0) {
      throw new Error('No fills found for Bob after partial fill test');
    }
    // Bob should have a resting buy order with remaining size (0.005 - 0.001 = 0.004)
    const restingBuy = bobOrders.find(o => o.side === 'B');
    if (!restingBuy) throw new Error('Bob should have a resting buy order after partial fill');
    const restingSize = parseFloat(restingBuy.sz);
    if (Math.abs(restingSize - 0.004) > 0.001) {
      throw new Error(`Bob resting size should be ~0.004 (0.005 - 0.001 filled), got ${restingSize}`);
    }
    logProgress(`Partial fill verified: Bob filled=${bobFills[0].sz}, resting=${restingBuy.sz}`);
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

    await sleep(300);
    const aliceOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    const bobOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.BOB.address })) as unknown[];
    if (aliceOrders.length !== 0) throw new Error(`Alice still has ${aliceOrders.length} orders after cancelAll`);
    if (bobOrders.length !== 0) throw new Error(`Bob still has ${bobOrders.length} orders after cancelAll`);
    logProgress('Cleanup verified: 0 orders for both accounts');
  });
}
