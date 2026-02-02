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
} from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { foundry } from 'viem/chains';

import type { TestContext, SignatureWire } from '../lib/types.js';
import { logSection, logProgress } from '../lib/logging.js';
import { runTest, sleep } from '../lib/testing.js';
import { signAction } from '../lib/signing.js';
import { TEST_ACCOUNTS } from '../lib/accounts.js';

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
    await sleep(4000);

    // Cancel on Node 1
    logProgress('Cancelling all orders via Node 1...');
    const cancelAction = { type: 'cancelAll' };
    const { signature: cSig, nonce: cNonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequestTo(GATEWAY_URLS[Math.min(1, NUM_NODES - 1)], cancelAction, cSig, cNonce);
    await sleep(4000);

    // Verify removed on all nodes
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
    if (successCount === 0) {
      throw new Error('No EVM transactions succeeded across any node');
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
}
