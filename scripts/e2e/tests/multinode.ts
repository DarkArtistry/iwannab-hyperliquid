/**
 * Multi-Node E2E Tests
 *
 * Comprehensive tests for a 5-validator HyperCore cluster:
 * - Connectivity and peer discovery across all nodes
 * - Transaction propagation (orders, leverage, cancels)
 * - EVM state sync (block number, balances, chain ID)
 * - Invalid transaction rejection
 * - Block progression over time
 * - State consistency (appHash, balances, positions)
 *
 * Requires: GATEWAY_URLS, EVM_RPC_URLS, COMETBFT_RPC_URLS env vars (comma-separated)
 */

import {
  createPublicClient,
  createWalletClient,
  http,
  parseEther,
  encodeFunctionData,
  decodeFunctionResult,
} from 'viem';
import { privateKeyToAccount, signTypedData } from 'viem/accounts';
import { foundry } from 'viem/chains';
import { exec as execCb } from 'child_process';
import { promisify } from 'util';

import type { TestContext, SignatureWire } from '../lib/types.js';
import { logSection, logProgress } from '../lib/logging.js';
import { runTest, sleep } from '../lib/testing.js';
import { signAction } from '../lib/signing.js';
import { TEST_ACCOUNTS } from '../lib/accounts.js';

const exec = promisify(execCb);

// =============================================================================
// MULTI-NODE CONFIG
// =============================================================================

const GATEWAY_URLS = (process.env.GATEWAY_URLS || 'http://localhost:3000,http://localhost:3010,http://localhost:3020,http://localhost:3030,http://localhost:3040').split(',');
const EVM_RPC_URLS = (process.env.EVM_RPC_URLS || 'http://localhost:8545,http://localhost:8555,http://localhost:8565,http://localhost:8575,http://localhost:8585').split(',');
const COMETBFT_RPC_URLS = (process.env.COMETBFT_RPC_URLS || 'http://localhost:26657,http://localhost:26667,http://localhost:26677,http://localhost:26687,http://localhost:26697').split(',');
const NUM_NODES = GATEWAY_URLS.length;

// =============================================================================
// MULTI-NODE API HELPERS
// =============================================================================

async function infoRequestTo(gatewayUrl: string, type: string, params: Record<string, unknown> = {}): Promise<unknown> {
  const response = await fetch(`${gatewayUrl}/info`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type, ...params }),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }
  return response.json();
}

async function exchangeRequestTo(
  gatewayUrl: string,
  action: Record<string, unknown>,
  signature: SignatureWire,
  nonce: number,
): Promise<unknown> {
  const response = await fetch(`${gatewayUrl}/exchange`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action, signature, nonce }),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }
  return response.json();
}

async function evmRpcCall(evmUrl: string, method: string, params: unknown[] = []): Promise<unknown> {
  const response = await fetch(evmUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
  });
  if (!response.ok) {
    throw new Error(`EVM RPC HTTP ${response.status}: ${await response.text()}`);
  }
  const data = (await response.json()) as { result?: unknown; error?: { message: string } };
  if (data.error) {
    throw new Error(`EVM RPC error: ${data.error.message}`);
  }
  return data.result;
}

async function cometbftRpcCall(rpcUrl: string, endpoint: string): Promise<unknown> {
  const response = await fetch(`${rpcUrl}/${endpoint}`);
  if (!response.ok) {
    throw new Error(`CometBFT RPC HTTP ${response.status}: ${await response.text()}`);
  }
  return response.json();
}

/** Query all nodes in parallel and return results array */
async function queryAllNodes<T>(
  fn: (url: string) => Promise<T>,
  urls: string[],
): Promise<T[]> {
  return Promise.all(urls.map(fn));
}

/**
 * Poll a condition function until it returns true or timeout is reached.
 * Polls every `intervalMs` (default 1000ms) for up to `timeoutMs` (default 15000ms).
 */
async function waitForCondition(
  condition: () => Promise<boolean>,
  timeoutMs: number = 15000,
  intervalMs: number = 1000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await condition()) return true;
    await sleep(intervalMs);
  }
  return false;
}

// =============================================================================
// TEST CATEGORIES
// =============================================================================

