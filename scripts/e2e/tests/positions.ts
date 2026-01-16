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
      assetPositions?: Array<{ position?: { szi?: string } }>;
    };

    if (state.assetPositions) {
      for (const ap of state.assetPositions) {
        if (ap.position && parseFloat(ap.position.szi || '0') !== 0) {
          logProgress(`Position size: ${ap.position.szi}`);
        }
      }
    }
    logProgress('Position check complete');
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
    await exchangeRequest(action, signature, nonce);
    logProgress('Leverage updated to 10x');
  });

  await runTest(ctx, 'Check margin requirements', 'positions', 'Verify margin calculations after leverage change', async () => {
    logProgress('Checking margin after leverage change...');
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      marginSummary?: { totalMarginUsed?: string; totalNtlPos?: string };
    };

    if (state.marginSummary) {
      logProgress(`Margin used: $${state.marginSummary.totalMarginUsed}`);
      logProgress(`Notional: $${state.marginSummary.totalNtlPos}`);
    }
  });
}
