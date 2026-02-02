/**
 * Order Lifecycle Tests
 *
 * Tests for order placement, modification, and cancellation.
 *
 * ORDER WIRE FORMAT (Perpetuals)
 * ==============================
 * Orders are sent to POST /exchange with action.type = 'order'
 *
 * Each order object has these fields (matching Hyperliquid's API):
 *   a: number   - Asset/Market ID (0 = BTC-PERP, 1 = ETH-PERP, etc.)
 *   b: boolean  - Side (true = Buy, false = Sell)
 *   p: string   - Limit price as decimal string (e.g., "65000.50")
 *   s: string   - Size in base asset (e.g., "0.001" BTC)
 *   r: boolean  - Reduce-only flag (true = can only reduce position)
 *   t: object   - Order type configuration
 *   c: string?  - Client order ID (optional, for tracking)
 *
 * Order Types (t field):
 *   { limit: { tif: 'Gtc' } }  - Good-till-cancelled (rests on book)
 *   { limit: { tif: 'Ioc' } }  - Immediate-or-cancel (fill or kill remaining)
 *   { limit: { tif: 'Alo' } }  - Add-liquidity-only (post-only, reject if would cross)
 *
 * MARKET IDS:
 *   Perpetuals: 0-127 (0 = BTC-PERP, 1 = ETH-PERP, ...)
 *   Spot:       128+  (128 = first spot market, typically TEST-USDC)
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

export async function runOrderTests(ctx: TestContext): Promise<void> {
  logSection('4. Order Lifecycle Tests');
  log('');
  log('  Testing order placement, modification, and cancellation');
  log('');

  let placedOrderId: string | null = null;

  await runTest(ctx, 'Place limit buy order', 'orders', 'Place a limit buy order below market price', async () => {
    logProgress('Preparing limit buy order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,          // Market ID 0 = BTC-PERP
          b: true,       // Buy side
          p: '60000',    // Limit price: $60,000 per BTC
          s: '0.001',    // Size: 0.001 BTC (~$60 notional)
          r: false,      // Not reduce-only
          t: { limit: { tif: 'Gtc' } }, // Good-till-cancelled
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    logProgress('Order signed, sending to exchange...');

    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      response?: { data?: { statuses?: Array<{ resting?: { oid?: string } }> } };
    };

    if (result.status !== 'ok') {
      throw new Error(`Order placement failed: ${JSON.stringify(result)}`);
    }
    if (result.response?.data?.statuses?.[0]?.resting?.oid) {
      placedOrderId = result.response.data.statuses[0].resting.oid;
      logProgress(`Order placed successfully, ID: ${placedOrderId}`);
    } else {
      logProgress('Order accepted (status: ok)');
    }
  });

  await runTest(ctx, 'Place limit sell order', 'orders', 'Place a limit sell order above market price', async () => {
    logProgress('Preparing limit sell order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: false, // isSell
          p: '70000',
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(action, signature, nonce) as { status?: string };
    if (result.status !== 'ok') {
      throw new Error(`Sell order failed: ${JSON.stringify(result)}`);
    }
    logProgress('Sell order placed');
  });

  await runTest(ctx, 'Place post-only order', 'orders', 'Place maker-only order that rejects if would cross', async () => {
    logProgress('Preparing post-only order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: true,
          p: '55000', // Well below market
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Alo' } }, // Add-liquidity-only
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(action, signature, nonce) as { status?: string };
    if (result.status !== 'ok') {
      throw new Error(`Post-only order failed: ${JSON.stringify(result)}`);
    }
    logProgress('Post-only order placed');
  });

  await runTest(ctx, 'Place IOC order', 'orders', 'Place immediate-or-cancel order', async () => {
    logProgress('Preparing IOC order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: true,
          p: '50000', // Below market - won't fill
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Ioc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(action, signature, nonce) as { status?: string };
    if (result.status !== 'ok') {
      throw new Error(`IOC order failed: ${JSON.stringify(result)}`);
    }
    logProgress('IOC order submitted and handled');
  });

  await runTest(ctx, 'Batch place orders', 'orders', 'Place multiple orders in a single request', async () => {
    logProgress('Preparing batch order...');

    const action = {
      type: 'order',
      orders: [
        { a: 0, b: true, p: '58000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
        { a: 0, b: true, p: '57000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
        { a: 0, b: false, p: '72000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(action, signature, nonce) as { status?: string };
    if (result.status !== 'ok') {
      throw new Error(`Batch order failed: ${JSON.stringify(result)}`);
    }
    logProgress('Batch orders placed');
  });

  await runTest(ctx, 'Cancel single order', 'orders', 'Cancel a specific order by ID', async () => {
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ oid?: string }>;

    if (orders.length === 0) {
      logProgress('No orders to cancel, placing one first...');
      const placeAction = {
        type: 'order',
        orders: [{ a: 0, b: true, p: '55000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature: placeSig, nonce: placeNonce } = await signAction(placeAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(placeAction, placeSig, placeNonce);
      await sleep(500);
    }

    const refreshedOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ oid?: string }>;
    if (refreshedOrders.length > 0) {
      const orderToCancel = refreshedOrders[0];
      logProgress(`Cancelling order ${orderToCancel.oid}...`);

      const cancelAction = {
        type: 'cancel',
        cancels: [{ a: 0, o: orderToCancel.oid }],
      };

      const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(cancelAction, signature, nonce);
      logProgress('Order cancelled');
    } else {
      logProgress('No orders available to cancel');
    }
  });

  await runTest(ctx, 'Cancel all orders', 'orders', 'Cancel all open orders for an account', async () => {
    logProgress('Cancelling all orders...');

    const cancelAction = {
      type: 'cancelAll',
    };

    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    logProgress('All orders cancelled');

    await sleep(500);
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    if (orders.length !== 0) {
      throw new Error(`Expected 0 open orders after cancelAll, got ${orders.length}`);
    }
    logProgress('All orders cancelled, 0 remaining');
  });

  await runTest(ctx, 'Cancel order by client order ID', 'orders', 'Place an order with CLOID and cancel it by CLOID', async () => {
    logProgress('Placing order with client order ID...');
    const cloid = `test-cloid-${Date.now()}`;

    const placeAction = {
      type: 'order',
      orders: [{
        a: 0,
        b: true,
        p: '50000',
        s: '0.001',
        r: false,
        t: { limit: { tif: 'Gtc' } },
        c: cloid
      }],
      grouping: 'na',
    };

    const { signature: placeSig, nonce: placeNonce } = await signAction(placeAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(placeAction, placeSig, placeNonce);
    await sleep(500);

    logProgress(`Order placed with cloid: ${cloid}`);

    const cancelAction = {
      type: 'cancelByCloid',
      cancels: [{ asset: 0, cloid: cloid }],
    };

    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(cancelAction, signature, nonce) as { status?: string };

    if (result.status !== 'ok') {
      throw new Error(`Cancel by CLOID failed: ${JSON.stringify(result)}`);
    }
    logProgress('Order cancelled by CLOID successfully');
  });

  await runTest(ctx, 'USD transfer between accounts', 'orders', 'Transfer USDC between accounts on Core layer', async () => {
    logProgress('Initiating USD transfer from Alice to Bob...');

    // First check Alice's core_view balance (USD transfers use unified state core_view)
    const balanceResponse = await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address }) as {
      balances: { tokenIndex: number; symbol: string; total: string; coreView: string; evmView: string }[];
    };
    const usdcBalance = balanceResponse.balances?.find((b) => b.tokenIndex === 0);
    const coreViewBalance = parseFloat(usdcBalance?.coreView || '0');
    logProgress(`Alice core_view balance: ${coreViewBalance} USDC`);

    // Only try transfer if there's enough balance
    const transferAmount = Math.min(10, Math.floor(coreViewBalance / 2));
    if (transferAmount < 1) {
      throw new Error(`Insufficient core_view balance for transfer test: ${coreViewBalance} USDC`);
    }

    const transferAction = {
      type: 'usdTransfer',
      destination: TEST_ACCOUNTS.BOB.address,
      amount: transferAmount.toString(),
    };

    const { signature, nonce } = await signAction(transferAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(transferAction, signature, nonce) as { status?: string; error?: string };

    if (result.status !== 'ok') {
      throw new Error(`USD transfer failed: ${JSON.stringify(result)}`);
    }
    logProgress(`USD transfer of ${transferAmount} USDC completed successfully`);

    // Transfer back to restore balance
    logProgress('Transferring back to Alice...');
    const { signature: returnSig, nonce: returnNonce } = await signAction(
      { type: 'usdTransfer', destination: TEST_ACCOUNTS.ALICE.address, amount: transferAmount.toString() },
      TEST_ACCOUNTS.BOB.privateKey
    );
    const returnResult = await exchangeRequest(
      { type: 'usdTransfer', destination: TEST_ACCOUNTS.ALICE.address, amount: transferAmount.toString() },
      returnSig,
      returnNonce
    ) as { status?: string };

    if (returnResult.status === 'ok') {
      logProgress('Balance restored');
    }
  });

  await runTest(ctx, 'Update leverage', 'orders', 'Update leverage for a market', async () => {
    logProgress('Updating leverage for BTC-PERP...');

    const leverageAction = {
      type: 'updateLeverage',
      asset: 0,
      leverage: 10,
      isCross: true,
    };

    const { signature, nonce } = await signAction(leverageAction, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(leverageAction, signature, nonce) as { status?: string };

    if (result.status !== 'ok') {
      throw new Error(`Update leverage failed: ${JSON.stringify(result)}`);
    }
    logProgress('Leverage updated to 10x successfully');

    const { signature: restoreSig, nonce: restoreNonce } = await signAction(
      { type: 'updateLeverage', asset: 0, leverage: 20, isCross: true },
      TEST_ACCOUNTS.ALICE.privateKey
    );
    await exchangeRequest(
      { type: 'updateLeverage', asset: 0, leverage: 20, isCross: true },
      restoreSig,
      restoreNonce
    );
    logProgress('Leverage restored to 20x');
  });
}
