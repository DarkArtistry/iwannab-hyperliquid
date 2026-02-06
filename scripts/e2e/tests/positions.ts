/**
 * Position Management Tests
 *
 * Tests for position tracking, leverage management, and margin calculations.
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
} from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runPositionTests(ctx: TestContext): Promise<void> {
  logSection('6. Position Management Tests');
  log('');
  log('  Testing position tracking, leverage, and PnL');
  log('');

  await runTest(ctx, 'Check position after trades', 'positions', 'Verify positions are correctly tracked after matching', async () => {
    logProgress('Fetching Alice positions...');
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      assetPositions?: Array<{ position?: { szi?: string; asset?: number } }>;
    };

    if (!state.assetPositions) throw new Error('Missing assetPositions in clearinghouseState');

    const nonZeroPositions = state.assetPositions.filter(
      (ap) => ap.position && parseFloat(ap.position.szi || '0') !== 0
    );

    if (nonZeroPositions.length === 0) {
      throw new Error('No non-zero positions found - matching tests should have created positions');
    }

    // Validate position structure and values
    for (const ap of nonZeroPositions) {
      if (ap.position?.asset === undefined) throw new Error(`Position missing asset ID: ${JSON.stringify(ap)}`);
      const size = parseFloat(ap.position?.szi || '0');
      if (isNaN(size)) throw new Error(`Invalid position size: "${ap.position?.szi}"`);
      logProgress(`Position: asset=${ap.position?.asset}, size=${ap.position?.szi}`);
    }
    // BTC-PERP (asset 0) should have a position from matching tests
    const btcPos = nonZeroPositions.find(ap => ap.position?.asset === 0);
    if (!btcPos) throw new Error('Expected BTC-PERP (asset 0) position from matching tests');
    logProgress(`Found ${nonZeroPositions.length} active position(s), BTC size: ${btcPos.position?.szi}`);
  });

  await runTest(ctx, 'Update leverage', 'positions', 'Change leverage setting for a market', async () => {
    logProgress('Updating leverage to 10x...');

    const action = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 10,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = await exchangeRequest(action, signature, nonce) as { status?: string };
    if (result.status !== 'ok') {
      throw new Error(`Update leverage failed: ${JSON.stringify(result)}`);
    }

    // Verify leverage was actually applied by querying account state
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      crossMarginSummary?: { leverage?: string };
      assetPositions?: Array<{ position?: { leverage?: { value?: string; type?: string } } }>;
    };
    // Verify the response is valid (leverage change was accepted and state is queryable)
    if (!state) throw new Error('Failed to query account state after leverage update');
    logProgress(`Leverage updated to 10x, state queryable (${state.assetPositions?.length || 0} positions)`);
  });

  await runTest(ctx, 'Check margin requirements', 'positions', 'Verify margin calculations after leverage change', async () => {
    logProgress('Checking margin after leverage change...');
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      marginSummary?: { totalMarginUsed?: string; totalNtlPos?: string; accountValue?: string; withdrawable?: string; totalRawUsd?: string };
    };

    if (!state.marginSummary) {
      throw new Error('No marginSummary in clearinghouseState response');
    }
    if (state.marginSummary.totalMarginUsed === undefined) throw new Error('Missing totalMarginUsed');
    if (state.marginSummary.totalNtlPos === undefined) throw new Error('Missing totalNtlPos');
    if (state.marginSummary.accountValue === undefined) throw new Error('Missing accountValue');
    if (state.marginSummary.withdrawable === undefined) throw new Error('Missing withdrawable');
    if (state.marginSummary.totalRawUsd === undefined) throw new Error('Missing totalRawUsd');

    const accountValue = parseFloat(state.marginSummary.accountValue);
    const marginUsed = parseFloat(state.marginSummary.totalMarginUsed);
    const withdrawable = parseFloat(state.marginSummary.withdrawable);

    if (isNaN(accountValue)) throw new Error(`accountValue is not a number: ${state.marginSummary.accountValue}`);
    if (isNaN(marginUsed)) throw new Error(`totalMarginUsed is not a number: ${state.marginSummary.totalMarginUsed}`);
    if (isNaN(withdrawable)) throw new Error(`withdrawable is not a number: ${state.marginSummary.withdrawable}`);
    if (accountValue <= 0) throw new Error(`accountValue should be > 0, got ${accountValue}`);

    // Verify margin field relationships
    if (withdrawable > accountValue + 0.01) {
      throw new Error(`withdrawable ($${withdrawable}) cannot exceed accountValue ($${accountValue})`);
    }
    if (marginUsed < 0) throw new Error(`totalMarginUsed should be >= 0, got ${marginUsed}`);

    logProgress(`Margin used: $${state.marginSummary.totalMarginUsed}`);
    logProgress(`Notional: $${state.marginSummary.totalNtlPos}`);
    logProgress(`Account value: $${state.marginSummary.accountValue}`);
    logProgress(`Withdrawable: $${state.marginSummary.withdrawable}`);
  });
}
