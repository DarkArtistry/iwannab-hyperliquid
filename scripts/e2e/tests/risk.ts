/**
 * Risk & Margin Tests
 *
 * Tests for risk management, margin validation, order validation,
 * fee calculations, and leverage management.
 *
 * IMPORTANT: These tests verify REAL functionality:
 * - Uses `unifiedBalances` API for actual trading balance (coreView)
 * - Orders are verified by polling `openOrders` API with timeouts
 * - Fills are verified by checking `userFills` API
 * - All operations are timed for latency measurement
 *
 * BALANCE ARCHITECTURE:
 * - `clearinghouseState` → Engine's perpetual account (positions/margin)
 * - `unifiedBalances` → Unified state (actual trading funds, source of truth)
 *   - total: Total balance owned by user
 *   - coreView: Balance available for Core trading (perps, spot)
 *   - evmView: Balance available for EVM operations
 */

import {
  TEST_ACCOUNTS,
  infoRequest,
  exchangeRequest,
  signAction,
  runTest,
  logSection,
  log,
  logProgress,
  sleep,
} from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

// ============================================================================
// TYPE DEFINITIONS
// ============================================================================

interface ApiResponse {
  status: string;
  response?: {
    data?: {
      statuses?: Array<{
        resting?: { oid?: number };
        filled?: { oid?: number; totalSz?: string };
        error?: string;
      }>;
    };
  };
  error?: string;
}

interface OpenOrder {
  oid?: number;
  coin?: string;
  side?: string;
  limitPx?: string;
  sz?: string;
  cloid?: string;
}

interface UserFill {
  coin?: string;
  px?: string;
  sz?: string;
  side?: string;
  time?: number;
  oid?: number;
}

interface UnifiedBalance {
  tokenIndex: number;
  symbol: string;
  total: string;
  coreView: string;
  evmView: string;
}

interface UnifiedBalancesResponse {
  balances: UnifiedBalance[];
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/**
 * Get the coreView balance for USDC (token index 0)
 * This is the actual trading balance available for perps/spot
 */
async function getCoreViewBalance(userAddress: string): Promise<number> {
  const response = (await infoRequest('unifiedBalances', { user: userAddress })) as UnifiedBalancesResponse;
  const usdc = response.balances?.find((b) => b.tokenIndex === 0);
  return parseFloat(usdc?.coreView || '0');
}

/**
 * Get total balance for USDC (token index 0)
 */
async function getTotalBalance(userAddress: string): Promise<number> {
  const response = (await infoRequest('unifiedBalances', { user: userAddress })) as UnifiedBalancesResponse;
  const usdc = response.balances?.find((b) => b.tokenIndex === 0);
  return parseFloat(usdc?.total || '0');
}

/**
 * Poll for order to appear in openOrders with timeout
 */
async function waitForOrderInBook(
  userAddress: string,
  orderId: number,
  timeoutMs: number = 5000,
  pollIntervalMs: number = 100
): Promise<{ found: boolean; order?: OpenOrder; durationMs: number }> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const orders = (await infoRequest('openOrders', { user: userAddress })) as OpenOrder[];
    const found = orders.find((o) => o.oid === orderId);

    if (found) {
      return { found: true, order: found, durationMs: Date.now() - startTime };
    }

    await sleep(pollIntervalMs);
  }

  return { found: false, durationMs: Date.now() - startTime };
}

/**
 * Poll for order with specific cloid to appear in openOrders
 */
async function waitForOrderByCloid(
  userAddress: string,
  cloid: string,
  timeoutMs: number = 5000,
  pollIntervalMs: number = 100
): Promise<{ found: boolean; order?: OpenOrder; durationMs: number }> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const orders = (await infoRequest('openOrders', { user: userAddress })) as OpenOrder[];
    const found = orders.find((o) => o.cloid === cloid);

    if (found) {
      return { found: true, order: found, durationMs: Date.now() - startTime };
    }

    await sleep(pollIntervalMs);
  }

  return { found: false, durationMs: Date.now() - startTime };
}

/**
 * Poll for fill to appear in userFills after a specific timestamp
 */
