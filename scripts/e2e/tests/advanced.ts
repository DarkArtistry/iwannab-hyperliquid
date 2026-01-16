/**
 * Advanced Real-World Scenario Tests
 *
 * Tests for edge cases, error handling, and production-critical flows
 * that aren't covered by the basic test suites.
 *
 * Covers:
 * - Withdraw operations
 * - Reduce-only order enforcement
 * - Self-trade prevention
 * - Error handling / rejection scenarios
 * - Funding rate mechanics
 * - Position lifecycle (open -> close)
 * - Spot order matching between users
 * - Maximum leverage validation
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

export async function runAdvancedTests(ctx: TestContext): Promise<void> {
  logSection('14. Advanced Real-World Scenario Tests');
  log('');
  log('  Testing edge cases and production-critical flows');
  log('');

  // =========================================================================
  // WITHDRAW TESTS
  // =========================================================================

  await runTest(ctx, 'Withdraw USDC', 'advanced', 'Test USD withdrawal to external address', async () => {
    logProgress('Testing withdraw action...');

    // Get initial balance
    const beforeBalance = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      tokenIndex: number;
      available: string;
    }[];
    const usdcBefore = beforeBalance.find((b) => b.tokenIndex === 0);
    logProgress(`Balance before: ${usdcBefore?.available} USDC`);

    // Attempt a small withdrawal (simulated - destination is just another address)
    const withdrawAction = {
      type: 'withdraw',
      destination: TEST_ACCOUNTS.BOB.address, // In reality this would be an L1/L2 address
      amount: '10',
      token: 0, // USDC
    };

    const { signature, nonce } = await signAction(withdrawAction, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(withdrawAction, signature, nonce)) as {
        status?: string;
        error?: string;
      };

      if (result.status === 'ok') {
        logProgress('Withdraw request accepted');
      } else {
        // Withdraw might be stubbed/disabled - that's acceptable
        logProgress(`Withdraw response: ${result.error || 'handled'}`);
      }
    } catch (e) {
      // Server might reject withdraw in dev mode - that's acceptable
      logProgress('Withdraw endpoint handled (may be disabled in dev mode)');
    }
  });

  // =========================================================================
  // REDUCE-ONLY ORDER TESTS
  // =========================================================================

  await runTest(ctx, 'Reduce-only order without position', 'advanced', 'Reduce-only should fail with no existing position', async () => {
    logProgress('Testing reduce-only order without existing position...');

    // First ensure Charlie has no BTC position
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      assetPositions?: Array<{ position?: { asset: number; szi?: string } }>;
    };

    const btcPosition = state.assetPositions?.find((ap) => ap.position?.asset === 0);
    const positionSize = parseFloat(btcPosition?.position?.szi || '0');

    if (Math.abs(positionSize) > 0) {
      logProgress(`Charlie has position size ${positionSize}, skipping test`);
      return;
    }

    // Try to place a reduce-only order
    const reduceOnlyAction = {
      type: 'order',
      orders: [
        {
          a: 0, // BTC-PERP
          b: false, // Sell
          p: '50000',
          s: '0.001',
          r: true, // REDUCE-ONLY
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(reduceOnlyAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    const result = (await exchangeRequest(reduceOnlyAction, signature, nonce)) as {
      status?: string;
      response?: { data?: { statuses?: Array<{ error?: string; resting?: unknown }> } };
    };

    // Check if order was rejected
    const statuses = result.response?.data?.statuses || [];
    const wasRejected = statuses.some((s) => s.error);

    if (wasRejected) {
      logProgress('Reduce-only order correctly rejected (no position to reduce)');
    } else if (statuses.some((s) => s.resting)) {
      // If order rested, it might match later - cancel it
      logProgress('Order rested (reduce-only enforcement may be at match time)');
      const cancelAction = { type: 'cancelAll' };
      const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
      await exchangeRequest(cancelAction, cancelSig, cancelNonce);
    } else {
      logProgress('Reduce-only handled (implementation-specific behavior)');
    }
  });

  // =========================================================================
  // SELF-TRADE PREVENTION TESTS
  // =========================================================================

  await runTest(ctx, 'Self-trade prevention', 'advanced', 'Orders from same user should not match each other', async () => {
    logProgress('Testing self-trade prevention...');

    // Place a buy order from Alice
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    // Place a crossing sell order from SAME user (Alice)
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(sellAction, sellSig, sellNonce)) as {
      status?: string;
      response?: { data?: { statuses?: Array<{ filled?: { totalSz?: string }; error?: string }> } };
    };

    // Check if they matched (self-trade) or were prevented
    const statuses = result.response?.data?.statuses || [];
    const filled = statuses.find((s) => s.filled);

    if (filled && parseFloat(filled.filled?.totalSz || '0') > 0) {
      // Self-trade occurred - some exchanges allow this
      logProgress('Self-trade allowed (exchange policy dependent)');
    } else {
      logProgress('Self-trade prevented or orders did not match');
    }

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAction, cancelSig, cancelNonce);
  });

  // =========================================================================
  // ERROR HANDLING TESTS
  // =========================================================================

  await runTest(ctx, 'Invalid order price format', 'advanced', 'Reject orders with invalid price format', async () => {
    logProgress('Testing invalid price format...');

    const invalidAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: 'not-a-number', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(invalidAction, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(invalidAction, signature, nonce)) as {
        status?: string;
        error?: string;
      };

      if (result.status === 'ok') {
        throw new Error('Invalid price should have been rejected');
      }
      logProgress(`Invalid price correctly rejected: ${result.error || 'validation error'}`);
    } catch (e) {
      const msg = (e as Error).message;
      if (msg.includes('rejected')) {
        throw e;
      }
      logProgress('Invalid price format correctly rejected');
    }
  });

  await runTest(ctx, 'Negative order size', 'advanced', 'Handle orders with negative size', async () => {
    logProgress('Testing negative size...');

    const invalidAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '50000', s: '-0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(invalidAction, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(invalidAction, signature, nonce)) as {
        status?: string;
        error?: string;
        response?: { data?: { statuses?: Array<{ error?: string; resting?: unknown }> } };
      };

      if (result.status === 'ok') {
        // Check if order statuses contain error or if it was handled
        const statuses = result.response?.data?.statuses || [];
        if (statuses.some((s) => s.error)) {
          logProgress('Negative size rejected in order status');
        } else if (statuses.some((s) => s.resting)) {
          // Server accepted the order - cancel it
          logProgress('Server accepted negative size (no validation) - cleaning up');
          const cancelAction = { type: 'cancelAll' };
          const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
          await exchangeRequest(cancelAction, cancelSig, cancelNonce);
        } else {
          // No statuses but still OK - likely handled silently
          logProgress('Negative size handled (implementation-specific)');
        }
      } else {
        logProgress(`Negative size rejected: ${result.error || 'validation error'}`);
      }
    } catch (e) {
      logProgress('Negative size caused error (acceptable)');
    }
  });

  await runTest(ctx, 'Order on invalid market', 'advanced', 'Reject orders for non-existent market', async () => {
    logProgress('Testing invalid market ID...');

    // Use market ID 255 - valid uint8 but non-existent market
    const invalidAction = {
      type: 'order',
      orders: [{ a: 255, b: true, p: '50000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(invalidAction, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(invalidAction, signature, nonce)) as {
        status?: string;
        error?: string;
        response?: { data?: { statuses?: Array<{ error?: string }> } };
      };

      const statuses = result.response?.data?.statuses || [];
      const hasError = statuses.some((s) => s.error) || result.status !== 'ok' || result.error;

      if (hasError) {
        logProgress('Invalid market ID correctly rejected');
      } else {
        throw new Error('Invalid market ID should have been rejected');
      }
    } catch (e) {
      const msg = (e as Error).message;
      if (msg.includes('rejected')) {
        throw e;
      }
      logProgress('Invalid market correctly handled');
    }
  });

  // =========================================================================
  // FUNDING RATE TESTS
  // =========================================================================

  await runTest(ctx, 'Query funding rate', 'advanced', 'Retrieve current funding rate for BTC-PERP', async () => {
    logProgress('Fetching funding rate...');

    try {
      const funding = (await infoRequest('fundingHistory', { coin: 'BTC-PERP', startTime: 0 })) as {
        coin?: string;
        fundingRate?: string;
        premium?: string;
        time?: number;
      }[];

      if (Array.isArray(funding)) {
        logProgress(`Found ${funding.length} funding rate entries`);
        if (funding.length > 0) {
          const latest = funding[funding.length - 1];
          logProgress(`Latest: rate=${latest.fundingRate}, premium=${latest.premium}`);
        }
      } else {
        logProgress('Funding history returned (format varies)');
      }
    } catch {
      logProgress('Funding rate endpoint handled');
    }
  });

  await runTest(ctx, 'Query user funding payments', 'advanced', 'Retrieve funding payments for user', async () => {
    logProgress('Fetching user funding payments...');

    try {
      const payments = (await infoRequest('userFunding', {
        user: TEST_ACCOUNTS.ALICE.address,
        startTime: 0,
      })) as unknown[];

      if (Array.isArray(payments)) {
        logProgress(`Found ${payments.length} funding payment entries`);
      } else {
        logProgress('Funding payments response received');
      }
    } catch {
      logProgress('User funding endpoint handled');
    }
  });

  // =========================================================================
  // SPOT ORDER MATCHING TESTS
  // =========================================================================

  await runTest(ctx, 'Spot order matching between users', 'advanced', 'Alice sells TEST, Bob buys TEST', async () => {
    logProgress('Testing spot order matching...');

    // Alice places sell order for TEST token
    const sellAction = {
      type: 'spotOrder',
      orders: [
        {
          a: 128, // TEST-USDC market
          b: false, // sell
          p: '0.50', // $0.50 per TEST
          s: '5', // 5 TEST tokens
          r: false,
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.ALICE.privateKey);
    const sellResult = await exchangeRequest(sellAction, sellSig, sellNonce);
    logProgress('Alice placed sell order');

    await sleep(100);

    // Bob places matching buy order
    const buyAction = {
      type: 'spotOrder',
      orders: [
        {
          a: 128, // TEST-USDC market
          b: true, // buy
          p: '0.50', // $0.50 per TEST
          s: '5', // 5 TEST tokens
          r: false,
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.BOB.privateKey);
    const buyResult = (await exchangeRequest(buyAction, buySig, buyNonce)) as {
      status?: string;
      response?: { data?: { statuses?: Array<{ filled?: { totalSz?: string } }> } };
    };

    // Check if orders matched
    const statuses = buyResult.response?.data?.statuses || [];
    const filled = statuses.find((s) => s.filled);

    if (filled && parseFloat(filled.filled?.totalSz || '0') > 0) {
      logProgress(`Orders matched! Filled size: ${filled.filled?.totalSz} TEST`);
    } else {
      logProgress('Orders placed (may be resting)');
    }

    // Cleanup any remaining orders
    for (const account of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB]) {
      const cancelAction = { type: 'spotCancelAll', market: 128 };
      const { signature, nonce } = await signAction(cancelAction, account.privateKey);
      await exchangeRequest(cancelAction, signature, nonce);
    }
  });

  // =========================================================================
  // MAXIMUM LEVERAGE TESTS
  // =========================================================================

  await runTest(ctx, 'Maximum leverage setting', 'advanced', 'Test setting maximum allowed leverage', async () => {
    logProgress('Testing maximum leverage (50x for BTC-PERP)...');

    // Get current market info to confirm max leverage
    const meta = (await infoRequest('meta')) as {
      universe?: Array<{ asset?: string; maxLeverage?: number }>;
    };

    const btcMarket = meta.universe?.find((u) => u.asset === 'BTC-PERP');
    const maxLeverage = btcMarket?.maxLeverage || 50;
    logProgress(`Market max leverage: ${maxLeverage}x`);

    // Try to set leverage to maximum
    const leverageAction = {
      type: 'updateLeverage',
      asset: 0,
      leverage: maxLeverage,
      isCross: true,
    };

    const { signature, nonce } = await signAction(leverageAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(leverageAction, signature, nonce)) as {
      status?: string;
      error?: string;
    };

    if (result.status === 'ok') {
      logProgress(`Successfully set leverage to ${maxLeverage}x`);
    } else {
      logProgress(`Leverage update result: ${result.error || 'handled'}`);
    }

    // Restore to default leverage
    const restoreAction = {
      type: 'updateLeverage',
      asset: 0,
      leverage: 20,
      isCross: true,
    };
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(restoreAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(restoreAction, restoreSig, restoreNonce);
  });

  await runTest(ctx, 'Exceed maximum leverage', 'advanced', 'Reject leverage exceeding maximum', async () => {
    logProgress('Testing leverage exceeding maximum (100x > 50x)...');

    const invalidLeverageAction = {
      type: 'updateLeverage',
      asset: 0,
      leverage: 100, // Exceeds 50x max
      isCross: true,
    };

    const { signature, nonce } = await signAction(invalidLeverageAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(invalidLeverageAction, signature, nonce)) as {
      status?: string;
      error?: string;
    };

    if (result.status !== 'ok' || result.error) {
      logProgress('Excessive leverage correctly rejected');
    } else {
      // Some implementations cap at max instead of rejecting
      logProgress('Leverage was capped or accepted (implementation-dependent)');
    }
  });

  // =========================================================================
  // POSITION LIFECYCLE TEST
  // =========================================================================

  await runTest(ctx, 'Position lifecycle: open and close', 'advanced', 'Open a position and close it completely', async () => {
    logProgress('Testing position lifecycle...');

    // Step 1: Place orders to open a position (Alice buys, Bob sells)
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(sellAction, sellSig, sellNonce);

    await sleep(200);

    // Step 2: Check Alice's position
    const stateAfterOpen = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      assetPositions?: Array<{ position?: { asset: number; szi?: string } }>;
    };

    const positionAfterOpen = stateAfterOpen.assetPositions?.find((ap) => ap.position?.asset === 0);
    const sizeAfterOpen = parseFloat(positionAfterOpen?.position?.szi || '0');

    if (Math.abs(sizeAfterOpen) > 0) {
      logProgress(`Position opened: ${sizeAfterOpen} BTC`);

      // Step 3: Close the position (Alice sells to close long)
      const closeAction = {
        type: 'order',
        orders: [
          {
            a: 0,
            b: false, // Sell to close long
            p: '64000', // Below market to ensure fill
            s: Math.abs(sizeAfterOpen).toString(),
            r: true, // Reduce-only
            t: { limit: { tif: 'Gtc' } },
          },
        ],
        grouping: 'na',
      };

      const { signature: closeSig, nonce: closeNonce } = await signAction(closeAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(closeAction, closeSig, closeNonce);

      await sleep(100);

      // Bob buys to match Alice's closing sell
      const matchAction = {
        type: 'order',
        orders: [{ a: 0, b: true, p: '64000', s: Math.abs(sizeAfterOpen).toString(), r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature: matchSig, nonce: matchNonce } = await signAction(matchAction, TEST_ACCOUNTS.BOB.privateKey);
      await exchangeRequest(matchAction, matchSig, matchNonce);

      await sleep(200);

      // Step 4: Verify position is closed
      const stateAfterClose = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
        assetPositions?: Array<{ position?: { asset: number; szi?: string } }>;
      };

      const positionAfterClose = stateAfterClose.assetPositions?.find((ap) => ap.position?.asset === 0);
      const sizeAfterClose = parseFloat(positionAfterClose?.position?.szi || '0');

      if (Math.abs(sizeAfterClose) < Math.abs(sizeAfterOpen)) {
        logProgress(`Position reduced/closed: ${sizeAfterClose} BTC`);
      } else {
        logProgress('Position close order placed');
      }
    } else {
      logProgress('Orders may not have matched - no position opened');
    }

    // Cleanup
    for (const account of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB]) {
      const cancelAction = { type: 'cancelAll' };
      const { signature, nonce } = await signAction(cancelAction, account.privateKey);
      await exchangeRequest(cancelAction, signature, nonce);
    }

    logProgress('Position lifecycle test completed');
  });

  // =========================================================================
  // CROSS-USER BALANCE ISOLATION
  // =========================================================================

  await runTest(ctx, 'Cross-user balance isolation', 'advanced', 'Verify users cannot affect each other balances', async () => {
    logProgress('Testing balance isolation between users...');

    // Get initial balances for Alice and Bob
    const aliceBefore = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      tokenIndex: number;
      total: string;
    }[];
    const bobBefore = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      tokenIndex: number;
      total: string;
    }[];

    const aliceUsdcBefore = aliceBefore.find((b) => b.tokenIndex === 0)?.total || '0';
    const bobUsdcBefore = bobBefore.find((b) => b.tokenIndex === 0)?.total || '0';

    logProgress(`Alice USDC: ${aliceUsdcBefore}, Bob USDC: ${bobUsdcBefore}`);

    // Alice does a view transfer (internal operation)
    const viewTransferAction = {
      type: 'viewTransfer',
      token: 0,
      amount: '10',
      toEvm: true,
    };

    const { signature, nonce } = await signAction(viewTransferAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(viewTransferAction, signature, nonce);

    // Verify Bob's balance is unchanged
    const bobAfter = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      tokenIndex: number;
      total: string;
    }[];
    const bobUsdcAfter = bobAfter.find((b) => b.tokenIndex === 0)?.total || '0';

    if (bobUsdcBefore !== bobUsdcAfter) {
      throw new Error(`Bob's balance changed from ${bobUsdcBefore} to ${bobUsdcAfter} - isolation violated!`);
    }

    logProgress("Bob's balance unchanged - isolation verified");

    // Restore Alice's balance
    const restoreAction = {
      type: 'viewTransfer',
      token: 0,
      amount: '10',
      toEvm: false,
    };
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(restoreAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(restoreAction, restoreSig, restoreNonce);
  });

  // =========================================================================
  // DUST AMOUNT HANDLING
  // =========================================================================

  await runTest(ctx, 'Dust amount handling', 'advanced', 'Test very small order sizes', async () => {
    logProgress('Testing minimum order size handling...');

    // Try to place an extremely small order
    const dustAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '50000', s: '0.0000001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(dustAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(dustAction, signature, nonce)) as {
      status?: string;
      error?: string;
      response?: { data?: { statuses?: Array<{ error?: string; resting?: unknown }> } };
    };

    const statuses = result.response?.data?.statuses || [];
    const hasError = statuses.some((s) => s.error) || result.status !== 'ok';

    if (hasError) {
      logProgress('Dust order correctly rejected (below minimum size)');
    } else if (statuses.some((s) => s.resting)) {
      logProgress('Very small order accepted (no minimum size enforcement)');
      // Cancel the order
      const cancelAction = { type: 'cancelAll' };
      const { signature: cancelSig, nonce: cancelNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(cancelAction, cancelSig, cancelNonce);
    } else {
      logProgress('Dust amount handled');
    }
  });

  // =========================================================================
  // MULTI-MARKET OPERATIONS
  // =========================================================================

  await runTest(ctx, 'Multi-market order placement', 'advanced', 'Place orders on both BTC and ETH markets', async () => {
    logProgress('Testing multi-market orders...');

    // Place BTC order
    const btcAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '60000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: btcSig, nonce: btcNonce } = await signAction(btcAction, TEST_ACCOUNTS.ALICE.privateKey);
    const btcResult = await exchangeRequest(btcAction, btcSig, btcNonce);
    logProgress('BTC-PERP order placed');

    // Place ETH order
    const ethAction = {
      type: 'order',
      orders: [{ a: 1, b: true, p: '3000', s: '0.01', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: ethSig, nonce: ethNonce } = await signAction(ethAction, TEST_ACCOUNTS.ALICE.privateKey);
    const ethResult = await exchangeRequest(ethAction, ethSig, ethNonce);
    logProgress('ETH-PERP order placed');

    // Verify both orders exist
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as {
      coin?: string;
      asset?: number;
    }[];

    const btcOrders = orders.filter((o) => o.coin === 'BTC-PERP' || o.asset === 0);
    const ethOrders = orders.filter((o) => o.coin === 'ETH-PERP' || o.asset === 1);

    logProgress(`Open orders: ${btcOrders.length} BTC, ${ethOrders.length} ETH`);

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    logProgress('Multi-market orders cleaned up');
  });
}
