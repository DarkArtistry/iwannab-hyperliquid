/**
 * Account State Tests
 *
 * Tests for account queries and state retrieval.
 */

import { TEST_ACCOUNTS, infoRequest, runTest, logSection, log, logProgress } from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runAccountTests(ctx: TestContext): Promise<void> {
  logSection('3. Account State Tests');
  log('');
  log('  Testing account queries for test wallets');
  log('');

  await runTest(ctx, 'Get Alice account state', 'account', 'Retrieve full account state including margin and positions', async () => {
    logProgress(`Fetching account state for ${TEST_ACCOUNTS.ALICE.address}...`);
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      marginSummary?: { accountValue?: string; totalRawUsd?: string; withdrawable?: string; totalMarginUsed?: string; totalNtlPos?: string };
      assetPositions?: unknown[];
      crossMaintenanceMarginUsed?: string;
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    if (state.marginSummary.accountValue === undefined) throw new Error('Missing marginSummary.accountValue');
    if (state.marginSummary.totalRawUsd === undefined) throw new Error('Missing marginSummary.totalRawUsd');
    if (state.marginSummary.withdrawable === undefined) throw new Error('Missing marginSummary.withdrawable');
    if (state.assetPositions === undefined) throw new Error('Missing assetPositions');
    const accountValue = parseFloat(state.marginSummary.accountValue);
    const withdrawable = parseFloat(state.marginSummary.withdrawable);
    const totalRawUsd = parseFloat(state.marginSummary.totalRawUsd);
    if (isNaN(accountValue) || accountValue <= 0) throw new Error(`Alice accountValue should be > 0 (funded by genesis), got ${state.marginSummary.accountValue}`);
    if (isNaN(withdrawable)) throw new Error(`withdrawable is not a number: ${state.marginSummary.withdrawable}`);
    if (isNaN(totalRawUsd)) throw new Error(`totalRawUsd is not a number: ${state.marginSummary.totalRawUsd}`);
    // Verify margin field relationships
    if (withdrawable > accountValue + 0.01) throw new Error(`withdrawable ($${withdrawable}) > accountValue ($${accountValue})`);
    logProgress(`Account value: $${state.marginSummary.accountValue}, withdrawable: $${state.marginSummary.withdrawable}, positions: ${state.assetPositions.length}`);
  });

  await runTest(ctx, 'Get Bob account state', 'account', 'Retrieve Bob account for matching tests', async () => {
    logProgress(`Fetching account state for ${TEST_ACCOUNTS.BOB.address}...`);
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.BOB.address })) as {
      marginSummary?: { accountValue?: string; totalRawUsd?: string; withdrawable?: string; totalMarginUsed?: string; totalNtlPos?: string };
      assetPositions?: unknown[];
      crossMaintenanceMarginUsed?: string;
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    if (state.marginSummary.accountValue === undefined) throw new Error('Missing marginSummary.accountValue');
    if (state.marginSummary.totalRawUsd === undefined) throw new Error('Missing marginSummary.totalRawUsd');
    if (state.marginSummary.withdrawable === undefined) throw new Error('Missing marginSummary.withdrawable');
    if (state.assetPositions === undefined) throw new Error('Missing assetPositions');
    const accountValue = parseFloat(state.marginSummary.accountValue);
    const withdrawable = parseFloat(state.marginSummary.withdrawable);
    if (isNaN(accountValue) || accountValue <= 0) throw new Error(`Bob accountValue should be > 0 (funded by genesis), got ${state.marginSummary.accountValue}`);
    if (withdrawable > accountValue + 0.01) throw new Error(`withdrawable ($${withdrawable}) > accountValue ($${accountValue})`);
    logProgress(`Account value: $${state.marginSummary.accountValue}, withdrawable: $${state.marginSummary.withdrawable}, positions: ${state.assetPositions.length}`);
  });

  await runTest(ctx, 'Get open orders (Alice)', 'account', 'Retrieve all open orders for an account', async () => {
    logProgress('Fetching open orders...');
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ oid?: number; coin?: string; side?: string; limitPx?: string; sz?: string }>;
    if (!Array.isArray(orders)) throw new Error(`Expected array for openOrders, got ${typeof orders}`);
    // Validate structure of any returned orders
    for (const order of orders) {
      if (order.oid === undefined) throw new Error(`Order missing oid: ${JSON.stringify(order)}`);
      if (!order.side || (order.side !== 'B' && order.side !== 'A')) throw new Error(`Invalid order side: "${order.side}"`);
      if (order.limitPx === undefined || isNaN(parseFloat(order.limitPx))) throw new Error(`Invalid order price: "${order.limitPx}"`);
      if (order.sz === undefined || isNaN(parseFloat(order.sz))) throw new Error(`Invalid order size: "${order.sz}"`);
    }
    logProgress(`Found ${orders.length} open orders (all structurally valid)`);
  });

  await runTest(ctx, 'Get user fills (Alice)', 'account', 'Retrieve trade fill history for an account', async () => {
    logProgress('Fetching fill history...');
    const fills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ px?: string; sz?: string; side?: string; time?: number; oid?: number }>;
    if (!Array.isArray(fills)) throw new Error(`Expected array for userFills, got ${typeof fills}`);
    // Validate structure of any returned fills
    for (const fill of fills) {
      if (fill.px === undefined || isNaN(parseFloat(fill.px))) throw new Error(`Invalid fill price: "${fill.px}"`);
      if (fill.sz === undefined || isNaN(parseFloat(fill.sz))) throw new Error(`Invalid fill size: "${fill.sz}"`);
      if (fill.side !== undefined && fill.side !== 'B' && fill.side !== 'A') throw new Error(`Invalid fill side: "${fill.side}"`);
    }
    logProgress(`Found ${fills.length} fills (all structurally valid)`);
  });

  await runTest(ctx, 'Get user funding (Alice)', 'account', 'Retrieve funding payment history', async () => {
    logProgress('Fetching funding history...');
    const funding = (await infoRequest('userFundingHistory', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ delta?: { coin?: string; fundingRate?: string }; time?: number }>;
    if (!Array.isArray(funding)) {
      throw new Error(`Expected array for userFundingHistory, got ${typeof funding}`);
    }
    // Validate structure of any returned entries
    for (const entry of funding) {
      if (entry.time !== undefined && (typeof entry.time !== 'number' || entry.time <= 0)) {
        throw new Error(`Invalid funding timestamp: ${entry.time}`);
      }
    }
    logProgress(`Funding history: ${funding.length} valid entries`);
  });
}