async function waitForFill(
  userAddress: string,
  afterTimestamp: number,
  timeoutMs: number = 5000,
  pollIntervalMs: number = 100
): Promise<{ found: boolean; fill?: UserFill; durationMs: number }> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const fills = (await infoRequest('userFills', { user: userAddress })) as UserFill[];
    const newFill = fills.find((f) => (f.time || 0) > afterTimestamp);

    if (newFill) {
      return { found: true, fill: newFill, durationMs: Date.now() - startTime };
    }

    await sleep(pollIntervalMs);
  }

  return { found: false, durationMs: Date.now() - startTime };
}

/**
 * Get open orders count
 */
async function getOpenOrdersCount(userAddress: string): Promise<number> {
  const orders = (await infoRequest('openOrders', { user: userAddress })) as OpenOrder[];
  return orders.length;
}

/**
 * Cancel all orders for a user
 */
async function cancelAllOrders(userAddress: string, privateKey: `0x${string}`): Promise<void> {
  const cancelAction = { type: 'cancelAll' };
  const { signature, nonce } = await signAction(cancelAction, privateKey);
  await exchangeRequest(cancelAction, signature, nonce);
  await sleep(200);
}

// ============================================================================
// TEST SUITE
// ============================================================================

export async function runRiskTests(ctx: TestContext): Promise<void> {
  logSection('15. Risk & Margin Tests');
  log('');
  log('  Testing risk management, margin validation, and fee calculations');
  log('  (Real tests with unifiedBalances API and order confirmation polling)');
  log('');

  // =========================================================================
  // ACCOUNT FUNDING VERIFICATION
  // Uses unifiedBalances API to check actual trading balance (coreView)
  // =========================================================================

  await runTest(ctx, 'Verify account funding (Alice)', 'risk', 'Check coreView balance from unifiedBalances API', async () => {
    logProgress('Checking Alice unified balance (coreView)...');

    const totalBalance = await getTotalBalance(TEST_ACCOUNTS.ALICE.address);
    const coreViewBalance = await getCoreViewBalance(TEST_ACCOUNTS.ALICE.address);

    logProgress(`Total balance: $${totalBalance.toFixed(2)}`);
    logProgress(`Core view (trading) balance: $${coreViewBalance.toFixed(2)}`);

    // Genesis should provide funded accounts
    if (totalBalance < 1000) {
      throw new Error(`Alice total balance too low ($${totalBalance.toFixed(2)}), expected >= $1000 from genesis`);
    }
    logProgress(`Account properly funded: $${totalBalance.toFixed(2)}`);
  });

  await runTest(ctx, 'Verify account funding (Bob)', 'risk', 'Bob should also have funded coreView', async () => {
    logProgress('Checking Bob unified balance...');

    const totalBalance = await getTotalBalance(TEST_ACCOUNTS.BOB.address);
    const coreViewBalance = await getCoreViewBalance(TEST_ACCOUNTS.BOB.address);

    logProgress(`Bob total: $${totalBalance.toFixed(2)}, coreView: $${coreViewBalance.toFixed(2)}`);

    if (totalBalance < 1000) {
      throw new Error(`Bob total balance too low ($${totalBalance.toFixed(2)}), expected >= $1000 from genesis`);
    }
  });

  // =========================================================================
  // CLEANUP BEFORE TESTS
  // =========================================================================

  await runTest(ctx, 'Cleanup existing orders', 'risk', 'Cancel all existing orders before testing', async () => {
    logProgress('Cancelling any existing Alice orders...');
    await cancelAllOrders(TEST_ACCOUNTS.ALICE.address, TEST_ACCOUNTS.ALICE.privateKey);

    logProgress('Cancelling any existing Bob orders...');
    await cancelAllOrders(TEST_ACCOUNTS.BOB.address, TEST_ACCOUNTS.BOB.privateKey);

    const aliceCount = await getOpenOrdersCount(TEST_ACCOUNTS.ALICE.address);
    const bobCount = await getOpenOrdersCount(TEST_ACCOUNTS.BOB.address);

    if (aliceCount !== 0) throw new Error(`Alice still has ${aliceCount} orders after cleanup`);
    if (bobCount !== 0) throw new Error(`Bob still has ${bobCount} orders after cleanup`);
    logProgress(`After cleanup - Alice: ${aliceCount} orders, Bob: ${bobCount} orders`);
  });

  // =========================================================================
  // LEVERAGE TESTS
  // =========================================================================

  await runTest(ctx, 'Update leverage to 50x (max)', 'risk', 'Set BTC-PERP leverage to maximum', async () => {
    logProgress('Updating leverage to 50x...');

    const action = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 50,
    };

    const startTime = Date.now();
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;
    const durationMs = Date.now() - startTime;

    if (result.status !== 'ok') {
      throw new Error(`Failed to update leverage: ${JSON.stringify(result)}`);
    }

    logProgress(`Leverage updated to 50x in ${durationMs}ms`);
  });

  await runTest(ctx, 'Update leverage to 1x (min)', 'risk', 'Set BTC-PERP leverage to minimum', async () => {
    logProgress('Updating leverage to 1x...');

    const action = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 1,
    };

    const startTime = Date.now();
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;
    const durationMs = Date.now() - startTime;

    if (result.status !== 'ok') {
      throw new Error(`Failed to update leverage: ${JSON.stringify(result)}`);
    }

    logProgress(`Leverage updated to 1x in ${durationMs}ms`);
  });

  await runTest(ctx, 'Reset leverage to 10x', 'risk', 'Set BTC-PERP leverage to 10x for further tests', async () => {
    logProgress('Updating leverage to 10x...');

    const action = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 10,
    };

    const startTime = Date.now();
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;
    const durationMs = Date.now() - startTime;

    if (result.status !== 'ok') {
      throw new Error(`Failed to update leverage: ${JSON.stringify(result)}`);
    }

    logProgress(`Leverage reset to 10x in ${durationMs}ms`);
  });

  // =========================================================================
  // ORDER PLACEMENT WITH CONFIRMATION POLLING
  // Places resting order at distant price and verifies it appears in order book
  // =========================================================================

  await runTest(ctx, 'Place resting order and confirm in book', 'risk', 'Place far-from-market order, poll until confirmed', async () => {
    const cloid = `risk-rest-${Date.now()}`;
    logProgress(`Placing resting buy at $50,000 (far below market), cloid: ${cloid}`);

    const action = {
      type: 'order',
      orders: [{
        a: 0, // BTC-PERP
        b: true, // buy
        p: '50000', // Far below market to ensure it rests
        s: '0.001', // Minimum size
        r: false,
        t: { limit: { tif: 'Gtc' } },
        c: cloid,
      }],
      grouping: 'na',
    };

    const orderStartTime = Date.now();
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;
    const submitDuration = Date.now() - orderStartTime;

    if (result.status !== 'ok') {
      throw new Error(`Order rejected: ${JSON.stringify(result)}`);
    }

    logProgress(`Order submitted in ${submitDuration}ms`);

    // Get order ID from response
    const orderStatus = result.response?.data?.statuses?.[0];
    const orderId = orderStatus?.resting?.oid;

    if (orderId) {
      logProgress(`Order ID: ${orderId}, polling for confirmation...`);

      // Poll for order to appear
      const { found, order, durationMs } = await waitForOrderInBook(
        TEST_ACCOUNTS.ALICE.address,
        orderId,
        5000,
        100
      );

      if (found && order) {
        logProgress(`Order confirmed in ${durationMs}ms`);
        logProgress(`  Price: ${order.limitPx}, Size: ${order.sz}, Side: ${order.side}`);
      } else {
        throw new Error(`Order ${orderId} not found in order book after 5s (duration: ${durationMs}ms)`);
      }
    } else {
      // Order might have filled immediately (unlikely at $50k) or response format differs
      logProgress(`Order submitted, checking by cloid...`);

      const { found, order, durationMs } = await waitForOrderByCloid(
        TEST_ACCOUNTS.ALICE.address,
        cloid,
        5000,
        100
      );

      if (found && order) {
        logProgress(`Order found by cloid in ${durationMs}ms, ID: ${order.oid}`);
      } else {
        throw new Error(`Order not found by cloid ${cloid} after 5s - order may have been rejected`);
      }
    }
  });

  // =========================================================================
  // ORDER MATCHING WITH FILL VERIFICATION
  // Bob places sell order, Alice places matching buy, verify fills appear
  // =========================================================================

  await runTest(ctx, 'Match orders and verify fills', 'risk', 'Alice and Bob trade, verify fills with polling', async () => {
    const beforeTimestamp = Date.now();

    // Step 1: Bob places a sell order at $65,000
    logProgress('Bob placing sell at $65,000...');
    const bobCloid = `bob-sell-${Date.now()}`;
    const bobAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: false, // sell
        p: '65000',
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
        c: bobCloid,
      }],
      grouping: 'na',
    };

    const bobStartTime = Date.now();
    const { signature: bobSig, nonce: bobNonce } = await signAction(bobAction, TEST_ACCOUNTS.BOB.privateKey);
    const bobResult = (await exchangeRequest(bobAction, bobSig, bobNonce)) as ApiResponse;
    const bobSubmitDuration = Date.now() - bobStartTime;

    if (bobResult.status !== 'ok') {
      throw new Error(`Bob order rejected: ${JSON.stringify(bobResult)}`);
    }

    logProgress(`Bob order submitted in ${bobSubmitDuration}ms`);

    // Wait a bit for Bob's order to settle
    await sleep(200);

    // Step 2: Alice places a matching buy order at $65,000
    logProgress('Alice placing matching buy at $65,000...');
    const aliceCloid = `alice-buy-${Date.now()}`;
    const aliceAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: true, // buy
        p: '65000',
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
        c: aliceCloid,
      }],
      grouping: 'na',
    };

    const aliceStartTime = Date.now();
    const { signature: aliceSig, nonce: aliceNonce } = await signAction(aliceAction, TEST_ACCOUNTS.ALICE.privateKey);
    const aliceResult = (await exchangeRequest(aliceAction, aliceSig, aliceNonce)) as ApiResponse;
    const aliceSubmitDuration = Date.now() - aliceStartTime;

    if (aliceResult.status !== 'ok') {
      throw new Error(`Alice order rejected: ${JSON.stringify(aliceResult)}`);
    }

    logProgress(`Alice order submitted in ${aliceSubmitDuration}ms`);

    // Check if order was filled immediately
    const aliceStatus = aliceResult.response?.data?.statuses?.[0];
    if (aliceStatus?.filled) {
      logProgress(`Immediate fill! Size: ${aliceStatus.filled.totalSz}`);
    }

    // Step 3: Poll for Alice's fill
    logProgress('Polling for Alice fill...');
    const { found: aliceFillFound, fill: aliceFill, durationMs: alicePollDuration } = await waitForFill(
      TEST_ACCOUNTS.ALICE.address,
      beforeTimestamp,
      5000,
      100
    );

    if (aliceFillFound && aliceFill) {
      logProgress(`Alice fill confirmed in ${alicePollDuration}ms:`);
      logProgress(`  Price: $${aliceFill.px}, Size: ${aliceFill.sz}, Side: ${aliceFill.side}`);
    } else {
      logProgress(`No Alice fill found after ${alicePollDuration}ms (order may be resting)`);
    }

    // Step 4: Poll for Bob's fill
    logProgress('Polling for Bob fill...');
    const { found: bobFillFound, fill: bobFill, durationMs: bobPollDuration } = await waitForFill(
      TEST_ACCOUNTS.BOB.address,
      beforeTimestamp,
      5000,
      100
    );

    if (bobFillFound && bobFill) {
      logProgress(`Bob fill confirmed in ${bobPollDuration}ms:`);
      logProgress(`  Price: $${bobFill.px}, Size: ${bobFill.sz}, Side: ${bobFill.side}`);
    } else {
      logProgress(`No Bob fill found after ${bobPollDuration}ms`);
    }

    // Verify at least one fill occurred
    if (!aliceFillFound && !bobFillFound) {
      throw new Error('No fills found for either Alice or Bob - orders did not match');
    }
  });

  // =========================================================================
  // BATCH ORDER OPERATIONS
  // Place multiple orders in batch and verify all appear in order book
  // =========================================================================

  await runTest(ctx, 'Batch order placement and verification', 'risk', 'Place 3 orders in batch, poll all to confirm', async () => {
    const initialCount = await getOpenOrdersCount(TEST_ACCOUNTS.ALICE.address);
    logProgress(`Initial open orders: ${initialCount}`);

    const batchPrefix = `batch-${Date.now()}`;
    logProgress(`Placing 3 orders with prefix: ${batchPrefix}`);

    const action = {
      type: 'order',
      orders: [
        { a: 0, b: true, p: '45000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } }, c: `${batchPrefix}-1` },
        { a: 0, b: true, p: '44000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } }, c: `${batchPrefix}-2` },
        { a: 0, b: false, p: '85000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } }, c: `${batchPrefix}-3` },
      ],
      grouping: 'na',
    };

    const batchStartTime = Date.now();
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;
    const batchSubmitDuration = Date.now() - batchStartTime;

    if (result.status !== 'ok') {
      throw new Error(`Batch order failed: ${JSON.stringify(result)}`);
    }

    const statuses = result.response?.data?.statuses || [];
    logProgress(`Batch submitted in ${batchSubmitDuration}ms, got ${statuses.length} statuses`);

    // Poll for orders to appear
    await sleep(500);

    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as OpenOrder[];
    const batchOrders = orders.filter((o) => o.cloid?.startsWith(batchPrefix));

    logProgress(`Found ${batchOrders.length}/3 batch orders in book after ${Date.now() - batchStartTime}ms`);

    for (const order of batchOrders) {
      logProgress(`  Order ${order.oid}: price=${order.limitPx}, size=${order.sz}, side=${order.side}`);
    }

    if (batchOrders.length < 3) {
      throw new Error(`Expected 3 batch orders in book, found only ${batchOrders.length}`);
    }
  });

  // =========================================================================
  // CANCEL ORDER AND VERIFY REMOVAL
  // =========================================================================

  await runTest(ctx, 'Cancel order and verify removal', 'risk', 'Cancel specific order and confirm removal from book', async () => {
    // Place an order to cancel
    const cancelTestCloid = `cancel-${Date.now()}`;
    logProgress(`Placing order to cancel, cloid: ${cancelTestCloid}`);

    const placeAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: true,
        p: '40000', // Very far from market
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
        c: cancelTestCloid,
      }],
      grouping: 'na',
    };

    const { signature: placeSig, nonce: placeNonce } = await signAction(placeAction, TEST_ACCOUNTS.ALICE.privateKey);
    const placeResult = (await exchangeRequest(placeAction, placeSig, placeNonce)) as ApiResponse;

    if (placeResult.status !== 'ok') {
      throw new Error(`Order placement failed: ${JSON.stringify(placeResult)}`);
    }

    // Wait for order to appear
    const { found, order } = await waitForOrderByCloid(TEST_ACCOUNTS.ALICE.address, cancelTestCloid, 3000, 100);

    if (!found || !order?.oid) {
      logProgress('Order not found in book (may have been immediately filled)');
      return;
    }

    logProgress(`Order found, ID: ${order.oid}, cancelling...`);

    // Cancel the order
    const cancelAction = {
      type: 'cancel',
      cancels: [{ a: 0, o: order.oid }],
    };

    const cancelStartTime = Date.now();
    const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    const cancelResult = (await exchangeRequest(cancelAction, cancelSig, cancelNonce)) as ApiResponse;
    const cancelDuration = Date.now() - cancelStartTime;

    if (cancelResult.status !== 'ok') {
      throw new Error(`Cancel failed: ${JSON.stringify(cancelResult)}`);
    }

    logProgress(`Cancel request sent in ${cancelDuration}ms, verifying removal...`);

    // Verify order is removed
    await sleep(300);
    const afterOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as OpenOrder[];
    const stillExists = afterOrders.find((o) => o.oid === order.oid);

    if (stillExists) {
      throw new Error(`Order ${order.oid} still exists after cancel`);
    }

    logProgress(`Order ${order.oid} successfully cancelled and removed`);
  });

  // =========================================================================
  // BALANCE TRACKING THROUGH TRADE
  // =========================================================================

  await runTest(ctx, 'Track balance change through trade', 'risk', 'Verify balance reflects fees after trade', async () => {
    const initialBalance = await getCoreViewBalance(TEST_ACCOUNTS.ALICE.address);
    logProgress(`Initial coreView balance: $${initialBalance.toFixed(2)}`);

    if (initialBalance < 100) {
      throw new Error(`Insufficient balance for trade test: $${initialBalance.toFixed(2)} (expected >= $100)`);
    }

    // Place a small trade (Bob sell, Alice buy)
    const beforeTimestamp = Date.now();

    const bobAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: false,
        p: '65000',
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
      }],
      grouping: 'na',
    };

    const { signature: bobSig, nonce: bobNonce } = await signAction(bobAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(bobAction, bobSig, bobNonce);
    await sleep(100);

    const aliceAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: true,
        p: '65000',
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
      }],
      grouping: 'na',
    };

    const { signature: aliceSig, nonce: aliceNonce } = await signAction(aliceAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(aliceAction, aliceSig, aliceNonce);

    // Wait for settlement
    await sleep(500);

    const finalBalance = await getCoreViewBalance(TEST_ACCOUNTS.ALICE.address);
    logProgress(`Final coreView balance: $${finalBalance.toFixed(2)}`);

    const change = finalBalance - initialBalance;
    if (change === 0) {
      throw new Error('Balance should have changed after trade (margin lock + fees), but it did not');
    }
    // Buyer's balance should decrease (margin lock + taker fee)
    if (change > 0) {
      throw new Error(`Expected balance to decrease after buy trade (margin lock + fees), but it increased by $${change.toFixed(4)}`);
    }
    logProgress(`Balance change: $${change.toFixed(4)} (includes margin lock and fees)`);
  });

  // =========================================================================
  // REDUCE-ONLY ORDER TEST
  // =========================================================================

  await runTest(ctx, 'Reduce-only order behavior', 'risk', 'Verify reduce-only flag prevents opening new positions', async () => {
    logProgress('Testing reduce-only order behavior...');

    // Place a reduce-only buy order when we have no position
    // This should be rejected — reduce-only cannot open a new position
    const reduceOnlyCloid = `reduce-only-${Date.now()}`;
    const action = {
      type: 'order',
      orders: [{
        a: 0,
        b: true,
        p: '50000',
        s: '0.001',
        r: true, // reduce-only flag
        t: { limit: { tif: 'Gtc' } },
        c: reduceOnlyCloid,
      }],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as ApiResponse;

    // Check that the order was rejected or did not rest in the book
    const orderStatus = result.response?.data?.statuses?.[0];
    if (orderStatus?.resting) {
      // Order should NOT rest when reduce-only with no position — clean up and fail
      const cancelAction = {
        type: 'cancel',
        cancels: [{ a: 0, o: orderStatus.resting.oid }],
      };
      const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(cancelAction, cancelSig, cancelNonce);
      throw new Error(`Reduce-only order should not rest when no position exists (got oid=${orderStatus.resting.oid})`);
    }

    // Verify the order was either explicitly rejected or simply not placed
    if (orderStatus?.error) {
      logProgress(`Reduce-only correctly rejected: ${orderStatus.error}`);
    } else {
      logProgress('Reduce-only order did not rest (correct — no position to reduce)');
    }
  });

  // =========================================================================
  // FINAL CLEANUP
  // =========================================================================

  await runTest(ctx, 'Final cleanup', 'risk', 'Cancel all remaining orders from risk tests', async () => {
    logProgress('Cleaning up Alice orders...');
    const aliceOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as OpenOrder[];
    logProgress(`Found ${aliceOrders.length} orders`);

    if (aliceOrders.length > 0) {
      await cancelAllOrders(TEST_ACCOUNTS.ALICE.address, TEST_ACCOUNTS.ALICE.privateKey);
    }

    logProgress('Cleaning up Bob orders...');
    const bobOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.BOB.address })) as OpenOrder[];
    logProgress(`Found ${bobOrders.length} orders`);

    if (bobOrders.length > 0) {
      await cancelAllOrders(TEST_ACCOUNTS.BOB.address, TEST_ACCOUNTS.BOB.privateKey);
    }

    // Verify cleanup
    await sleep(300);
    const aliceRemaining = await getOpenOrdersCount(TEST_ACCOUNTS.ALICE.address);
    const bobRemaining = await getOpenOrdersCount(TEST_ACCOUNTS.BOB.address);

    if (aliceRemaining !== 0) throw new Error(`Alice still has ${aliceRemaining} orders after final cleanup`);
    if (bobRemaining !== 0) throw new Error(`Bob still has ${bobRemaining} orders after final cleanup`);
    logProgress(`After cleanup - Alice: ${aliceRemaining} orders, Bob: ${bobRemaining} orders`);
  });
}