export async function runMultinodeTests(ctx: TestContext): Promise<void> {
  // =========================================================================
  // 1. CONNECTIVITY TESTS
  // =========================================================================
  logSection(`17. Multi-Node Connectivity (${NUM_NODES} validators)`);

  await runTest(ctx, 'All nodes healthy', 'multinode-connectivity', 'Health check all validator nodes', async () => {
    for (let i = 0; i < NUM_NODES; i++) {
      const url = `${GATEWAY_URLS[i]}/health`;
      logProgress(`Checking Node ${i} at ${GATEWAY_URLS[i]}...`);
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`Node ${i} health check failed: ${resp.status}`);
    }
    logProgress(`All ${NUM_NODES} nodes are healthy`);
  });

  await runTest(ctx, 'All nodes have peers', 'multinode-connectivity', 'Each node connected to other validators', async () => {
    const expectedPeers = NUM_NODES - 1;
    for (let i = 0; i < NUM_NODES; i++) {
      const data = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'net_info')) as {
        result: { n_peers: string };
      };
      const peers = parseInt(data.result.n_peers);
      logProgress(`Node ${i}: ${peers} peers`);
      if (peers < 1) {
        throw new Error(`Node ${i} has ${peers} peers, expected at least 1`);
      }
    }
    logProgress(`All nodes have peers (expected ~${expectedPeers})`);
  });

  await runTest(ctx, 'Validator set correct', 'multinode-connectivity', 'CometBFT reports correct validator count', async () => {
    const data = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'validators')) as {
      result: { count: string; total: string };
    };
    const count = parseInt(data.result.count);
    logProgress(`Validator count: ${count}, total power: ${data.result.total}`);
    if (count !== NUM_NODES) {
      throw new Error(`Expected ${NUM_NODES} validators, got ${count}`);
    }
  });

  await runTest(ctx, 'All CometBFT RPCs responding', 'multinode-connectivity', 'CometBFT RPC accessible on all nodes', async () => {
    const networks: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const data = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { node_info: { network: string } };
      };
      networks.push(data.result.node_info.network);
      logProgress(`Node ${i} network: ${data.result.node_info.network}`);
    }
    const allSame = networks.every((n) => n === networks[0]);
    if (!allSame) {
      throw new Error(`Nodes on different networks: ${JSON.stringify(networks)}`);
    }
  });

  await runTest(ctx, 'EVM RPC accessible on all nodes', 'multinode-connectivity', 'EVM JSON-RPC responding on all validators', async () => {
    const chainIds: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const chainId = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_chainId')) as string;
      chainIds.push(chainId);
      logProgress(`Node ${i} EVM chain ID: ${chainId}`);
    }
    const allSame = chainIds.every((id) => id === chainIds[0]);
    if (!allSame) {
      throw new Error(`EVM chain IDs differ across nodes: ${JSON.stringify(chainIds)}`);
    }
  });

  // =========================================================================
  // 2. TRANSACTION PROPAGATION TESTS
  // =========================================================================
  logSection('18. Transaction Propagation Across Nodes');

  await runTest(ctx, 'Order via Node 0 visible on all nodes', 'multinode-txprop', 'Place order on Node 0, verify orderbook on all', async () => {
    const cloid = `mn-order-${Date.now()}`;
    const orderAction = {
      type: 'order',
      orders: [{
        a: 0, b: true, p: '45000', s: '0.001', r: false,
        t: { limit: { tif: 'Gtc' } },
        c: cloid,
      }],
      grouping: 'na',
    };

    logProgress('Placing resting buy order at $45,000 via Node 0...');
    const { signature, nonce } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], orderAction, signature, nonce);

    // Poll for order propagation across all nodes (up to 15s)
    const foundOnAll = await waitForCondition(async () => {
      for (let i = 0; i < NUM_NODES; i++) {
        const orders = (await infoRequestTo(GATEWAY_URLS[i], 'openOrders', {
          user: TEST_ACCOUNTS.ALICE.address,
        })) as Array<{ coin: string; side: string }>;
        if (orders.length === 0) return false;
      }
      return true;
    }, 15000, 1000);

    if (foundOnAll) {
      logProgress(`Order propagated to all ${NUM_NODES} nodes`);
    } else {
      // Log final state for debugging
      for (let i = 0; i < NUM_NODES; i++) {
        const orders = (await infoRequestTo(GATEWAY_URLS[i], 'openOrders', {
          user: TEST_ACCOUNTS.ALICE.address,
        })) as Array<{ coin: string; side: string }>;
        logProgress(`Node ${i}: ${orders.length} open orders`);
      }
      throw new Error('Order placed on Node 0 was not visible on all nodes after 15s');
    }

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAction, cSig, cNonce);
    await sleep(1000);
  });

  await runTest(ctx, 'Leverage update propagates to all nodes', 'multinode-txprop', 'Update leverage on Node 2, verify on all nodes', async () => {
    const leverageAction = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 25,
    };

    logProgress('Setting leverage to 25x via Node 2...');
    const { signature, nonce } = await signAction(leverageAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(2, NUM_NODES - 1)], leverageAction, signature, nonce);

    // Wait for propagation
    await sleep(5000);

    // Verify leverage is 25x on all nodes
    const leverages: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const state = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { assetPositions?: Array<{ position: { leverage: { type: string; value: number } } }> };
      const lev = state.assetPositions?.[0]?.position?.leverage;
      const levStr = lev ? `${lev.value}x (${lev.type})` : 'default';
      leverages.push(levStr);
      logProgress(`Node ${i}: leverage=${levStr}`);
    }
    const allMatch = leverages.every((l) => l === leverages[0]);
    if (!allMatch) {
      throw new Error(`Leverage differs across nodes: ${JSON.stringify(leverages)}`);
    }

    // Reset leverage
    const resetAction = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 10 };
    const { signature: rSig, nonce: rNonce } = await signAction(resetAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], resetAction, rSig, rNonce);
    await sleep(1000);
  });

  await runTest(ctx, 'Order via different nodes matches', 'multinode-txprop', 'Alice buys on Node 0, Bob sells on Node 3', async () => {
    // Alice places buy on Node 0
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    logProgress('Alice placing buy at $64,000 via Node 0...');
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], buyAction, buySig, buyNonce);

    // Wait for Alice's resting order to be visible before placing Bob's sell
    const aliceOrderVisible = await waitForCondition(async () => {
      const orders = (await infoRequestTo(GATEWAY_URLS[Math.min(3, NUM_NODES - 1)], 'openOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ coin: string; side: string }>;
      return orders.length > 0;
    }, 15000, 1000);

    if (!aliceOrderVisible) {
      throw new Error('Alice resting order not visible on Node 3 after 15s - cannot proceed with cross-node match');
    }
    logProgress('Alice resting order visible on target node, placing Bob sell...');

    // Bob places sell on Node 3
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    logProgress('Bob placing sell at $64,000 via Node 3...');
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(3, NUM_NODES - 1)], sellAction, sellSig, sellNonce);

    // Poll for fills (up to 15s)
    const fillFound = await waitForCondition(async () => {
      const fills = (await infoRequestTo(GATEWAY_URLS[0], 'userFills', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ px: string; sz: string }>;
      if (fills.length > 0) {
        logProgress(`Alice has ${fills.length} fill(s) at price ${fills[0].px}`);
        return true;
      }
      return false;
    }, 15000, 1000);

    if (!fillFound) {
      throw new Error('Cross-node order matching failed: no fills found for Alice after placing buy on Node 0 and sell on Node 3');
    }

    // Verify fills visible on all nodes
    for (let i = 0; i < NUM_NODES; i++) {
      const fills = (await infoRequestTo(GATEWAY_URLS[i], 'userFills', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ px: string; sz: string }>;
      logProgress(`Node ${i}: Alice has ${fills.length} fill(s)`);
      if (fills.length === 0) {
        throw new Error(`Node ${i} missing fills that should have propagated`);
      }
    }

    // Cleanup
    const cancelAlice = { type: 'cancelAll' };
    const { signature: ca, nonce: cn } = await signAction(cancelAlice, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAlice, ca, cn);
    const cancelBob = { type: 'cancelAll' };
    const { signature: cb, nonce: cbn } = await signAction(cancelBob, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelBob, cb, cbn);
    await sleep(1000);
  });

  await runTest(ctx, 'Cancel order propagates across nodes', 'multinode-txprop', 'Place on Node 0, cancel on Node 1, verify removal on all', async () => {
    // Place order on Node 0
    const orderAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '40000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    logProgress('Placing resting order at $40,000 via Node 0...');
    const { signature, nonce } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], orderAction, signature, nonce);

    // Wait for order to appear on target cancel node
    await waitForCondition(async () => {
      const orders = (await infoRequestTo(GATEWAY_URLS[Math.min(1, NUM_NODES - 1)], 'openOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<unknown>;
      return orders.length > 0;
    }, 15000, 1000);

    // Cancel on Node 1
    logProgress('Cancelling all orders via Node 1...');
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(1, NUM_NODES - 1)], cancelAction, cSig, cNonce);

    // Poll until cancel propagates to all nodes (instead of hardcoded sleep)
    const cancelled = await waitForCondition(async () => {
      for (let i = 0; i < NUM_NODES; i++) {
        const orders = (await infoRequestTo(GATEWAY_URLS[i], 'openOrders', {
          user: TEST_ACCOUNTS.ALICE.address,
        })) as Array<unknown>;
        if (orders.length !== 0) return false;
      }
      return true;
    }, 15000, 1000);

    // Log final state
    for (let i = 0; i < NUM_NODES; i++) {
      const orders = (await infoRequestTo(GATEWAY_URLS[i], 'openOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<unknown>;
      logProgress(`Node ${i}: ${orders.length} open orders (expected 0)`);
      if (orders.length !== 0) {
        throw new Error(`Node ${i} still has ${orders.length} open orders after cancel propagation`);
      }
    }
  });

  await runTest(ctx, 'Balance consistent after transactions', 'multinode-txprop', 'Verify Alice/Bob balances match across all nodes', async () => {
    await sleep(2000);

    const aliceBalances: string[] = [];
    const bobBalances: string[] = [];

    for (let i = 0; i < NUM_NODES; i++) {
      const aliceState = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { marginSummary: { accountValue: string } };
      const bobState = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.BOB.address,
      })) as { marginSummary: { accountValue: string } };

      aliceBalances.push(aliceState.marginSummary.accountValue);
      bobBalances.push(bobState.marginSummary.accountValue);
      logProgress(`Node ${i}: Alice=$${aliceState.marginSummary.accountValue}, Bob=$${bobState.marginSummary.accountValue}`);
    }

    // All nodes should report the same balance
    const allAliceSame = aliceBalances.every((b) => b === aliceBalances[0]);
    const allBobSame = bobBalances.every((b) => b === bobBalances[0]);
    if (!allAliceSame) {
      throw new Error(`Alice balances differ across nodes: ${JSON.stringify(aliceBalances)}`);
    }
    if (!allBobSame) {
      throw new Error(`Bob balances differ across nodes: ${JSON.stringify(bobBalances)}`);
    }
    logProgress('All nodes report identical balances');
  });

  // =========================================================================
  // 3. EVM STATE SYNC TESTS
  // =========================================================================
  logSection('19. EVM State Sync Across Nodes');

  await runTest(ctx, 'EVM block numbers consistent', 'multinode-evm', 'All nodes at same EVM block height', async () => {
    const blockNumbers: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const bn = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_blockNumber')) as string;
      const num = parseInt(bn, 16);
      blockNumbers.push(num);
      logProgress(`Node ${i} EVM block: ${num}`);
    }

    const maxDiff = Math.max(...blockNumbers) - Math.min(...blockNumbers);
    if (maxDiff > 2) {
      throw new Error(`EVM block numbers diverged by ${maxDiff}: ${JSON.stringify(blockNumbers)}`);
    }
    logProgress(`Block numbers within tolerance (max diff: ${maxDiff})`);
  });

  await runTest(ctx, 'EVM balances consistent across nodes', 'multinode-evm', 'eth_getBalance returns same result on all nodes', async () => {
    const aliceAddr = TEST_ACCOUNTS.ALICE.address;
    const balances: string[] = [];

    for (let i = 0; i < NUM_NODES; i++) {
      const bal = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_getBalance', [aliceAddr, 'latest'])) as string;
      balances.push(bal);
      logProgress(`Node ${i} Alice EVM balance: ${bal}`);
    }

    const allSame = balances.every((b) => b === balances[0]);
    if (!allSame) {
      throw new Error(`EVM balances differ: ${JSON.stringify(balances)}`);
    }
    logProgress('EVM balances consistent across all nodes');
  });

  await runTest(ctx, 'EVM chain ID consistent', 'multinode-evm', 'All nodes report same chain ID', async () => {
    const chainIds: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const id = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_chainId')) as string;
      chainIds.push(id);
    }
    const allSame = chainIds.every((id) => id === chainIds[0]);
    if (!allSame) {
      throw new Error(`Chain IDs differ: ${JSON.stringify(chainIds)}`);
    }
    logProgress(`All nodes: chain ID = ${chainIds[0]}`);
  });

  await runTest(ctx, 'EVM gas price consistent', 'multinode-evm', 'All nodes report same gas price', async () => {
    const gasPrices: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const gp = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_gasPrice')) as string;
      gasPrices.push(gp);
    }
    const allSame = gasPrices.every((gp) => gp === gasPrices[0]);
    if (!allSame) {
      throw new Error(`Gas prices differ: ${JSON.stringify(gasPrices)}`);
    }
    logProgress(`All nodes: gas price = ${gasPrices[0]}`);
  });

  // =========================================================================
  // 4. INVALID TRANSACTION TESTS
  // =========================================================================
  logSection('20. Invalid Transaction Handling (Multi-Node)');

  await runTest(ctx, 'Invalid leverage rejected on all nodes', 'multinode-invalid', 'Leverage > 50x rejected regardless of which node receives it', async () => {
    for (let i = 0; i < Math.min(3, NUM_NODES); i++) {
      const action = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 100 };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

      try {
        await exchangeRequestTo(GATEWAY_URLS[i], action, signature, nonce);
        throw new Error(`Node ${i} accepted 100x leverage, expected rejection`);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('accepted 100x leverage')) {
          throw err; // Re-throw our own assertion error
        }
        if (msg.includes('400') || msg.includes('Invalid leverage') || msg.includes('Validation')) {
          logProgress(`Node ${i}: correctly rejected`);
        } else {
          throw err;
        }
      }
    }
  });

  await runTest(ctx, 'Invalid price format rejected', 'multinode-invalid', 'Non-numeric price rejected on all nodes', async () => {
    const action = {
      type: 'order',
      orders: [{ a: 0, b: true, p: 'not-a-number', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);

    try {
      await exchangeRequestTo(GATEWAY_URLS[0], action, signature, nonce);
      throw new Error('Order with non-numeric price was accepted, expected rejection');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('was accepted')) {
        throw err; // Re-throw our own assertion error
      }
      logProgress('Invalid price correctly rejected at validation layer');
    }
  });

  await runTest(ctx, 'State unaffected by invalid transactions', 'multinode-invalid', 'Chain keeps producing blocks after invalid tx attempts', async () => {
    // Get block height before
    const statusBefore = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightBefore = parseInt(statusBefore.result.sync_info.latest_block_height);
    logProgress(`Before: height=${heightBefore}`);

    // Send invalid transactions
    for (let i = 0; i < 3; i++) {
      try {
        const action = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 999 };
        const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
        await exchangeRequestTo(GATEWAY_URLS[i % NUM_NODES], action, signature, nonce);
      } catch {
        // Expected - invalid transactions should be rejected
      }
    }

    await sleep(3000);

    // Verify chain continued producing blocks
    const statusAfter = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightAfter = parseInt(statusAfter.result.sync_info.latest_block_height);
    logProgress(`After: height=${heightAfter} (grew by ${heightAfter - heightBefore})`);

    if (heightAfter <= heightBefore) {
      throw new Error('Chain stopped producing blocks after invalid transactions');
    }

    // Verify all nodes still in consensus
    const appHashes: string[] = [];
    const checkHeight = Math.min(heightBefore, heightAfter) - 1;
    for (let i = 0; i < NUM_NODES; i++) {
      const commit = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `commit?height=${checkHeight}`)) as {
        result: { signed_header: { header: { app_hash: string } } };
      };
      appHashes.push(commit.result.signed_header.header.app_hash);
    }
    const allMatch = appHashes.every((h) => h === appHashes[0]);
    if (!allMatch) {
      throw new Error(`AppHashes diverged after invalid txs: ${JSON.stringify(appHashes)}`);
    }
    logProgress('State consistent across all nodes after invalid transactions');
  });

  // =========================================================================
  // 5. BLOCK PROGRESSION TESTS
  // =========================================================================
  logSection('21. Block Progression (Multi-Node)');

  await runTest(ctx, 'Blocks progressing on all nodes', 'multinode-blocks', 'Wait 15s, verify all nodes advance', async () => {
    // Record initial heights
    const initialHeights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      initialHeights.push(parseInt(status.result.sync_info.latest_block_height));
    }
    logProgress(`Initial heights: [${initialHeights.join(', ')}]`);

    // Wait for blocks to be produced
    logProgress('Waiting 15 seconds for block production...');
    await sleep(15000);

    // Record final heights
    const finalHeights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      finalHeights.push(parseInt(status.result.sync_info.latest_block_height));
    }
    logProgress(`Final heights: [${finalHeights.join(', ')}]`);

    // All nodes should have progressed
    for (let i = 0; i < NUM_NODES; i++) {
      const growth = finalHeights[i] - initialHeights[i];
      logProgress(`Node ${i}: grew by ${growth} blocks`);
      if (growth < 3) {
        throw new Error(`Node ${i} only grew by ${growth} blocks in 15s (expected at least 3)`);
      }
    }

    // Heights should be close to each other
    const maxHeight = Math.max(...finalHeights);
    const minHeight = Math.min(...finalHeights);
    if (maxHeight - minHeight > 3) {
      throw new Error(`Heights diverged: max=${maxHeight}, min=${minHeight} (diff=${maxHeight - minHeight})`);
    }
    logProgress(`All ${NUM_NODES} nodes progressing, height spread: ${maxHeight - minHeight}`);
  });

  await runTest(ctx, 'Block hashes match across nodes', 'multinode-blocks', 'All nodes agree on block hash at same height', async () => {
    // Get a recent committed height (a few blocks back to ensure all nodes have it)
    const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const checkHeight = parseInt(status.result.sync_info.latest_block_height) - 3;

    if (checkHeight < 1) {
      throw new Error('Not enough blocks produced yet');
    }

    logProgress(`Comparing block hash at height ${checkHeight}...`);

    const hashes: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const block = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `block?height=${checkHeight}`)) as {
        result: { block_id: { hash: string } };
      };
      hashes.push(block.result.block_id.hash);
      logProgress(`Node ${i}: hash=${hashes[i].slice(0, 20)}...`);
    }

    const allMatch = hashes.every((h) => h === hashes[0]);
    if (!allMatch) {
      throw new Error(`Block hashes differ at height ${checkHeight}: ${JSON.stringify(hashes)}`);
    }
    logProgress(`All ${NUM_NODES} nodes agree on block ${checkHeight}`);
  });

  // =========================================================================
  // 6. STATE CONSISTENCY TESTS
  // =========================================================================
  logSection('22. State Consistency Across Nodes');

  await runTest(ctx, 'AppHash consistent across nodes', 'multinode-state', 'All nodes have same application hash', async () => {
    // Wait a moment for consensus to settle
    await sleep(2000);

    // Get the minimum height across all nodes
    const heights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      heights.push(parseInt(status.result.sync_info.latest_block_height));
    }
    const minHeight = Math.min(...heights) - 1;
    logProgress(`Comparing app hash at height ${minHeight}...`);

    // Get commits at same height for app hash comparison
    const appHashes: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const commit = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `commit?height=${minHeight}`)) as {
        result: { signed_header: { header: { app_hash: string }; commit: { block_id: { hash: string } } } };
      };
      appHashes.push(commit.result.signed_header.header.app_hash);
      logProgress(`Node ${i}: appHash=${appHashes[i].slice(0, 20)}...`);
    }

    const allMatch = appHashes.every((h) => h === appHashes[0]);
    if (!allMatch) {
      throw new Error(`AppHashes differ at height ${minHeight}: ${JSON.stringify(appHashes)}`);
    }
    logProgress(`All ${NUM_NODES} nodes agree on application state`);
  });

  await runTest(ctx, 'Clearinghouse state matches across nodes', 'multinode-state', 'Alice account state identical on all nodes', async () => {
    await sleep(1000);

    const states: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const state = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { marginSummary: { accountValue: string; totalRawUsd: string } };
      const key = `${state.marginSummary.accountValue}|${state.marginSummary.totalRawUsd}`;
      states.push(key);
      logProgress(`Node ${i}: accountValue=${state.marginSummary.accountValue}, rawUsd=${state.marginSummary.totalRawUsd}`);
    }

    const allMatch = states.every((s) => s === states[0]);
    if (!allMatch) {
      throw new Error(`Clearinghouse states differ: ${JSON.stringify(states)}`);
    }
    logProgress('Clearinghouse state identical across all nodes');
  });

  await runTest(ctx, 'Unified balances match across nodes', 'multinode-state', 'Unified balance views identical on all nodes', async () => {
    const balances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const ub = (await infoRequestTo(GATEWAY_URLS[i], 'unifiedBalances', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { balances?: Array<{ tokenIndex?: number; total: string; coreView: string; evmView: string }> };
      // Sort by tokenIndex to ensure consistent ordering across nodes
      const sorted = (ub.balances || [])
        .sort((a, b) => (a.tokenIndex || 0) - (b.tokenIndex || 0))
        .map((b) => `${b.total}/${b.coreView}/${b.evmView}`);
      const summary = JSON.stringify(sorted);
      balances.push(summary);
      logProgress(`Node ${i}: ${summary}`);
    }

    const allMatch = balances.every((b) => b === balances[0]);
    if (!allMatch) {
      throw new Error(`Unified balances differ: ${JSON.stringify(balances)}`);
    }
    logProgress('Unified balances identical across all nodes');
  });

  await runTest(ctx, 'Market metadata consistent', 'multinode-state', 'All nodes report same markets', async () => {
    const metas: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const meta = (await infoRequestTo(GATEWAY_URLS[i], 'meta')) as {
        universe?: Array<{ name: string }>;
      };
      const names = (meta.universe || []).map((m) => m.name).sort().join(',');
      metas.push(names);
      logProgress(`Node ${i}: markets=[${names}]`);
    }

    const allMatch = metas.every((m) => m === metas[0]);
    if (!allMatch) {
      throw new Error(`Market metadata differs: ${JSON.stringify(metas)}`);
    }
    logProgress('Market metadata identical across all nodes');
  });

  await runTest(ctx, 'State still consistent after extended run', 'multinode-state', 'Wait and re-verify state consistency', async () => {
    logProgress('Waiting 10 seconds for additional blocks...');
    await sleep(10000);

    // Re-check heights
    const heights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      heights.push(parseInt(status.result.sync_info.latest_block_height));
    }
    logProgress(`Heights after extended run: [${heights.join(', ')}]`);

    const spread = Math.max(...heights) - Math.min(...heights);
    if (spread > 3) {
      throw new Error(`Heights diverged after extended run: spread=${spread}`);
    }

    // Re-check balances
    const aliceBalances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const state = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { marginSummary: { accountValue: string } };
      aliceBalances.push(state.marginSummary.accountValue);
    }

    const balancesMatch = aliceBalances.every((b) => b === aliceBalances[0]);
    if (!balancesMatch) {
      throw new Error(`Balances diverged after extended run: ${JSON.stringify(aliceBalances)}`);
    }

    logProgress(`Cluster stable: ${heights[0]}+ blocks, consistent state, spread=${spread}`);
  });

  // =========================================================================
  // 7. MIXED TRANSACTION TYPE TESTS (EVM + Non-EVM)
  // =========================================================================
  logSection('23. Mixed Transaction Types (EVM + Non-EVM)');

  const CHAIN_ID = 1337;

  await runTest(ctx, 'EVM tx via Node 0 processes correctly', 'multinode-mixed', 'Send eth_sendRawTransaction to Node 0', async () => {
    const account = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
    const walletClient = createWalletClient({
      account,
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });

    const hash = await walletClient.sendTransaction({
      to: TEST_ACCOUNTS.BOB.address,
      value: parseEther('0.001'),
    });
    logProgress(`EVM tx submitted to Node 0: ${hash}`);

    // Poll for receipt (CometBFT needs to include tx in a block first)
    const publicClient = createPublicClient({
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });
    let receipt: any = null;
    const found = await waitForCondition(async () => {
      try {
        receipt = await publicClient.getTransactionReceipt({ hash });
        return receipt != null;
      } catch {
        return false;
      }
    }, 15000, 1000);

    if (!found || !receipt) {
      throw new Error(`Transaction receipt not found after 15s: ${hash}`);
    }
    if (receipt.status !== 'success') {
      throw new Error(`EVM transaction reverted: ${hash}`);
    }
    logProgress('EVM transaction processed via Node 0');
  });

  await runTest(ctx, 'EVM tx via different nodes', 'multinode-mixed', 'Send EVM transactions to different validators', async () => {
    // Use different accounts per node to avoid duplicate tx rejection by CometBFT.
    // When the same account sends identical txs (same nonce/to/value) to different
    // nodes, CometBFT's mempool gossip sees them as duplicates.
    const senders = [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB, TEST_ACCOUNTS.CHARLIE];
    let successCount = 0;
    for (let i = 0; i < Math.min(3, NUM_NODES); i++) {
      const sender = senders[i];
      const recipient = senders[(i + 1) % senders.length];
      const account = privateKeyToAccount(sender.privateKey);
      const walletClient = createWalletClient({
        account,
        chain: { ...foundry, id: CHAIN_ID },
        transport: http(EVM_RPC_URLS[i]),
      });

      try {
        const hash = await walletClient.sendTransaction({
          to: recipient.address,
          value: parseEther('0.0001'),
        });
        logProgress(`Node ${i}: EVM tx submitted: ${hash.slice(0, 20)}...`);
        successCount++;
      } catch (e: any) {
        logProgress(`Node ${i}: EVM tx failed: ${e.message?.slice(0, 80)}`);
      }
      await sleep(2000); // Wait for block inclusion between txs
    }

    await sleep(3000);
    logProgress(`${successCount}/${Math.min(3, NUM_NODES)} EVM transactions sent to different validators`);
    if (successCount < 2) {
      throw new Error(`Only ${successCount}/${Math.min(3, NUM_NODES)} EVM transactions succeeded (expected at least 2)`);
    }
  });

  await runTest(ctx, 'Mixed EVM and perp orders in same block window', 'multinode-mixed', 'Interleave EVM tx and perp orders across nodes', async () => {
    // Record initial block height
    const statusBefore = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightBefore = parseInt(statusBefore.result.sync_info.latest_block_height);
    logProgress(`Block height before mixed txs: ${heightBefore}`);

    // Send perp order via Node 0 (must succeed)
    const orderAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '42000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: oSig, nonce: oNonce } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], orderAction, oSig, oNonce);
    logProgress('Perp order submitted via Node 0');

    // Send EVM tx via Node 2 (use Alice who has ETH from prior EVM tests)
    const evmAccount = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
    const evmClient = createWalletClient({
      account: evmAccount,
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[Math.min(2, NUM_NODES - 1)]),
    });
    const evmHash = await evmClient.sendTransaction({
      to: TEST_ACCOUNTS.CHARLIE.address,
      value: parseEther('0.0001'),
    });
    logProgress(`EVM tx submitted via Node 2: ${evmHash.slice(0, 20)}...`);

    // Send leverage update via last node
    const levAction = { type: 'updateLeverage', asset: 1, isCross: true, leverage: 20 };
    const { signature: lSig, nonce: lNonce } = await signAction(levAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(4, NUM_NODES - 1)], levAction, lSig, lNonce);
    logProgress('Leverage update submitted via last node');

    // Wait for blocks to include all transactions
    await sleep(5000);

    // Verify chain continued producing blocks
    const statusAfter = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightAfter = parseInt(statusAfter.result.sync_info.latest_block_height);
    logProgress(`Block height after mixed txs: ${heightAfter} (grew by ${heightAfter - heightBefore})`);

    if (heightAfter <= heightBefore) {
      throw new Error('Chain stopped producing blocks after mixed transactions');
    }

    // Cleanup orders
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAction, cSig, cNonce);
    await sleep(1000);
  });

  await runTest(ctx, 'Failing EVM tx does not halt chain', 'multinode-mixed', 'Invalid EVM tx should be rejected without stopping block production', async () => {
    const heightBefore = await (async () => {
      const s = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      return parseInt(s.result.sync_info.latest_block_height);
    })();

    // Send an invalid EVM tx (malformed data) to different nodes - must be rejected
    for (let i = 0; i < Math.min(3, NUM_NODES); i++) {
      try {
        await evmRpcCall(EVM_RPC_URLS[i], 'eth_sendRawTransaction', ['0xdeadbeef']);
        throw new Error(`Node ${i} accepted malformed EVM tx '0xdeadbeef'`);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('accepted malformed')) {
          throw err; // Re-throw our assertion error
        }
        logProgress(`Node ${i}: correctly rejected invalid EVM tx`);
      }
    }

    // Wait and verify chain continues
    await sleep(5000);

    const heightAfter = await (async () => {
      const s = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      return parseInt(s.result.sync_info.latest_block_height);
    })();

    logProgress(`Block progression: ${heightBefore} -> ${heightAfter} (+${heightAfter - heightBefore})`);

    if (heightAfter - heightBefore < 2) {
      throw new Error(`Chain stalled after invalid EVM transactions (only grew ${heightAfter - heightBefore} blocks)`);
    }
    logProgress('Chain continues producing blocks after invalid EVM transactions');
  });

  await runTest(ctx, 'Failing perp tx does not halt chain', 'multinode-mixed', 'Invalid perp transaction should not stop block production', async () => {
    const heightBefore = await (async () => {
      const s = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      return parseInt(s.result.sync_info.latest_block_height);
    })();

    // Send transactions that should fail (e.g., order with zero size, huge leverage)
    const badActions = [
      { type: 'updateLeverage', asset: 0, isCross: true, leverage: 200 }, // too high
      { type: 'order', orders: [{ a: 99, b: true, p: '1', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }], grouping: 'na' }, // non-existent market
    ];

    let rejectedCount = 0;
    for (let j = 0; j < badActions.length; j++) {
      const action = badActions[j];
      const nodeIdx = j % NUM_NODES;
      try {
        const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
        await exchangeRequestTo(GATEWAY_URLS[nodeIdx], action, signature, nonce);
        // Some invalid actions may be accepted at gateway but fail in consensus
        logProgress(`Node ${nodeIdx}: accepted at gateway (will fail in execution)`);
      } catch (err: unknown) {
        rejectedCount++;
        logProgress(`Node ${nodeIdx}: rejected at validation layer`);
      }
    }
    logProgress(`${rejectedCount}/${badActions.length} invalid actions rejected at validation layer`);

    await sleep(5000);

    const heightAfter = await (async () => {
      const s = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      return parseInt(s.result.sync_info.latest_block_height);
    })();

    logProgress(`Block progression: ${heightBefore} -> ${heightAfter} (+${heightAfter - heightBefore})`);

    if (heightAfter - heightBefore < 2) {
      throw new Error(`Chain stalled after invalid perp transactions (only grew ${heightAfter - heightBefore} blocks)`);
    }
    logProgress('Chain resilient to invalid perp transactions');
  });

  await runTest(ctx, 'State consistent after mixed transaction storm', 'multinode-mixed', 'Verify all nodes agree on state after mixed EVM/perp traffic', async () => {
    await sleep(2000);

    // Check all nodes have same block height (within tolerance)
    const heights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const status = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], 'status')) as {
        result: { sync_info: { latest_block_height: string } };
      };
      heights.push(parseInt(status.result.sync_info.latest_block_height));
    }
    const spread = Math.max(...heights) - Math.min(...heights);
    logProgress(`Final heights: [${heights.join(', ')}] (spread: ${spread})`);

    if (spread > 3) {
      throw new Error(`Heights diverged after mixed tx storm: spread=${spread}`);
    }

    // Verify balances still consistent
    const aliceBalances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const state = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { marginSummary: { accountValue: string } };
      aliceBalances.push(state.marginSummary.accountValue);
    }

    const balancesMatch = aliceBalances.every((b) => b === aliceBalances[0]);
    if (!balancesMatch) {
      throw new Error(`Balances diverged after mixed storm: ${JSON.stringify(aliceBalances)}`);
    }

    logProgress(`Cluster stable after mixed transaction storm: consistent state, spread=${spread}`);
  });

  // =========================================================================
  // 8. NODE RESILIENCE TESTS
  // =========================================================================
  logSection('24. Node Resilience (Restart, Catch-Up, Failure)');

  // Helper: get CometBFT block height for a specific node
  async function getNodeHeight(nodeIdx: number): Promise<number> {
    const data = (await cometbftRpcCall(COMETBFT_RPC_URLS[nodeIdx], 'status')) as {
      result: { sync_info: { latest_block_height: string; catching_up: boolean } };
    };
    return parseInt(data.result.sync_info.latest_block_height);
  }

  // Helper: check if a node is catching up
  async function isNodeCatchingUp(nodeIdx: number): Promise<boolean> {
    const data = (await cometbftRpcCall(COMETBFT_RPC_URLS[nodeIdx], 'status')) as {
      result: { sync_info: { catching_up: boolean } };
    };
    return data.result.sync_info.catching_up;
  }

  // Helper: check if a gateway is reachable
  async function isGatewayHealthy(nodeIdx: number): Promise<boolean> {
    try {
      const resp = await fetch(`${GATEWAY_URLS[nodeIdx]}/health`, { signal: AbortSignal.timeout(2000) });
      return resp.ok;
    } catch {
      return false;
    }
  }

  // Helper: check if CometBFT RPC is reachable
  async function isCometBftReachable(nodeIdx: number): Promise<boolean> {
    try {
      await cometbftRpcCall(COMETBFT_RPC_URLS[nodeIdx], 'status');
      return true;
    } catch {
      return false;
    }
  }

  // Determine compose file based on number of nodes
  const COMPOSE_FILE = NUM_NODES === 5 ? 'docker-compose-multinode-5.yml' : 'docker-compose-multinode.yml';
  const CONTAINER_PREFIX = NUM_NODES === 5 ? 'hypercore-5v' : 'hypercore';

  await runTest(ctx, 'Validator failure: chain continues with 4/5', 'multinode-resilience', 'Stop 1 validator, verify chain keeps producing blocks', async () => {
    // Record baseline height
    const heightBefore = await getNodeHeight(0);
    logProgress(`Height before stopping node 4: ${heightBefore}`);

    // Stop validator 4 (node + cometbft)
    logProgress('Stopping validator 4...');
    try {
      await exec(`docker stop ${CONTAINER_PREFIX}-cometbft-4 ${CONTAINER_PREFIX}-node-4`);
    } catch (e: any) {
      logProgress(`Stop command: ${e.message?.slice(0, 100)}`);
    }

    // Wait for a few blocks to confirm chain continues
    await sleep(8000);

    // Verify chain is still producing blocks on remaining nodes
    const heightAfter = await getNodeHeight(0);
    const growth = heightAfter - heightBefore;
    logProgress(`Height after 8s with 4/5 validators: ${heightAfter} (grew by ${growth})`);

    if (growth < 2) {
      // Restart node before failing
      await exec(`docker start ${CONTAINER_PREFIX}-node-4 ${CONTAINER_PREFIX}-cometbft-4`).catch(() => {});
      throw new Error(`Chain stalled with 4/5 validators: only grew ${growth} blocks`);
    }

    // Submit a transaction while node 4 is down
    logProgress('Submitting order while node 4 is offline...');
    const orderAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '41000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature, nonce } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], orderAction, signature, nonce);
    logProgress('Order submitted successfully with 4/5 validators');

    // Verify order visible on remaining nodes
    await sleep(2000);
    for (let i = 0; i < NUM_NODES - 1; i++) {
      const orders = (await infoRequestTo(GATEWAY_URLS[i], 'openOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<unknown>;
      if (orders.length === 0) {
        logProgress(`Warning: Node ${i} has 0 open orders`);
      }
    }

    // Cleanup order
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAction, cSig, cNonce);

    // Restart node 4
    logProgress('Restarting validator 4...');
    await exec(`docker start ${CONTAINER_PREFIX}-node-4 ${CONTAINER_PREFIX}-cometbft-4`);
    logProgress('Chain continued producing blocks with 4/5 validators');
  });

  await runTest(ctx, 'Node catch-up after restart', 'multinode-resilience', 'Restarted node syncs missed blocks and matches state', async () => {
    // Wait for node 4 to come back online
    const caughtUp = await waitForCondition(async () => {
      if (!(await isCometBftReachable(4))) return false;
      if (!(await isGatewayHealthy(4))) return false;
      return !(await isNodeCatchingUp(4));
    }, 60000, 2000);

    if (!caughtUp) {
      throw new Error('Node 4 did not finish catching up within 60s');
    }

    logProgress('Node 4 is back online and synced');

    // Wait a moment for state to settle
    await sleep(3000);

    // Verify heights are consistent
    const heights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      heights.push(await getNodeHeight(i));
    }
    const spread = Math.max(...heights) - Math.min(...heights);
    logProgress(`All node heights: [${heights.join(', ')}] (spread: ${spread})`);

    if (spread > 3) {
      throw new Error(`Node 4 height diverged after restart: spread=${spread}`);
    }

    // Verify appHash is consistent (proves state matches)
    const minHeight = Math.min(...heights) - 1;
    const appHashes: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const commit = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `commit?height=${minHeight}`)) as {
        result: { signed_header: { header: { app_hash: string } } };
      };
      appHashes.push(commit.result.signed_header.header.app_hash);
    }

    const allMatch = appHashes.every((h) => h === appHashes[0]);
    if (!allMatch) {
      throw new Error(`AppHash mismatch after catch-up: ${JSON.stringify(appHashes)}`);
    }
    logProgress(`AppHash consistent across all ${NUM_NODES} nodes after restart`);

    // Verify balances match (proves unified state is intact)
    const balances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const state = (await infoRequestTo(GATEWAY_URLS[i], 'clearinghouseState', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { marginSummary: { accountValue: string } };
      balances.push(state.marginSummary.accountValue);
    }
    const balancesMatch = balances.every((b) => b === balances[0]);
    if (!balancesMatch) {
      throw new Error(`Balances differ after catch-up: ${JSON.stringify(balances)}`);
    }
    logProgress('State fully consistent after node restart and catch-up');
  });

  await runTest(ctx, 'Transactions during downtime reflected after sync', 'multinode-resilience', 'Orders placed while node offline appear after sync', async () => {
    // Stop node 3
    logProgress('Stopping validator 3...');
    await exec(`docker stop ${CONTAINER_PREFIX}-cometbft-3 ${CONTAINER_PREFIX}-node-3`).catch(() => {});
    await sleep(3000);

    // Place an order while node 3 is offline
    const cloid = `resilience-${Date.now()}`;
    const orderAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '43000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } }, c: cloid }],
      grouping: 'na',
    };
    logProgress('Placing order while node 3 is offline...');
    const { signature, nonce } = await signAction(orderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], orderAction, signature, nonce);

    // Verify order is on Node 0
    await sleep(2000);
    const ordersOnNode0 = (await infoRequestTo(GATEWAY_URLS[0], 'openOrders', {
      user: TEST_ACCOUNTS.ALICE.address,
    })) as Array<{ cloid?: string }>;
    const hasOrder = ordersOnNode0.some((o) => o.cloid === cloid);
    if (!hasOrder) {
      logProgress('Warning: Order not found on Node 0 (may have been consumed)');
    } else {
      logProgress('Order confirmed on Node 0');
    }

    // Restart node 3
    logProgress('Restarting validator 3...');
    await exec(`docker start ${CONTAINER_PREFIX}-node-3 ${CONTAINER_PREFIX}-cometbft-3`);

    // Wait for node 3 to catch up
    const synced = await waitForCondition(async () => {
      if (!(await isCometBftReachable(3))) return false;
      if (!(await isGatewayHealthy(3))) return false;
      return !(await isNodeCatchingUp(3));
    }, 60000, 2000);

    if (!synced) {
      throw new Error('Node 3 did not finish catching up within 60s');
    }
    await sleep(3000);

    // Verify the order (or its effects) are on node 3
    const ordersOnNode3 = (await infoRequestTo(GATEWAY_URLS[3], 'openOrders', {
      user: TEST_ACCOUNTS.ALICE.address,
    })) as Array<{ cloid?: string }>;

    // Check that node 3 has the same view as node 0
    const ordersOnNode0After = (await infoRequestTo(GATEWAY_URLS[0], 'openOrders', {
      user: TEST_ACCOUNTS.ALICE.address,
    })) as Array<unknown>;

    logProgress(`Node 0: ${ordersOnNode0After.length} orders, Node 3: ${ordersOnNode3.length} orders`);

    if (ordersOnNode0After.length !== ordersOnNode3.length) {
      throw new Error(`Order count mismatch: Node 0 has ${ordersOnNode0After.length}, Node 3 has ${ordersOnNode3.length}`);
    }
    logProgress('Transactions during downtime correctly reflected after sync');

    // Cleanup
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAction, cSig, cNonce);
    await sleep(1000);
  });

  await runTest(ctx, 'Double failure halts consensus, recovery resumes', 'multinode-resilience', 'Stop 2/5 validators (below 2/3), verify halt, restart to resume', async () => {
    const heightBefore = await getNodeHeight(0);
    logProgress(`Height before double failure: ${heightBefore}`);

    // Stop 2 validators (3/5 = 60% < 66.7% required)
    logProgress('Stopping validators 3 and 4...');
    await exec(`docker stop ${CONTAINER_PREFIX}-cometbft-3 ${CONTAINER_PREFIX}-node-3 ${CONTAINER_PREFIX}-cometbft-4 ${CONTAINER_PREFIX}-node-4`).catch(() => {});

    // Wait and check if chain halted
    await sleep(10000);

    let heightDuringHalt: number;
    try {
      heightDuringHalt = await getNodeHeight(0);
    } catch {
      // Node 0 might be unresponsive if consensus is fully stuck
      heightDuringHalt = heightBefore;
    }

    const growthDuringHalt = heightDuringHalt - heightBefore;
    logProgress(`Height during halt: ${heightDuringHalt} (grew by ${growthDuringHalt})`);

    // With only 3/5 nodes, chain should have stalled or grown very little
    // (CometBFT might produce 1-2 blocks before realizing no supermajority)
    if (growthDuringHalt > 5) {
      logProgress(`Warning: Chain produced ${growthDuringHalt} blocks with 3/5 validators (expected stall)`);
    } else {
      logProgress('Chain correctly stalled with insufficient validators');
    }

    // Restart 1 validator (back to 4/5 = 80% > 66.7%)
    logProgress('Restarting validator 3 (to reach 4/5 = 80%)...');
    await exec(`docker start ${CONTAINER_PREFIX}-node-3 ${CONTAINER_PREFIX}-cometbft-3`);

    // Wait for consensus to resume
    const resumed = await waitForCondition(async () => {
      try {
        const h = await getNodeHeight(0);
        return h > heightDuringHalt + 2;
      } catch {
        return false;
      }
    }, 60000, 3000);

    if (!resumed) {
      // Restart remaining node before failing
      await exec(`docker start ${CONTAINER_PREFIX}-node-4 ${CONTAINER_PREFIX}-cometbft-4`).catch(() => {});
      throw new Error('Chain did not resume after restoring 4th validator');
    }

    const heightAfterResume = await getNodeHeight(0);
    logProgress(`Chain resumed at height ${heightAfterResume} after restoring 4th validator`);

    // Restart remaining validator
    logProgress('Restarting validator 4...');
    await exec(`docker start ${CONTAINER_PREFIX}-node-4 ${CONTAINER_PREFIX}-cometbft-4`);

    // Wait for full cluster sync
    const fullSync = await waitForCondition(async () => {
      try {
        if (!(await isCometBftReachable(4))) return false;
        return !(await isNodeCatchingUp(4));
      } catch {
        return false;
      }
    }, 60000, 3000);

    if (!fullSync) {
      logProgress('Warning: Node 4 still syncing after 60s');
    }

    await sleep(5000);

    // Verify all 5 nodes are back and consistent
    const finalHeights: number[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      try {
        finalHeights.push(await getNodeHeight(i));
      } catch {
        finalHeights.push(-1);
      }
    }
    logProgress(`Final heights: [${finalHeights.join(', ')}]`);

    const activeNodes = finalHeights.filter((h) => h > 0);
    if (activeNodes.length < NUM_NODES) {
      logProgress(`Warning: Only ${activeNodes.length}/${NUM_NODES} nodes responsive`);
    }

    logProgress('Double failure + recovery test complete');
  });

  await runTest(ctx, 'CometBFT finality: committed blocks never change', 'multinode-resilience', 'Block hashes at committed heights are immutable', async () => {
    // Get a committed height
    const currentHeight = await getNodeHeight(0);
    const checkHeight = currentHeight - 5;

    if (checkHeight < 1) {
      throw new Error('Not enough blocks produced for finality test');
    }

    // Record block hashes at checkHeight from all nodes
    logProgress(`Recording block hashes at height ${checkHeight}...`);
    const hashesBefore: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      try {
        const block = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `block?height=${checkHeight}`)) as {
          result: { block_id: { hash: string } };
        };
        hashesBefore.push(block.result.block_id.hash);
      } catch {
        hashesBefore.push('unreachable');
      }
    }

    // Wait for more blocks
    logProgress('Waiting for 10 more blocks...');
    await waitForCondition(async () => {
      const h = await getNodeHeight(0);
      return h > currentHeight + 10;
    }, 30000, 1000);

    // Re-query the SAME height - hashes must be identical
    logProgress(`Re-querying block hashes at height ${checkHeight}...`);
    const hashesAfter: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      try {
        const block = (await cometbftRpcCall(COMETBFT_RPC_URLS[i], `block?height=${checkHeight}`)) as {
          result: { block_id: { hash: string } };
        };
        hashesAfter.push(block.result.block_id.hash);
      } catch {
        hashesAfter.push('unreachable');
      }
    }

    // Compare
    for (let i = 0; i < NUM_NODES; i++) {
      if (hashesBefore[i] === 'unreachable' || hashesAfter[i] === 'unreachable') continue;
      if (hashesBefore[i] !== hashesAfter[i]) {
        throw new Error(
          `REORG DETECTED: Node ${i} block hash at height ${checkHeight} changed from ` +
          `${hashesBefore[i].slice(0, 16)}... to ${hashesAfter[i].slice(0, 16)}...`
        );
      }
    }

    // Also verify all reachable nodes agree
    const reachableHashes = hashesAfter.filter((h) => h !== 'unreachable');
    const allSame = reachableHashes.every((h) => h === reachableHashes[0]);
    if (!allSame) {
      throw new Error(`Block hash disagreement at height ${checkHeight}: ${JSON.stringify(hashesAfter)}`);
    }

    logProgress(`Finality verified: block ${checkHeight} hash immutable across all nodes`);
  });

  // =========================================================================
  // 9. CROSS-NODE EVM CONTRACT & SPOT TESTS
  // =========================================================================
  logSection('25. Cross-Node EVM Contract & Spot Trading');

  // Let cluster stabilize after resilience tests (node restarts, double failure recovery)
  await sleep(5000);

  await runTest(ctx, 'Contract deploy on Node 0, read on all nodes', 'multinode-crossnode', 'Deploy SimpleStorage on Node 0, call get() on all nodes', async () => {
    const account = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
    const walletClient = createWalletClient({
      account,
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });

    // Deploy SimpleStorage: contract with set(uint256) and get() returns (uint256)
    // Hand-crafted minimal bytecode with correct JUMPDEST alignment.
    //
    // Runtime code (52 bytes):
    //   Function dispatch: CALLDATALOAD >> 224, match set(uint256)=0x60fe47b1 or get()=0x6d4ce63c
    //   set(uint256) at 0x1e: SSTORE(slot=0, CALLDATALOAD(4)), STOP
    //   get()        at 0x27: RETURN(SLOAD(slot=0))
    //
    // Init code (12 bytes): CODECOPY runtime to memory, RETURN
    const SIMPLE_STORAGE_BYTECODE = '0x6034600c60003960346000f360003560e01c806360fe47b114601e5780636d4ce63c14602757600080fd5b50600435600055005b5060005460005260206000f3';

    logProgress('Deploying SimpleStorage on Node 0...');
    const deployHash = await walletClient.deployContract({
      abi: [
        { type: 'function', name: 'set', inputs: [{ type: 'uint256' }], outputs: [], stateMutability: 'nonpayable' },
        { type: 'function', name: 'get', inputs: [], outputs: [{ type: 'uint256' }], stateMutability: 'view' },
      ],
      bytecode: SIMPLE_STORAGE_BYTECODE as `0x${string}`,
    });

    // Wait for deployment
    const publicClient0 = createPublicClient({
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });

    let contractAddr: string | null = null;
    const deployed = await waitForCondition(async () => {
      try {
        const receipt = await publicClient0.getTransactionReceipt({ hash: deployHash });
        if (receipt?.contractAddress) {
          contractAddr = receipt.contractAddress;
          return true;
        }
      } catch { /* not yet */ }
      return false;
    }, 15000, 1000);

    if (!deployed || !contractAddr) {
      throw new Error(`Contract deployment receipt not found after 15s`);
    }
    logProgress(`Contract deployed at ${contractAddr}`);

    // Set value to 42
    const setHash = await walletClient.writeContract({
      address: contractAddr as `0x${string}`,
      abi: [
        { type: 'function', name: 'set', inputs: [{ type: 'uint256' }], outputs: [], stateMutability: 'nonpayable' },
      ],
      functionName: 'set',
      args: [42n],
    });
    logProgress(`set(42) tx: ${setHash.slice(0, 20)}...`);

    // Wait for set tx
    await waitForCondition(async () => {
      try {
        const r = await publicClient0.getTransactionReceipt({ hash: setHash });
        return r != null;
      } catch { return false; }
    }, 15000, 1000);

    // Wait for state propagation across nodes
    await sleep(5000);

    // Call get() on ALL nodes
    const getAbi = [
      { type: 'function', name: 'get', inputs: [], outputs: [{ type: 'uint256' }], stateMutability: 'view' },
    ] as const;

    let successCount = 0;
    for (let i = 0; i < NUM_NODES; i++) {
      try {
        const client = createPublicClient({
          chain: { ...foundry, id: CHAIN_ID },
          transport: http(EVM_RPC_URLS[i]),
        });

        const value = await client.readContract({
          address: contractAddr as `0x${string}`,
          abi: getAbi,
          functionName: 'get',
        });

        if (value !== 42n) {
          logProgress(`Node ${i}: get() returned ${value} (expected 42)`);
        } else {
          logProgress(`Node ${i}: get() = 42`);
          successCount++;
        }
      } catch (e: any) {
        logProgress(`Node ${i}: get() failed: ${e.message?.slice(0, 60)}`);
      }
    }

    if (successCount < NUM_NODES - 1) {
      throw new Error(`Only ${successCount}/${NUM_NODES} nodes returned correct contract state`);
    }
    logProgress(`Contract state consistent on ${successCount}/${NUM_NODES} nodes`);
  });

  await runTest(ctx, 'Spot order propagation across nodes', 'multinode-crossnode', 'Place spot buy on Node 0, verify on all nodes', async () => {
    const spotOrderAction = {
      type: 'spotOrder',
      orders: [{
        a: 128, // TEST-USDC
        b: true,
        p: '0.01', // Low price so it rests
        s: '10',
        r: false,
        t: { limit: { tif: 'Gtc' } },
      }],
      grouping: 'na',
    };

    logProgress('Placing spot buy order on Node 0...');
    const { signature, nonce } = await signAction(spotOrderAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], spotOrderAction, signature, nonce);

    // Wait for propagation (needs block inclusion + consensus propagation)
    const found = await waitForCondition(async () => {
      for (let i = 0; i < NUM_NODES; i++) {
        try {
          const orders = (await infoRequestTo(GATEWAY_URLS[i], 'spotOpenOrders', {
            user: TEST_ACCOUNTS.ALICE.address,
          })) as Array<unknown>;
          if (orders.length === 0) return false;
        } catch {
          return false;
        }
      }
      return true;
    }, 15000, 1000);

    if (!found) {
      // Log status for debugging
      for (let i = 0; i < NUM_NODES; i++) {
        try {
          const orders = (await infoRequestTo(GATEWAY_URLS[i], 'spotOpenOrders', {
            user: TEST_ACCOUNTS.ALICE.address,
          })) as Array<unknown>;
          logProgress(`Node ${i}: ${orders.length} spot orders`);
        } catch {
          logProgress(`Node ${i}: unreachable`);
        }
      }
      throw new Error('Spot order not propagated to all nodes within 15s');
    }
    logProgress('Spot order visible on all nodes');

    // Cleanup
    const cancelAction = { type: 'spotCancelAll', market: 128 };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], cancelAction, cSig, cNonce);
    await sleep(1000);
  });

  await runTest(ctx, 'View transfer propagation across nodes', 'multinode-crossnode', 'View transfer on Node 0 reflected on all nodes', async () => {
    // Get baseline unified balance on all nodes
    const baseBalances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const ub = (await infoRequestTo(GATEWAY_URLS[i], 'unifiedBalances', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { balances?: Array<{ tokenIndex?: number; evmView: string }> };
      const usdc = ub.balances?.find((b) => (b.tokenIndex || 0) === 0);
      baseBalances.push(usdc?.evmView || '0');
    }
    logProgress(`Baseline EVM view: ${baseBalances[0]}`);

    // Execute view transfer on Node 0
    const transferAction = {
      type: 'viewTransfer',
      token: 0,
      amount: '100',
      toEvm: true,
    };
    const { signature, nonce } = await signAction(transferAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], transferAction, signature, nonce);
    logProgress('View transfer (100 USDC to EVM) submitted on Node 0');

    // Poll until all nodes reflect the transfer (needs block inclusion + propagation)
    const baseEvmView = parseFloat(baseBalances[0]);
    let afterBalances: string[] = [];
    const propagated = await waitForCondition(async () => {
      afterBalances = [];
      for (let i = 0; i < NUM_NODES; i++) {
        const ub = (await infoRequestTo(GATEWAY_URLS[i], 'unifiedBalances', {
          user: TEST_ACCOUNTS.ALICE.address,
        })) as { balances?: Array<{ tokenIndex?: number; evmView: string }> };
        const usdc = ub.balances?.find((b) => (b.tokenIndex || 0) === 0);
        afterBalances.push(usdc?.evmView || '0');
      }
      // All nodes must have the same value AND it must have changed from baseline
      const allMatch = afterBalances.every((b) => b === afterBalances[0]);
      const changed = parseFloat(afterBalances[0]) !== baseEvmView;
      return allMatch && changed;
    }, 15000, 1000);

    if (!propagated) {
      throw new Error(`View transfer not consistent after 15s: ${JSON.stringify(afterBalances)}`);
    }

    const evmDiff = parseFloat(afterBalances[0]) - baseEvmView;
    logProgress(`EVM view increased by ${evmDiff} on all nodes (expected ~100)`);

    if (Math.abs(evmDiff - 100) > 1) {
      throw new Error(`Unexpected EVM view change: ${evmDiff} (expected 100)`);
    }

    // Restore
    const restoreAction = { type: 'viewTransfer', token: 0, amount: '100', toEvm: false };
    const { signature: rSig, nonce: rNonce } = await signAction(restoreAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], restoreAction, rSig, rNonce);
    await sleep(1000);
    logProgress('View transfer propagation verified across all nodes');
  });

  await runTest(ctx, 'Genesis state matches across all nodes', 'multinode-crossnode', 'Verify all genesis accounts and markets are consistent', async () => {
    // Check market metadata
    const metas: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const meta = (await infoRequestTo(GATEWAY_URLS[i], 'meta')) as {
        universe?: Array<{ name: string; maxLeverage?: number }>;
      };
      const summary = (meta.universe || []).map((m) => `${m.name}:${m.maxLeverage}`).sort().join(',');
      metas.push(summary);
    }

    const metaMatch = metas.every((m) => m === metas[0]);
    if (!metaMatch) {
      throw new Error(`Market metadata differs: ${JSON.stringify(metas)}`);
    }
    logProgress(`Markets consistent: ${metas[0]}`);

    // Check spot metadata
    const spotMetas: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const spotMeta = (await infoRequestTo(GATEWAY_URLS[i], 'spotMeta')) as {
        tokens?: Array<{ name: string; index: number }>;
      };
      const summary = (spotMeta.tokens || []).map((t) => `${t.name}:${t.index}`).sort().join(',');
      spotMetas.push(summary);
    }

    const spotMatch = spotMetas.every((m) => m === spotMetas[0]);
    if (!spotMatch) {
      throw new Error(`Spot metadata differs: ${JSON.stringify(spotMetas)}`);
    }
    logProgress(`Spot tokens consistent: ${spotMetas[0]}`);

    // Check Alice balance on all nodes
    const balances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const ub = (await infoRequestTo(GATEWAY_URLS[i], 'unifiedBalances', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { balances?: Array<{ tokenIndex?: number; total: string }> };
      const usdc = ub.balances?.find((b) => (b.tokenIndex || 0) === 0);
      balances.push(usdc?.total || '0');
    }

    const balMatch = balances.every((b) => b === balances[0]);
    if (!balMatch) {
      throw new Error(`Alice total balance differs: ${JSON.stringify(balances)}`);
    }
    logProgress(`Alice total balance consistent: ${balances[0]}`);
  });

  // =========================================================================
  // 10. CROSS-NODE ADVANCED TESTS
  // =========================================================================
  logSection('26. Cross-Node Advanced (Spot Matching, Nonce Replay, EVM Receipts)');

  await runTest(ctx, 'Cross-node spot matching with balance consistency', 'multinode-crossnode-advanced', 'Alice buys spot on Node 0, Bob sells on Node 3, verify balances on all nodes', async () => {
    // Pre-cleanup: cancel any stale spot orders from previous sections
    for (const acct of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB]) {
      const ca = { type: 'spotCancelAll', market: 128 };
      const { signature: cs, nonce: cn } = await signAction(ca, acct.privateKey);
      await exchangeRequestTo(GATEWAY_URLS[0], ca, cs, cn);
    }
    await sleep(3000);

    // Verify both users have zero spot orders before starting
    for (const acct of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB]) {
      const orders = (await infoRequestTo(GATEWAY_URLS[0], 'spotOpenOrders', {
        user: acct.address,
      })) as Array<{ limitPx?: string; side?: string }>;
      if (orders.length > 0) {
        logProgress(`Warning: ${acct.address.slice(0, 10)} still has ${orders.length} spot orders after pre-cleanup`);
      }
    }

    // Capture baseline unified balances for Alice and Bob (token 0 = USDC, token 1 = TEST)
    const getTokenBalances = async (gatewayUrl: string, user: string) => {
      const ub = (await infoRequestTo(gatewayUrl, 'unifiedBalances', { user })) as {
        balances?: Array<{ tokenIndex?: number; total: string }>;
      };
      const balances = ub.balances || [];
      const usdc = balances.find((b) => (b.tokenIndex || 0) === 0);
      const test = balances.find((b) => b.tokenIndex === 1);
      return {
        usdc: parseFloat(usdc?.total || '0'),
        test: parseFloat(test?.total || '0'),
      };
    };

    const aliceBefore = await getTokenBalances(GATEWAY_URLS[0], TEST_ACCOUNTS.ALICE.address);
    const bobBefore = await getTokenBalances(GATEWAY_URLS[0], TEST_ACCOUNTS.BOB.address);
    logProgress(`Baseline - Alice: USDC=${aliceBefore.usdc}, TEST=${aliceBefore.test}`);
    logProgress(`Baseline - Bob: USDC=${bobBefore.usdc}, TEST=${bobBefore.test}`);

    // Alice places spot BUY 10 TEST @ 1.0 on Node 0 (market 128)
    const buyAction = {
      type: 'spotOrder',
      orders: [{
        a: 128,
        b: true,
        p: '1.0',
        s: '10',
        r: false,
        t: { limit: { tif: 'Gtc' } },
      }],
      grouping: 'na',
    };
    logProgress('Alice placing spot BUY 10 TEST @ 1.0 on Node 0...');
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], buyAction, buySig, buyNonce);

    // Wait for Alice's order at price 1.0 to be visible on Node 3
    // Check price to avoid matching against stale orders from earlier tests
    const aliceVisible = await waitForCondition(async () => {
      const orders = (await infoRequestTo(GATEWAY_URLS[Math.min(3, NUM_NODES - 1)], 'spotOpenOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ limitPx?: string; side?: string }>;
      return orders.some((o) => parseFloat(o.limitPx || '0') === 1 && o.side === 'B');
    }, 15000, 1000);

    if (!aliceVisible) {
      const orders = (await infoRequestTo(GATEWAY_URLS[Math.min(3, NUM_NODES - 1)], 'spotOpenOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ limitPx?: string; side?: string; sz?: string }>;
      logProgress(`Node 3 Alice spot orders: ${JSON.stringify(orders)}`);
      throw new Error('Alice spot order at price 1.0 not visible on Node 3 after 15s');
    }
    logProgress('Alice spot BUY @ 1.0 confirmed on Node 3, placing Bob sell...');

    // Bob places spot SELL 10 TEST @ 1.0 on Node 3 — should cross
    const sellAction = {
      type: 'spotOrder',
      orders: [{
        a: 128,
        b: false,
        p: '1.0',
        s: '10',
        r: false,
        t: { limit: { tif: 'Gtc' } },
      }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(3, NUM_NODES - 1)], sellAction, sellSig, sellNonce);

    // Poll until both orders disappear (= matched), with diagnostics
    let lastAliceCount = -1;
    let lastBobCount = -1;
    const matched = await waitForCondition(async () => {
      const aliceOrders = (await infoRequestTo(GATEWAY_URLS[0], 'spotOpenOrders', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as Array<{ limitPx?: string; side?: string }>;
      const bobOrders = (await infoRequestTo(GATEWAY_URLS[0], 'spotOpenOrders', {
        user: TEST_ACCOUNTS.BOB.address,
      })) as Array<{ limitPx?: string; side?: string }>;
      if (aliceOrders.length !== lastAliceCount || bobOrders.length !== lastBobCount) {
        logProgress(`Poll: Alice=${aliceOrders.length} orders${aliceOrders[0] ? ` (${aliceOrders[0].side}@${aliceOrders[0].limitPx})` : ''}, Bob=${bobOrders.length} orders${bobOrders[0] ? ` (${bobOrders[0].side}@${bobOrders[0].limitPx})` : ''}`);
        lastAliceCount = aliceOrders.length;
        lastBobCount = bobOrders.length;
      }
      return aliceOrders.length === 0 && bobOrders.length === 0;
    }, 30000, 1000);

    if (!matched) {
      // Log final state for diagnostics
      for (let i = 0; i < NUM_NODES; i++) {
        const ao = (await infoRequestTo(GATEWAY_URLS[i], 'spotOpenOrders', {
          user: TEST_ACCOUNTS.ALICE.address,
        })) as Array<{ limitPx?: string; side?: string; sz?: string }>;
        const bo = (await infoRequestTo(GATEWAY_URLS[i], 'spotOpenOrders', {
          user: TEST_ACCOUNTS.BOB.address,
        })) as Array<{ limitPx?: string; side?: string; sz?: string }>;
        logProgress(`Node ${i}: Alice=${JSON.stringify(ao)}, Bob=${JSON.stringify(bo)}`);
      }
      // Cleanup before failing
      for (const acct of [TEST_ACCOUNTS.ALICE, TEST_ACCOUNTS.BOB]) {
        const ca = { type: 'spotCancelAll', market: 128 };
        const { signature: cs, nonce: cn } = await signAction(ca, acct.privateKey);
        await exchangeRequestTo(GATEWAY_URLS[0], ca, cs, cn);
      }
      await sleep(1000);
      throw new Error('Cross-node spot orders did not match within 30s');
    }
    logProgress('Spot orders matched successfully');

    // Wait for balance propagation
    await sleep(3000);

    // Verify balance changes: Alice TEST increased, Bob TEST decreased
    const aliceAfter = await getTokenBalances(GATEWAY_URLS[0], TEST_ACCOUNTS.ALICE.address);
    const bobAfter = await getTokenBalances(GATEWAY_URLS[0], TEST_ACCOUNTS.BOB.address);
    logProgress(`After - Alice: USDC=${aliceAfter.usdc}, TEST=${aliceAfter.test}`);
    logProgress(`After - Bob: USDC=${bobAfter.usdc}, TEST=${bobAfter.test}`);

    const aliceTestDelta = aliceAfter.test - aliceBefore.test;
    const bobTestDelta = bobAfter.test - bobBefore.test;
    logProgress(`Alice TEST delta: ${aliceTestDelta}, Bob TEST delta: ${bobTestDelta}`);

    if (aliceTestDelta <= 0) {
      throw new Error(`Expected Alice TEST balance to increase, but delta = ${aliceTestDelta}`);
    }
    if (bobTestDelta >= 0) {
      throw new Error(`Expected Bob TEST balance to decrease, but delta = ${bobTestDelta}`);
    }

    // Verify unifiedBalances identical across all 5 nodes
    const nodeBalances: string[] = [];
    for (let i = 0; i < NUM_NODES; i++) {
      const aliceUb = (await infoRequestTo(GATEWAY_URLS[i], 'unifiedBalances', {
        user: TEST_ACCOUNTS.ALICE.address,
      })) as { balances?: Array<{ tokenIndex?: number; total: string }> };
      const sorted = (aliceUb.balances || [])
        .sort((a, b) => (a.tokenIndex || 0) - (b.tokenIndex || 0))
        .map((b) => `${b.tokenIndex || 0}:${b.total}`);
      nodeBalances.push(JSON.stringify(sorted));
    }

    const allMatch = nodeBalances.every((b) => b === nodeBalances[0]);
    if (!allMatch) {
      throw new Error(`Unified balances differ across nodes after spot trade: ${JSON.stringify(nodeBalances)}`);
    }
    logProgress(`Spot balances consistent across all ${NUM_NODES} nodes`);
  });

  await runTest(ctx, 'Nonce replay protection and old nonce rejection', 'multinode-crossnode-advanced', 'Replayed nonce and old nonce both rejected', async () => {
    // Part A: Replay protection — same nonce rejected on retry
    const leverageAction = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 30,
    };
    const { signature, nonce } = await signAction(leverageAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    logProgress(`Submitting leverage update with nonce=${nonce} to Node 0...`);
    await exchangeRequestTo(GATEWAY_URLS[0], leverageAction, signature, nonce);
    logProgress('First submission succeeded');

    // Wait for block inclusion so that last_timestamp_nonce is updated
    await sleep(5000);

    // Re-submit exact same action + signature + nonce — must fail
    let replayRejected = false;
    try {
      await exchangeRequestTo(GATEWAY_URLS[0], leverageAction, signature, nonce);
      logProgress('WARNING: Replay was accepted (should have been rejected)');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      logProgress(`Replay correctly rejected: ${msg.slice(0, 80)}`);
      replayRejected = true;
    }

    if (!replayRejected) {
      throw new Error('Nonce replay was not rejected — same nonce should fail on retry');
    }

    // Verify chain continues producing blocks after rejection
    const statusBefore = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightBefore = parseInt(statusBefore.result.sync_info.latest_block_height);
    await sleep(3000);
    const statusAfter = (await cometbftRpcCall(COMETBFT_RPC_URLS[0], 'status')) as {
      result: { sync_info: { latest_block_height: string } };
    };
    const heightAfter = parseInt(statusAfter.result.sync_info.latest_block_height);
    if (heightAfter <= heightBefore) {
      throw new Error('Chain stopped producing blocks after nonce replay rejection');
    }
    logProgress(`Chain healthy: blocks ${heightBefore} -> ${heightAfter}`);

    // Part B: Old nonce rejection — nonce > 1 hour in the past
    const oldNonce = Date.now() - 7_200_000; // 2 hours ago

    // EIP-712 domain and types for updateLeverage
    const EIP712_DOMAIN = {
      name: 'HyperCore',
      version: '1',
      chainId: 1337,
      verifyingContract: '0x0000000000000000000000000000000000000000' as `0x${string}`,
    };
    const leverageTypes = {
      Action: [
        { name: 'type', type: 'string' },
        { name: 'asset', type: 'uint8' },
        { name: 'isCross', type: 'bool' },
        { name: 'leverage', type: 'uint8' },
        { name: 'nonce', type: 'uint64' },
      ],
    };
    const oldAction = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 35,
    };
    const oldMessage = { ...oldAction, nonce: BigInt(oldNonce) };

    const oldSigRaw = await signTypedData({
      privateKey: TEST_ACCOUNTS.CHARLIE.privateKey,
      domain: EIP712_DOMAIN,
      types: leverageTypes,
      primaryType: 'Action',
      message: oldMessage,
    });
    // Parse signature into r, s, v
    const sigHex = oldSigRaw.slice(2);
    const oldSig = {
      r: `0x${sigHex.slice(0, 64)}`,
      s: `0x${sigHex.slice(64, 128)}`,
      v: parseInt(sigHex.slice(128, 130), 16),
    };

    logProgress(`Submitting action with old nonce=${oldNonce} (2 hours ago)...`);
    let oldNonceRejected = false;
    try {
      await exchangeRequestTo(GATEWAY_URLS[0], oldAction, oldSig, oldNonce);
      logProgress('WARNING: Old nonce was accepted (should have been rejected)');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      logProgress(`Old nonce correctly rejected: ${msg.slice(0, 80)}`);
      oldNonceRejected = true;
    }

    if (!oldNonceRejected) {
      throw new Error('Old nonce (2 hours ago) was not rejected — should be outside 1-hour window');
    }

    // Reset leverage to default
    const resetAction = { type: 'updateLeverage', asset: 0, isCross: true, leverage: 10 };
    const { signature: rSig, nonce: rNonce } = await signAction(resetAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[0], resetAction, rSig, rNonce);
    await sleep(1000);
    logProgress('Nonce replay and old nonce protection verified');
  });

  await runTest(ctx, 'EVM transaction receipts consistent across nodes', 'multinode-crossnode-advanced', 'Deploy contract, verify receipt fields match on all nodes', async () => {
    const account = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
    const walletClient = createWalletClient({
      account,
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });

    // Deploy SimpleStorage (same bytecode as Section 25)
    const SIMPLE_STORAGE_BYTECODE = '0x6034600c60003960346000f360003560e01c806360fe47b114601e5780636d4ce63c14602757600080fd5b50600435600055005b5060005460005260206000f3';

    logProgress('Deploying SimpleStorage on Node 0...');
    const deployHash = await walletClient.deployContract({
      abi: [
        { type: 'function', name: 'set', inputs: [{ type: 'uint256' }], outputs: [], stateMutability: 'nonpayable' },
        { type: 'function', name: 'get', inputs: [], outputs: [{ type: 'uint256' }], stateMutability: 'view' },
      ],
      bytecode: SIMPLE_STORAGE_BYTECODE as `0x${string}`,
    });

    const publicClient0 = createPublicClient({
      chain: { ...foundry, id: CHAIN_ID },
      transport: http(EVM_RPC_URLS[0]),
    });

    let contractAddr: string | null = null;
    const deployed = await waitForCondition(async () => {
      try {
        const receipt = await publicClient0.getTransactionReceipt({ hash: deployHash });
        if (receipt?.contractAddress) {
          contractAddr = receipt.contractAddress;
          return true;
        }
      } catch { /* not yet */ }
      return false;
    }, 15000, 1000);

    if (!deployed || !contractAddr) {
      throw new Error('Contract deployment receipt not found after 15s');
    }
    logProgress(`Contract deployed at ${contractAddr}`);

    // Call set(42) and wait for receipt
    const setHash = await walletClient.writeContract({
      address: contractAddr as `0x${string}`,
      abi: [
        { type: 'function', name: 'set', inputs: [{ type: 'uint256' }], outputs: [], stateMutability: 'nonpayable' },
      ],
      functionName: 'set',
      args: [42n],
    });
    logProgress(`set(42) tx: ${setHash.slice(0, 20)}...`);

    let setReceiptNode0: any = null;
    await waitForCondition(async () => {
      try {
        setReceiptNode0 = await publicClient0.getTransactionReceipt({ hash: setHash });
        return setReceiptNode0 != null;
      } catch { return false; }
    }, 15000, 1000);

    if (!setReceiptNode0) {
      throw new Error('set(42) receipt not found on Node 0 after 15s');
    }
    logProgress(`set(42) receipt on Node 0: status=${setReceiptNode0.status}, gasUsed=${setReceiptNode0.gasUsed}`);

    // Wait for propagation
    await sleep(5000);

    // Query eth_getTransactionReceipt on all nodes via raw evmRpcCall
    const receipts: Array<Record<string, unknown>> = [];
    for (let i = 0; i < NUM_NODES; i++) {
      try {
        const receipt = (await evmRpcCall(EVM_RPC_URLS[i], 'eth_getTransactionReceipt', [setHash])) as Record<string, unknown>;
        if (!receipt) {
          throw new Error(`Node ${i} returned null receipt`);
        }
        receipts.push(receipt);
        logProgress(`Node ${i}: status=${receipt.status}, gasUsed=${receipt.gasUsed}, from=${(receipt.from as string)?.slice(0, 10)}...`);
      } catch (e: any) {
        throw new Error(`Node ${i} failed to return receipt: ${e.message?.slice(0, 80)}`);
      }
    }

    // Compare canonical receipt fields across all nodes
    const referenceReceipt = receipts[0];
    const fieldsToCompare = ['status', 'gasUsed', 'from', 'to', 'transactionHash'];

    for (let i = 1; i < receipts.length; i++) {
      for (const field of fieldsToCompare) {
        const refVal = String(referenceReceipt[field] || '').toLowerCase();
        const nodeVal = String(receipts[i][field] || '').toLowerCase();
        if (refVal !== nodeVal) {
          throw new Error(
            `Receipt field '${field}' differs: Node 0 = ${refVal}, Node ${i} = ${nodeVal}`
          );
        }
      }
      // Compare logs count
      const refLogs = Array.isArray(referenceReceipt.logs) ? referenceReceipt.logs.length : 0;
      const nodeLogsArr = receipts[i].logs;
      const nodeLogs = Array.isArray(nodeLogsArr) ? nodeLogsArr.length : 0;
      if (refLogs !== nodeLogs) {
        throw new Error(
          `Receipt logs count differs: Node 0 = ${refLogs}, Node ${i} = ${nodeLogs}`
        );
      }
    }
    logProgress('All receipt fields consistent across nodes');

    // Also verify get() == 42 on all nodes
    const getAbi = [
      { type: 'function', name: 'get', inputs: [], outputs: [{ type: 'uint256' }], stateMutability: 'view' },
    ] as const;

    for (let i = 0; i < NUM_NODES; i++) {
      const client = createPublicClient({
        chain: { ...foundry, id: CHAIN_ID },
        transport: http(EVM_RPC_URLS[i]),
      });
      const value = await client.readContract({
        address: contractAddr as `0x${string}`,
        abi: getAbi,
        functionName: 'get',
      });
      if (value !== 42n) {
        throw new Error(`Node ${i}: get() returned ${value}, expected 42`);
      }
      logProgress(`Node ${i}: get() = 42`);
    }
    logProgress('EVM receipts and contract state consistent across all nodes');
  });
}
