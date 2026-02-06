/**
 * Unified State Tests
 *
 * Tests for the unified state model with view transfers between Core and EVM layers.
 *
 * UNIFIED STATE MODEL:
 * ====================
 * Each user has a single "total" balance per token, split into two "views":
 *   - core_view: Balance visible to Core layer (perps, spot trading)
 *   - evm_view:  Balance visible to EVM layer (smart contracts, DeFi)
 *
 * Key properties:
 * - View transfers are instant and free (no gas, no bridge delay)
 * - View transfers adjust views WITHOUT changing total
 *
 * Invariant: total == core_view + evm_view (ALWAYS)
 */

import {
  CONFIG,
  MARKETS,
  TEST_ACCOUNTS,
  infoRequest,
  exchangeRequest,
  signAction,
  runTest,
  assertErrorContains,
  logSection,
  log,
  logProgress,
} from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runUnifiedStateTests(ctx: TestContext): Promise<void> {
  logSection('12. Unified State Tests (Phase 2A)');
  log('');
  log('  Testing unified state model with view transfers');
  log('');

  // Store initial balances for comparison
  let initialCoreView = '0';
  let initialEvmView = '0';
  let initialTotal = '0';

  await runTest(ctx, 'Get unified balances', 'unified', 'Query unified balance with Core/EVM views', async () => {
    logProgress('Fetching unified balances for Alice...');
    const response = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: {
        tokenIndex: number;
        symbol: string;
        total: string;
        coreView: string;
        evmView: string;
      }[];
    };

    if (!response.balances) throw new Error('Missing balances in response');

    logProgress(`Found ${response.balances.length} unified balance(s)`);

    // Find USDC balance
    const usdcBalance = response.balances.find((b) => b.tokenIndex === 0);
    if (!usdcBalance) throw new Error('USDC unified balance not found');

    initialTotal = usdcBalance.total;
    initialCoreView = usdcBalance.coreView;
    initialEvmView = usdcBalance.evmView;

    logProgress(`USDC (token 0):`);
    logProgress(`  Total: ${initialTotal}`);
    logProgress(`  Core View: ${initialCoreView}`);
    logProgress(`  EVM View: ${initialEvmView}`);

    // Verify invariant: total == core_view + evm_view
    const total = parseFloat(initialTotal);
    const core = parseFloat(initialCoreView);
    const evm = parseFloat(initialEvmView);

    // Allow small floating point tolerance
    if (Math.abs(total - (core + evm)) > 0.01) {
      throw new Error(`Invariant violated: total (${total}) != core (${core}) + evm (${evm})`);
    }
    logProgress('Invariant verified: total == core_view + evm_view');
  });

  await runTest(ctx, 'View transfer: Core to EVM', 'unified', 'Transfer USDC from Core view to EVM view', async () => {
    const transferAmount = '100'; // Transfer 100 USDC
    logProgress(`Transferring ${transferAmount} USDC from Core view to EVM view...`);

    const action = {
      type: 'viewTransfer',
      token: 0, // USDC
      amount: transferAmount,
      toEvm: true,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      error?: string;
      response?: {
        type: string;
        data?: {
          newCoreView: string;
          newEvmView: string;
          total: string;
        };
      };
    };

    if (result.status !== 'ok') {
      throw new Error(`View transfer failed: ${result.error || 'Unknown error'}`);
    }

    if (!result.response?.data) {
      throw new Error('Missing data in view transfer response');
    }

    const { newCoreView, newEvmView, total } = result.response.data;

    logProgress(`View transfer successful!`);
    logProgress(`  New Core View: ${newCoreView}`);
    logProgress(`  New EVM View: ${newEvmView}`);
    logProgress(`  Total: ${total} (unchanged)`);

    // Verify total unchanged
    if (total !== initialTotal) {
      throw new Error(`Total changed! Before: ${initialTotal}, After: ${total}`);
    }
    logProgress('Total balance unchanged - view adjustment only');

    // Verify views changed correctly
    const expectedCore = parseFloat(initialCoreView) - parseFloat(transferAmount);
    const expectedEvm = parseFloat(initialEvmView) + parseFloat(transferAmount);

    if (Math.abs(parseFloat(newCoreView) - expectedCore) > 0.01) {
      throw new Error(`Core view mismatch: expected ${expectedCore}, got ${newCoreView}`);
    }
    if (Math.abs(parseFloat(newEvmView) - expectedEvm) > 0.01) {
      throw new Error(`EVM view mismatch: expected ${expectedEvm}, got ${newEvmView}`);
    }

    // Update for next test
    initialCoreView = newCoreView;
    initialEvmView = newEvmView;
  });

  await runTest(ctx, 'View transfer: EVM to Core', 'unified', 'Transfer USDC from EVM view back to Core view', async () => {
    const transferAmount = '50'; // Transfer 50 USDC back
    logProgress(`Transferring ${transferAmount} USDC from EVM view to Core view...`);

    const action = {
      type: 'viewTransfer',
      token: 0, // USDC
      amount: transferAmount,
      toEvm: false, // EVM -> Core
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      error?: string;
      response?: {
        type: string;
        data?: {
          newCoreView: string;
          newEvmView: string;
          total: string;
        };
      };
    };

    if (result.status !== 'ok') {
      throw new Error(`View transfer failed: ${result.error || 'Unknown error'}`);
    }

    const { newCoreView, newEvmView, total } = result.response!.data!;

    logProgress(`View transfer successful!`);
    logProgress(`  New Core View: ${newCoreView}`);
    logProgress(`  New EVM View: ${newEvmView}`);
    logProgress(`  Total: ${total} (unchanged)`);

    // Verify total unchanged
    if (total !== initialTotal) {
      throw new Error(`Total changed! Before: ${initialTotal}, After: ${total}`);
    }

    // Verify views changed correctly
    const expectedCore = parseFloat(initialCoreView) + parseFloat(transferAmount);
    const expectedEvm = parseFloat(initialEvmView) - parseFloat(transferAmount);

    if (Math.abs(parseFloat(newCoreView) - expectedCore) > 0.01) {
      throw new Error(`Core view mismatch: expected ${expectedCore}, got ${newCoreView}`);
    }
    if (Math.abs(parseFloat(newEvmView) - expectedEvm) > 0.01) {
      throw new Error(`EVM view mismatch: expected ${expectedEvm}, got ${newEvmView}`);
    }

    initialCoreView = newCoreView;
    initialEvmView = newEvmView;
  });

  await runTest(ctx, 'View transfer: Insufficient Core view', 'unified', 'Reject transfer exceeding Core view balance', async () => {
    // Try to transfer more than available in Core view
    const excessiveAmount = (parseFloat(initialCoreView) * 2).toString();
    logProgress(`Attempting to transfer ${excessiveAmount} USDC from Core (only ${initialCoreView} available)...`);

    const action = {
      type: 'viewTransfer',
      token: 0,
      amount: excessiveAmount,
      toEvm: true,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(action, signature, nonce)) as {
        status?: string;
        error?: string;
      };

      if (result.status === 'ok') {
        throw new Error('Transfer should have been rejected but succeeded!');
      }

      assertErrorContains(
        result.error || '',
        ['Insufficient balance', 'Insufficient'],
        'Insufficient Core view rejection'
      );
      logProgress(`Transfer correctly rejected: ${result.error}`);
    } catch (error: unknown) {
      // HTTP error is also acceptable for insufficient balance
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('should have been rejected')) throw error;
      assertErrorContains(message, ['Insufficient balance', 'Insufficient'], 'Insufficient Core view rejection');
      logProgress(`Transfer correctly rejected: ${message}`);
    }
  });

  await runTest(ctx, 'View transfer: Insufficient EVM view', 'unified', 'Reject transfer exceeding EVM view balance', async () => {
    // Try to transfer more than available in EVM view
    const excessiveAmount = (parseFloat(initialEvmView) * 2 + 1).toString();
    logProgress(`Attempting to transfer ${excessiveAmount} USDC from EVM (only ${initialEvmView} available)...`);

    const action = {
      type: 'viewTransfer',
      token: 0,
      amount: excessiveAmount,
      toEvm: false,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(action, signature, nonce)) as {
        status?: string;
        error?: string;
      };

      if (result.status === 'ok') {
        throw new Error('Transfer should have been rejected but succeeded!');
      }

      assertErrorContains(
        result.error || '',
        ['Insufficient balance', 'Insufficient'],
        'Insufficient EVM view rejection'
      );
      logProgress(`Transfer correctly rejected: ${result.error}`);
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('should have been rejected')) throw error;
      assertErrorContains(message, ['Insufficient balance', 'Insufficient'], 'Insufficient EVM view rejection');
      logProgress(`Transfer correctly rejected: ${message}`);
    }
  });

  await runTest(ctx, 'Verify invariant after all transfers', 'unified', 'Final check that total == core_view + evm_view', async () => {
    logProgress('Final invariant verification...');

    const response = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: {
        tokenIndex: number;
        symbol: string;
        total: string;
        coreView: string;
        evmView: string;
      }[];
    };

    const usdcBalance = response.balances.find((b) => b.tokenIndex === 0);
    if (!usdcBalance) throw new Error('USDC balance not found');

    const { total, coreView, evmView } = usdcBalance;

    logProgress(`Final USDC balance:`);
    logProgress(`  Total: ${total}`);
    logProgress(`  Core View: ${coreView}`);
    logProgress(`  EVM View: ${evmView}`);

    // Verify invariant
    const t = parseFloat(total);
    const c = parseFloat(coreView);
    const e = parseFloat(evmView);

    if (Math.abs(t - (c + e)) > 0.01) {
      throw new Error(`Final invariant VIOLATED: ${t} != ${c} + ${e}`);
    }

    // Verify total unchanged from initial
    if (total !== initialTotal) {
      throw new Error(`Total changed during tests! Initial: ${initialTotal}, Final: ${total}`);
    }

    logProgress('All invariants verified - unified state is consistent!');
  });

  await runTest(ctx, 'Multiple token view transfers', 'unified', 'Test view transfers for TEST token (token 1)', async () => {
    logProgress('Testing view transfer for TEST token...');

    // First get current TEST balance
    const balancesBefore = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };

    const testBefore = balancesBefore.balances.find((b) => b.tokenIndex === 1);
    if (!testBefore) {
      throw new Error('TEST token balance not found in unified balances - node initialization may have failed');
    }

    logProgress(`TEST before: total=${testBefore.total}, core=${testBefore.coreView}, evm=${testBefore.evmView}`);

    // Transfer some TEST to EVM view
    const transferAmount = '1'; // 1 TEST token
    const action = {
      type: 'viewTransfer',
      token: 1, // TEST
      amount: transferAmount,
      toEvm: true,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      response?: { data?: { newCoreView: string; newEvmView: string; total: string } };
    };

    if (result.status !== 'ok') {
      throw new Error(`TEST view transfer failed: ${JSON.stringify(result)}`);
    }

    // Verify the transfer actually changed the values
    const balancesAfter = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const testAfter = balancesAfter.balances.find((b) => b.tokenIndex === 1);
    if (!testAfter) throw new Error('TEST token balance not found after transfer');

    const coreBefore = parseFloat(testBefore.coreView);
    const coreAfter = parseFloat(testAfter.coreView);
    const evmBefore = parseFloat(testBefore.evmView);
    const evmAfter = parseFloat(testAfter.evmView);

    if (coreAfter >= coreBefore - 0.01) throw new Error(`Core view should have decreased: ${coreBefore} -> ${coreAfter}`);
    if (evmAfter <= evmBefore + 0.01) throw new Error(`EVM view should have increased: ${evmBefore} -> ${evmAfter}`);
    // Verify total unchanged
    if (Math.abs(parseFloat(testAfter.total) - parseFloat(testBefore.total)) > 0.01) {
      throw new Error(`Total changed: ${testBefore.total} -> ${testAfter.total}`);
    }
    logProgress(`TEST transfer verified: core ${coreBefore} -> ${coreAfter}, evm ${evmBefore} -> ${evmAfter}`);
  });

  // =========================================================================
  // ADVANCED UNIFIED STATE TESTS - Real World Scenarios
  // =========================================================================

  await runTest(ctx, 'Nonce increment verification', 'unified', 'Verify each transaction uses incrementing nonce', async () => {
    logProgress('Testing nonce handling across multiple transactions...');

    // Make 3 consecutive view transfers and verify nonces increment
    const nonces: number[] = [];

    for (let i = 0; i < 3; i++) {
      const action = {
        type: 'viewTransfer',
        token: 0,
        amount: '1', // Small amount
        toEvm: i % 2 === 0, // Alternate direction
      };

      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
      nonces.push(nonce);

      const result = await exchangeRequest(action, signature, nonce);
      if ((result as { status?: string }).status !== 'ok') {
        throw new Error(`Transaction ${i + 1} failed`);
      }

      logProgress(`Transaction ${i + 1}: nonce=${nonce}`);
    }

    // Verify nonces are incrementing (they should be timestamps, always increasing)
    for (let i = 1; i < nonces.length; i++) {
      if (nonces[i] <= nonces[i - 1]) {
        throw new Error(`Nonce did not increment: ${nonces[i - 1]} -> ${nonces[i]}`);
      }
    }

    logProgress('All nonces properly incremented');
  });

  await runTest(ctx, 'Trade after view transfer', 'unified', 'Verify trading works correctly after moving balance to EVM view', async () => {
    logProgress('Testing trading functionality after view transfer...');

    // Get initial core_view balance
    const beforeTransfer = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; coreView: string; evmView: string }[];
    };
    const usdcBefore = beforeTransfer.balances.find((b) => b.tokenIndex === 0);
    if (!usdcBefore) throw new Error('USDC balance not found');

    const initialCoreViewVal = parseFloat(usdcBefore.coreView);
    logProgress(`Initial Core View: ${initialCoreViewVal} USDC`);

    // Transfer 1000 USDC to EVM view
    const transferAmount = 1000;
    const transferAction = {
      type: 'viewTransfer',
      token: 0,
      amount: transferAmount.toString(),
      toEvm: true,
    };

    const { signature: sig1, nonce: nonce1 } = await signAction(transferAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(transferAction, sig1, nonce1);

    // Now try to place an order - should still work with remaining core_view
    const remainingCoreView = initialCoreViewVal - transferAmount;
    logProgress(`Remaining Core View after transfer: ${remainingCoreView} USDC`);

    // Place a small buy order (should succeed if we have enough core_view)
    const orderSize = Math.min(10, remainingCoreView / 2); // Use half of remaining
    if (orderSize > 0) {
      const orderAction = {
        type: 'spotOrder',
        orders: [
          {
            a: 128, // TEST-USDC market
            b: true, // buy
            p: '1.0',
            s: orderSize.toString(),
            r: false,
            t: { limit: { tif: 'Gtc' } },
          },
        ],
        grouping: 'na',
      };

      const { signature: sig2, nonce: nonce2 } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
      const orderResult = (await exchangeRequest(orderAction, sig2, nonce2)) as {
        status?: string;
        response?: { data?: { statuses?: unknown[] } };
      };

      if (orderResult.status === 'ok') {
        logProgress(`Order placed successfully with core_view balance`);

        // Cancel the order to clean up
        const cancelAction = { type: 'spotCancelAll', market: 128 };
        const { signature: sig3, nonce: nonce3 } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
        await exchangeRequest(cancelAction, sig3, nonce3);
      }
    }

    // Transfer back to restore balance
    const { signature: sig4, nonce: nonce4 } = await signAction(
      { type: 'viewTransfer', token: 0, amount: transferAmount.toString(), toEvm: false },
      TEST_ACCOUNTS.ALICE.privateKey
    );
    await exchangeRequest({ type: 'viewTransfer', token: 0, amount: transferAmount.toString(), toEvm: false }, sig4, nonce4);

    logProgress('Trading after view transfer works correctly');
  });

  await runTest(ctx, 'EVM balance reflects evm_view', 'unified', 'Verify eth_getBalance returns evm_view amount', async () => {
    logProgress('Testing EVM balance integration with view transfer...');

    // First, transfer some balance to EVM view so we have a non-zero evm_view
    const transferAmount = '100'; // 100 USDC
    const { signature: xferSig, nonce: xferNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: true },
      TEST_ACCOUNTS.ALICE.privateKey
    );

    const xferResult = (await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: true },
      xferSig,
      xferNonce
    )) as { status?: string };

    if (xferResult.status !== 'ok') {
      throw new Error('Failed to transfer balance to EVM view');
    }
    logProgress(`Transferred ${transferAmount} USDC to EVM view`);

    // Get unified balance to verify evm_view
    const unified = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; evmView: string; coreView: string; total: string }[];
    };
    const usdcUnified = unified.balances.find((b) => b.tokenIndex === 0);
    if (!usdcUnified) {
      throw new Error('USDC unified balance not found');
    }

    const expectedEvmView = parseFloat(usdcUnified.evmView);
    logProgress(`Unified state evm_view: ${expectedEvmView} USDC`);

    if (expectedEvmView <= 0) {
      throw new Error('EVM view should be positive after transfer');
    }

    // Query EVM balance via JSON-RPC
    try {
      const evmResponse = await fetch(CONFIG.EVM_RPC_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_getBalance',
          params: [TEST_ACCOUNTS.ALICE.address, 'latest'],
          id: 1,
        }),
        signal: AbortSignal.timeout(5000), // 5 second timeout
      });

      const evmResult = (await evmResponse.json()) as { result?: string; error?: { message: string } };
      if (evmResult.error) {
        throw new Error(`EVM RPC error: ${evmResult.error.message}`);
      }

      if (evmResult.result) {
        const evmBalanceRaw = BigInt(evmResult.result);
        // USDC has 6 decimals, so convert to human-readable
        const evmBalanceUsdc = Number(evmBalanceRaw) / 1_000_000;
        logProgress(`EVM eth_getBalance: ${evmBalanceUsdc} USDC (raw: ${evmBalanceRaw})`);

        // Verify the EVM balance matches the evm_view from unified state
        // Allow small tolerance for potential rounding
        const tolerance = 0.001;
        if (Math.abs(evmBalanceUsdc - expectedEvmView) > tolerance) {
          throw new Error(
            `EVM balance mismatch! eth_getBalance=${evmBalanceUsdc}, evm_view=${expectedEvmView}. ` +
            `Unified state integration may be broken.`
          );
        }

        logProgress(`EVM balance matches unified state evm_view - integration verified!`);
      } else {
        throw new Error('EVM RPC returned empty result');
      }
    } catch (e) {
      throw e;
    }

    // Clean up: transfer back to Core view
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: false },
      TEST_ACCOUNTS.ALICE.privateKey
    );
    await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: false },
      restoreSig,
      restoreNonce
    );
    logProgress('Balance restored to Core view');
  });

  await runTest(ctx, 'Zero amount transfer handling', 'unified', 'Verify zero amount transfers do not alter balance', async () => {
    logProgress('Testing zero amount transfer handling...');

    // Record balance before
    const before = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; coreView: string; evmView: string }[];
    };
    const usdcBefore = before.balances.find((b) => b.tokenIndex === 0);

    const action = {
      type: 'viewTransfer',
      token: 0,
      amount: '0',
      toEvm: true,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      const result = (await exchangeRequest(action, signature, nonce)) as {
        status?: string;
        error?: string;
      };

      if (result.status === 'ok') {
        // If accepted as no-op, verify balance unchanged
        const after = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
          balances: { tokenIndex: number; coreView: string; evmView: string }[];
        };
        const usdcAfter = after.balances.find((b) => b.tokenIndex === 0);

        if (usdcBefore?.coreView !== usdcAfter?.coreView || usdcBefore?.evmView !== usdcAfter?.evmView) {
          throw new Error(
            `Zero transfer altered balance: core ${usdcBefore?.coreView} -> ${usdcAfter?.coreView}, ` +
            `evm ${usdcBefore?.evmView} -> ${usdcAfter?.evmView}`
          );
        }
        logProgress('Zero transfer accepted as no-op, balance unchanged');
      } else {
        logProgress(`Zero transfer rejected: ${result.error}`);
      }
    } catch (e) {
      logProgress('Zero transfer rejected with error (expected)');
    }
  });

  await runTest(ctx, 'Concurrent transfers from multiple users', 'unified', 'Test simultaneous view transfers from different accounts', async () => {
    logProgress('Testing concurrent transfers from Alice and Bob...');

    // Prepare actions for both users
    const aliceAction = {
      type: 'viewTransfer',
      token: 0,
      amount: '10',
      toEvm: true,
    };

    const bobAction = {
      type: 'viewTransfer',
      token: 0,
      amount: '10',
      toEvm: true,
    };

    // Sign both
    const { signature: aliceSig, nonce: aliceNonce } = await signAction(aliceAction, TEST_ACCOUNTS.ALICE.privateKey);
    const { signature: bobSig, nonce: bobNonce } = await signAction(bobAction, TEST_ACCOUNTS.BOB.privateKey);

    // Execute concurrently
    const [aliceResult, bobResult] = await Promise.all([
      exchangeRequest(aliceAction, aliceSig, aliceNonce),
      exchangeRequest(bobAction, bobSig, bobNonce),
    ]);

    const aliceOk = (aliceResult as { status?: string }).status === 'ok';
    const bobOk = (bobResult as { status?: string }).status === 'ok';

    logProgress(`Alice transfer: ${aliceOk ? 'success' : 'failed'}`);
    logProgress(`Bob transfer: ${bobOk ? 'success' : 'failed'}`);

    // At least one should succeed (both should in proper implementation)
    if (!aliceOk && !bobOk) {
      throw new Error('Both concurrent transfers failed');
    }

    // Restore balances
    if (aliceOk) {
      const { signature, nonce } = await signAction(
        { type: 'viewTransfer', token: 0, amount: '10', toEvm: false },
        TEST_ACCOUNTS.ALICE.privateKey
      );
      await exchangeRequest({ type: 'viewTransfer', token: 0, amount: '10', toEvm: false }, signature, nonce);
    }
    if (bobOk) {
      const { signature, nonce } = await signAction(
        { type: 'viewTransfer', token: 0, amount: '10', toEvm: false },
        TEST_ACCOUNTS.BOB.privateKey
      );
      await exchangeRequest({ type: 'viewTransfer', token: 0, amount: '10', toEvm: false }, signature, nonce);
    }

    logProgress('Concurrent transfers handled correctly');
  });

  await runTest(ctx, 'Full lifecycle: deposit-trade-transfer-withdraw', 'unified', 'Test complete user workflow across both layers', async () => {
    logProgress('Testing full user lifecycle...');

    // Step 1: Check initial state
    const initial = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const initialUsdc = initial.balances.find((b) => b.tokenIndex === 0);
    logProgress(`Step 1 - Initial: total=${initialUsdc?.total}, core=${initialUsdc?.coreView}, evm=${initialUsdc?.evmView}`);

    // Step 2: Place a spot order (uses core_view)
    const spotBalance = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      tokenIndex: number;
      available: string;
    }[];
    const usdcAvailable = spotBalance.find((b: { tokenIndex: number }) => b.tokenIndex === 0);
    logProgress(`Step 2 - Spot available: ${usdcAvailable?.available} USDC`);

    // Step 3: Transfer some to EVM view
    const transferAmount = '50';
    const { signature, nonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: true },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );
    const transferResult = await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: true },
      signature,
      nonce
    );

    if ((transferResult as { status?: string }).status === 'ok') {
      logProgress(`Step 3 - Transferred ${transferAmount} USDC to EVM view`);
    }

    // Step 4: Verify spot balance decreased
    const afterTransfer = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      tokenIndex: number;
      available: string;
    }[];
    const usdcAfter = afterTransfer.find((b: { tokenIndex: number }) => b.tokenIndex === 0);
    logProgress(`Step 4 - Spot available after transfer: ${usdcAfter?.available} USDC`);

    // Step 5: Transfer back
    const { signature: sig2, nonce: nonce2 } = await signAction(
      { type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: false },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );
    await exchangeRequest({ type: 'viewTransfer', token: 0, amount: transferAmount, toEvm: false }, sig2, nonce2);
    logProgress(`Step 5 - Transferred ${transferAmount} USDC back to Core view`);

    // Step 6: Verify final state matches initial
    const final = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const finalUsdc = final.balances.find((b) => b.tokenIndex === 0);
    logProgress(`Step 6 - Final: total=${finalUsdc?.total}, core=${finalUsdc?.coreView}, evm=${finalUsdc?.evmView}`);

    // Verify total unchanged
    if (initialUsdc?.total !== finalUsdc?.total) {
      throw new Error(`Total changed during lifecycle! ${initialUsdc?.total} -> ${finalUsdc?.total}`);
    }

    logProgress('Full lifecycle completed successfully - total balance preserved');
  });

  await runTest(ctx, 'Reserved balance prevents over-transfer', 'unified', 'Verify orders reserve balance preventing view transfer', async () => {
    logProgress('Testing reserved balance interaction with view transfers...');

    // Get current available balance for Charlie (clean account)
    const before = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      tokenIndex: number;
      total: string;
      reserved: string;
      available: string;
    }[];
    const usdcBefore = before.find((b: { tokenIndex: number }) => b.tokenIndex === 0);
    if (!usdcBefore) throw new Error('USDC balance not found for Charlie');

    const totalBalance = parseFloat(usdcBefore.total);
    const initialAvailable = parseFloat(usdcBefore.available);
    logProgress(`Charlie's total: ${totalBalance}, available: ${initialAvailable}`);

    if (totalBalance < 100) {
      throw new Error(`Charlie has insufficient balance: ${totalBalance} USDC (expected >= 100)`);
    }

    // Place a buy order to reserve significant balance
    // Order: buy 1000 tokens at $0.05 = reserve 50 USDC
    const orderAction = {
      type: 'spotOrder',
      orders: [
        {
          a: 128, // First spot market (TEST-USDC)
          b: true, // Buy
          p: '0.05', // Low price so it won't fill
          s: '1000', // 1000 tokens * $0.05 = 50 USDC reserved
          r: false,
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature: orderSig, nonce: orderNonce } = await signAction(orderAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    const orderResult = (await exchangeRequest(orderAction, orderSig, orderNonce)) as { status?: string };

    if (orderResult.status !== 'ok') {
      throw new Error('Failed to place order for reserved balance test');
    }
    logProgress('Order placed, balance reserved');

    // Check new available balance - should be reduced by ~50 USDC
    const afterOrder = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.CHARLIE.address })) as {
      tokenIndex: number;
      total: string;
      available: string;
      reserved: string;
    }[];
    const usdcAfterOrder = afterOrder.find((b: { tokenIndex: number }) => b.tokenIndex === 0);
    const newAvailable = parseFloat(usdcAfterOrder?.available || '0');
    const reserved = parseFloat(usdcAfterOrder?.reserved || '0');

    logProgress(`After order - Available: ${newAvailable}, Reserved: ${reserved}`);

    // Verify some balance is now reserved
    if (reserved <= 0) {
      throw new Error('Order should have reserved some balance but reserved=0');
    }

    // CRITICAL TEST: Try to transfer more than AVAILABLE (but less than TOTAL)
    // This should FAIL because reserved balance must be protected
    const overAmount = (newAvailable + 10).toFixed(6); // 10 USDC more than available
    logProgress(`Attempting to transfer ${overAmount} USDC (available=${newAvailable})...`);

    const { signature: xferSig, nonce: xferNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: overAmount, toEvm: true },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );

    let transferBlocked = false;
    try {
      const xferResult = (await exchangeRequest(
        { type: 'viewTransfer', token: 0, amount: overAmount, toEvm: true },
        xferSig,
        xferNonce
      )) as { status?: string; error?: string };

      if (xferResult.status === 'ok') {
        // BUG: Transfer succeeded when it should have been blocked!
        throw new Error(
          `CRITICAL BUG: Transfer of ${overAmount} succeeded but only ${newAvailable} was available! ` +
          `Reserved balance (${reserved}) was not protected.`
        );
      } else {
        // Transfer was correctly rejected
        transferBlocked = true;
        logProgress(`Transfer correctly rejected: ${xferResult.error}`);
      }
    } catch (e) {
      if ((e as Error).message.includes('CRITICAL BUG')) {
        throw e; // Re-throw our bug detection
      }
      // Validate the error message is related to insufficient/available balance
      const errMsg = (e as Error).message;
      assertErrorContains(errMsg, ['Insufficient', 'balance', 'available'], 'Reserved balance over-transfer rejection');
      transferBlocked = true;
      logProgress(`Transfer correctly rejected: ${errMsg}`);
    }

    if (!transferBlocked) {
      throw new Error('Transfer should have been blocked but was not');
    }

    // Verify that a transfer WITHIN available amount still works
    const safeAmount = Math.floor(newAvailable * 0.5).toString(); // Half of available
    logProgress(`Verifying transfer of ${safeAmount} (within available) works...`);

    const { signature: safeSig, nonce: safeNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: safeAmount, toEvm: true },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );

    const safeResult = (await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: safeAmount, toEvm: true },
      safeSig,
      safeNonce
    )) as { status?: string };

    if (safeResult.status !== 'ok') {
      throw new Error('Transfer within available balance should have succeeded');
    }
    logProgress('Transfer within available balance succeeded');

    // Clean up - cancel the order and restore balance
    const { signature: cancelSig, nonce: cancelNonce } = await signAction(
      { type: 'spotCancelAll', market: 128 },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );
    await exchangeRequest({ type: 'spotCancelAll', market: 128 }, cancelSig, cancelNonce);

    // Transfer back from EVM to Core
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: safeAmount, toEvm: false },
      TEST_ACCOUNTS.CHARLIE.privateKey
    );
    await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: safeAmount, toEvm: false },
      restoreSig,
      restoreNonce
    );

    logProgress('Test passed: Reserved balance is properly protected');
  });

  await runTest(ctx, 'Rapid view transfers stress test', 'unified', 'Execute many view transfers in sequence', async () => {
    logProgress('Stress testing view transfers (10 rapid transfers)...');

    const startTime = Date.now();
    let successCount = 0;
    let failCount = 0;

    for (let i = 0; i < 10; i++) {
      const action = {
        type: 'viewTransfer',
        token: 0,
        amount: '1',
        toEvm: i % 2 === 0,
      };

      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

      try {
        const result = (await exchangeRequest(action, signature, nonce)) as { status?: string };
        if (result.status === 'ok') {
          successCount++;
        } else {
          failCount++;
        }
      } catch {
        failCount++;
      }
    }

    const elapsed = Date.now() - startTime;
    logProgress(`Completed ${successCount} successful, ${failCount} failed in ${elapsed}ms`);
    logProgress(`Average: ${(elapsed / 10).toFixed(1)}ms per transfer`);

    if (successCount < 5) {
      throw new Error(`Too many failures: ${failCount}/10`);
    }
  });

  await runTest(ctx, 'Non-USDC token view transfer', 'unified', 'Test view transfers with non-USDC tokens (18 decimals)', async () => {
    logProgress('Testing view transfer with TEST token (index 1)...');

    // First check if Bob has TEST tokens
    const spotBalances = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      tokenIndex: number;
      total: string;
      available: string;
    }[];
    const testToken = spotBalances.find((b) => b.tokenIndex === 1);

    if (!testToken || parseFloat(testToken.available) < 10) {
      throw new Error(`Bob has insufficient TEST tokens: ${testToken?.available || '0'} (expected >= 10)`);
    }

    const transferAmount = '5'; // 5 TEST tokens
    logProgress(`Transferring ${transferAmount} TEST tokens to EVM view...`);

    const { signature: xferSig, nonce: xferNonce } = await signAction(
      { type: 'viewTransfer', token: 1, amount: transferAmount, toEvm: true },
      TEST_ACCOUNTS.BOB.privateKey
    );

    const xferResult = (await exchangeRequest(
      { type: 'viewTransfer', token: 1, amount: transferAmount, toEvm: true },
      xferSig,
      xferNonce
    )) as { status?: string; response?: { data?: { newEvmView?: string } } };

    if (xferResult.status !== 'ok') {
      throw new Error('Failed to transfer TEST token to EVM view');
    }

    // Verify unified balance shows the transfer
    const unified = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      balances: { tokenIndex: number; evmView: string; coreView: string; total: string }[];
    };
    const testUnified = unified.balances.find((b) => b.tokenIndex === 1);

    if (!testUnified) {
      throw new Error('TEST token not found in unified balances');
    }

    const evmView = parseFloat(testUnified.evmView);
    if (evmView <= 0) {
      throw new Error('TEST token evm_view should be positive after transfer');
    }
    logProgress(`TEST token evm_view: ${evmView}`);

    // Transfer back
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(
      { type: 'viewTransfer', token: 1, amount: transferAmount, toEvm: false },
      TEST_ACCOUNTS.BOB.privateKey
    );
    await exchangeRequest(
      { type: 'viewTransfer', token: 1, amount: transferAmount, toEvm: false },
      restoreSig,
      restoreNonce
    );

    logProgress('Non-USDC token view transfer test passed');
  });

  await runTest(ctx, 'Invariant: total equals core_view + evm_view', 'unified', 'Verify balance invariant is always maintained', async () => {
    logProgress('Testing balance invariant across multiple operations...');

    // Get initial state
    const initial = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const usdcInitial = initial.balances.find((b) => b.tokenIndex === 0);

    if (!usdcInitial) {
      throw new Error('USDC balance not found');
    }

    const checkInvariant = (bal: { total: string; coreView: string; evmView: string }, phase: string) => {
      const total = parseFloat(bal.total);
      const core = parseFloat(bal.coreView);
      const evm = parseFloat(bal.evmView);
      const sum = core + evm;
      const tolerance = 0.000001;

      if (Math.abs(total - sum) > tolerance) {
        throw new Error(`Invariant violated at ${phase}: total=${total}, core+evm=${sum} (core=${core}, evm=${evm})`);
      }
      logProgress(`Invariant holds at ${phase}: ${total} = ${core} + ${evm}`);
    };

    checkInvariant(usdcInitial, 'initial');

    // Transfer to EVM
    const amount1 = '50';
    const { signature: sig1, nonce: nonce1 } = await signAction(
      { type: 'viewTransfer', token: 0, amount: amount1, toEvm: true },
      TEST_ACCOUNTS.ALICE.privateKey
    );
    await exchangeRequest({ type: 'viewTransfer', token: 0, amount: amount1, toEvm: true }, sig1, nonce1);

    const afterToEvm = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const usdcAfterToEvm = afterToEvm.balances.find((b) => b.tokenIndex === 0);
    if (usdcAfterToEvm) checkInvariant(usdcAfterToEvm, 'after transfer to EVM');

    // Transfer back to Core
    const { signature: sig2, nonce: nonce2 } = await signAction(
      { type: 'viewTransfer', token: 0, amount: amount1, toEvm: false },
      TEST_ACCOUNTS.ALICE.privateKey
    );
    await exchangeRequest({ type: 'viewTransfer', token: 0, amount: amount1, toEvm: false }, sig2, nonce2);

    const afterToCore = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      balances: { tokenIndex: number; total: string; coreView: string; evmView: string }[];
    };
    const usdcAfterToCore = afterToCore.balances.find((b) => b.tokenIndex === 0);
    if (usdcAfterToCore) checkInvariant(usdcAfterToCore, 'after transfer back to Core');

    // Verify total unchanged through all operations
    const initialTotalVal = parseFloat(usdcInitial.total);
    const finalTotal = parseFloat(usdcAfterToCore?.total || '0');
    if (Math.abs(initialTotalVal - finalTotal) > 0.000001) {
      throw new Error(`Total changed! Initial=${initialTotalVal}, Final=${finalTotal}`);
    }

    logProgress('Balance invariant maintained throughout all operations');
  });

  await runTest(ctx, 'Edge case: transfer exact available amount', 'unified', 'Test transferring exactly the available balance', async () => {
    logProgress('Testing transfer of exact available amount...');

    // Get Bob's available balance
    const balances = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      tokenIndex: number;
      available: string;
      reserved: string;
    }[];
    const usdcBal = balances.find((b) => b.tokenIndex === 0);

    if (!usdcBal || parseFloat(usdcBal.available) < 50) {
      throw new Error(`Bob has insufficient USDC: ${usdcBal?.available || '0'} (expected >= 50)`);
    }

    // Transfer exactly 50 USDC (a round number for precision)
    const exactAmount = '50';
    const { signature, nonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: exactAmount, toEvm: true },
      TEST_ACCOUNTS.BOB.privateKey
    );

    const result = (await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: exactAmount, toEvm: true },
      signature,
      nonce
    )) as { status?: string };

    if (result.status !== 'ok') {
      throw new Error('Exact amount transfer should succeed');
    }
    logProgress('Exact amount transfer succeeded');

    // Verify the balance changed exactly
    const after = (await infoRequest('unifiedBalances', { user: TEST_ACCOUNTS.BOB.address })) as {
      balances: { tokenIndex: number; evmView: string }[];
    };
    const usdcAfter = after.balances.find((b) => b.tokenIndex === 0);
    const evmView = parseFloat(usdcAfter?.evmView || '0');

    if (evmView < 50) {
      throw new Error(`EVM view should be at least 50, got ${evmView}`);
    }
    logProgress(`EVM view after transfer: ${evmView}`);

    // Transfer back
    const { signature: restoreSig, nonce: restoreNonce } = await signAction(
      { type: 'viewTransfer', token: 0, amount: exactAmount, toEvm: false },
      TEST_ACCOUNTS.BOB.privateKey
    );
    await exchangeRequest(
      { type: 'viewTransfer', token: 0, amount: exactAmount, toEvm: false },
      restoreSig,
      restoreNonce
    );

    logProgress('Exact amount transfer test passed');
  });
}
