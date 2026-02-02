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
      marginSummary?: { accountValue?: string };
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    logProgress(`Account value: $${state.marginSummary.accountValue}`);
  });

  await runTest(ctx, 'Get Bob account state', 'account', 'Retrieve Bob account for matching tests', async () => {
    logProgress(`Fetching account state for ${TEST_ACCOUNTS.BOB.address}...`);
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.BOB.address })) as {
      marginSummary?: { accountValue?: string };
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    logProgress(`Account value: $${state.marginSummary.accountValue}`);
  });

  await runTest(ctx, 'Get open orders (Alice)', 'account', 'Retrieve all open orders for an account', async () => {
    logProgress('Fetching open orders...');
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    logProgress(`Found ${orders.length} open orders`);
  });

  await runTest(ctx, 'Get user fills (Alice)', 'account', 'Retrieve trade fill history for an account', async () => {
    logProgress('Fetching fill history...');
    const fills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    logProgress(`Found ${fills.length} fills`);
  });

  await runTest(ctx, 'Get user funding (Alice)', 'account', 'Retrieve funding payment history', async () => {
    logProgress('Fetching funding history...');
    const funding = await infoRequest('userFundingHistory', { user: TEST_ACCOUNTS.ALICE.address });
    if (!Array.isArray(funding)) {
      throw new Error(`Expected array for userFundingHistory, got ${typeof funding}`);
    }
    logProgress(`Funding history retrieved: ${funding.length} entries`);
  });
}
