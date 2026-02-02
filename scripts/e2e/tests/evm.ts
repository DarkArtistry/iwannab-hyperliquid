/**
 * EVM Integration Tests
 *
 * Tests EVM RPC and contract interactions for HyperCore's
 * Ethereum-compatible execution environment.
 */

import {
  createPublicClient,
  createWalletClient,
  http,
  parseEther,
  formatEther,
} from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { foundry } from 'viem/chains';

import {
  CONFIG,
  TEST_ACCOUNTS,
  runTest,
  logSection,
  log,
  logProgress,
} from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runEVMTests(ctx: TestContext): Promise<void> {
  logSection('7. EVM Integration Tests');
  log('');
  log('  Testing EVM RPC and contract interactions');
  log('');

  // =========================================================================
  // EVM INTEGRATION (Phase 1a)
  // =========================================================================
  //
  // HyperCore runs a full EVM execution environment using revm (Rust EVM).
  // This provides Ethereum-compatible smart contract execution alongside
  // the high-performance trading engine.
  //
  // RPC ENDPOINT: http://localhost:8545
  // CHAIN ID: 1337 (local development)
  //
  // PRECOMPILE ADDRESSES:
  // ---------------------
  // Custom precompiles allow EVM contracts to read exchange state:
  //
  // 0x0800 - PositionReader    : Get user's position in a market
  // 0x0801 - AccountReader     : Get user's account balance/margin
  // 0x0802 - MarketReader      : Get market configuration
  // 0x0803 - OrderReader       : Get order details by ID
  // 0x0804 - FundingReader     : Get funding rate info
  // 0x0805 - OrderBookReader   : Get L2 orderbook snapshot
  // 0x0806 - SpotBalanceReader : Get spot token balance (Phase 1c)
  // 0x0807 - SpotMarketReader  : Get spot market info (Phase 1c)
  // 0x0808 - SpotOrderBookReader: Get spot orderbook (Phase 1c)
  //
  // These follow Hyperliquid's precompile convention for compatibility.
  //
  // VIEM CLIENTS:
  // -------------
  // We use viem (TypeScript Ethereum library) for EVM interactions:
  // - publicClient: Read-only operations (getBalance, call, etc.)
  // - walletClient: Signed transactions (sendTransaction, deployContract)
  // =========================================================================

  const publicClient = createPublicClient({
    chain: { ...foundry, id: CONFIG.CHAIN_ID },
    transport: http(CONFIG.EVM_RPC_URL),
  });

  const account = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
  const walletClient = createWalletClient({
    account,
    chain: { ...foundry, id: CONFIG.CHAIN_ID },
    transport: http(CONFIG.EVM_RPC_URL),
  });

  // ============================================================================
  // Basic RPC Methods
  // ============================================================================

  await runTest(ctx, 'eth_chainId', 'evm', 'Query chain ID from EVM RPC', async () => {
    logProgress('Fetching chain ID...');
    const chainId = await publicClient.getChainId();
    if (chainId !== CONFIG.CHAIN_ID) {
      throw new Error(`Chain ID mismatch: expected ${CONFIG.CHAIN_ID}, got ${chainId}`);
    }
    logProgress(`Chain ID: ${chainId}`);
  });

  await runTest(ctx, 'eth_blockNumber', 'evm', 'Query current block height from EVM', async () => {
    logProgress('Fetching block number...');
    const blockNumber = await publicClient.getBlockNumber();
    if (blockNumber < 1n) throw new Error(`Expected block number >= 1, got ${blockNumber}`);
    logProgress(`Current block: ${blockNumber}`);
  });

  await runTest(ctx, 'eth_gasPrice', 'evm', 'Query current gas price from EVM', async () => {
    logProgress('Fetching gas price...');
    const gasPrice = await publicClient.getGasPrice();
    if (gasPrice <= 0n) throw new Error('Gas price should be positive');
    logProgress(`Gas price: ${gasPrice} wei`);
  });

  // ============================================================================
  // Account State Methods
  // ============================================================================

  await runTest(ctx, 'eth_getBalance', 'evm', 'Check native token balance for test accounts', async () => {
    logProgress(`Checking Alice balance...`);
    const aliceBalance = await publicClient.getBalance({ address: TEST_ACCOUNTS.ALICE.address });
    logProgress(`Alice balance: ${formatEther(aliceBalance)} ETH`);

    logProgress(`Checking Bob balance...`);
    const bobBalance = await publicClient.getBalance({ address: TEST_ACCOUNTS.BOB.address });
    logProgress(`Bob balance: ${formatEther(bobBalance)} ETH`);
  });

  await runTest(ctx, 'eth_getTransactionCount', 'evm', 'Query nonce for test accounts', async () => {
    logProgress(`Fetching Alice nonce...`);
    const nonce = await publicClient.getTransactionCount({ address: TEST_ACCOUNTS.ALICE.address });
    logProgress(`Alice nonce: ${nonce}`);
  });

  await runTest(ctx, 'eth_getCode', 'evm', 'Query code at an address (EOA should be empty)', async () => {
    logProgress(`Fetching code for Alice (EOA)...`);
    const code = await publicClient.getCode({ address: TEST_ACCOUNTS.ALICE.address });
    // EOA should have no code
    if (code && code !== '0x') {
      throw new Error(`EOA has code (unexpected): ${code.slice(0, 40)}`);
    }
    logProgress('No code found (expected for EOA)');
  });

  await runTest(ctx, 'eth_getStorageAt', 'evm', 'Query storage slot at an address', async () => {
    logProgress('Fetching storage at slot 0 for Alice...');
    const storage = await publicClient.getStorageAt({
      address: TEST_ACCOUNTS.ALICE.address,
      slot: '0x0',
    });
    logProgress(`Storage value: ${storage}`);
  });

  // ============================================================================
  // Transaction Methods
  // ============================================================================

  let txHash: `0x${string}` | null = null;

  await runTest(ctx, 'eth_sendRawTransaction (ETH transfer)', 'evm', 'Execute a simple ETH transfer', async () => {
    logProgress('Sending 0.001 ETH from Alice to Bob...');
    txHash = await walletClient.sendTransaction({
      to: TEST_ACCOUNTS.BOB.address,
      value: parseEther('0.001'),
    });
    logProgress(`Transaction hash: ${txHash}`);

    logProgress('Waiting for confirmation...');
    const receipt = await publicClient.waitForTransactionReceipt({ hash: txHash });
    if (receipt.status !== 'success') throw new Error('Transaction failed');
    logProgress(`Confirmed in block ${receipt.blockNumber}, gas used: ${receipt.gasUsed}`);
  });

  await runTest(ctx, 'eth_getTransactionByHash', 'evm', 'Query transaction details by hash', async () => {
    if (!txHash) {
      throw new Error('No transaction hash from previous test');
    }
    logProgress(`Fetching transaction ${txHash}...`);
    const tx = await publicClient.getTransaction({ hash: txHash });
    if (!tx) throw new Error('Transaction not found');
    logProgress(`From: ${tx.from}, To: ${tx.to}, Value: ${formatEther(tx.value)} ETH`);
  });

  await runTest(ctx, 'eth_getTransactionReceipt', 'evm', 'Query transaction receipt by hash', async () => {
    if (!txHash) {
      throw new Error('No transaction hash from previous test');
    }
    logProgress(`Fetching receipt for ${txHash}...`);
    const receipt = await publicClient.getTransactionReceipt({ hash: txHash });
    if (!receipt) throw new Error('Receipt not found');
    logProgress(`Status: ${receipt.status}, Gas used: ${receipt.gasUsed}, Block: ${receipt.blockNumber}`);
  });

  await runTest(ctx, 'eth_estimateGas', 'evm', 'Estimate gas for a transaction', async () => {
    logProgress('Estimating gas for ETH transfer...');
    const gasEstimate = await publicClient.estimateGas({
      account: TEST_ACCOUNTS.ALICE.address,
      to: TEST_ACCOUNTS.BOB.address,
      value: parseEther('0.001'),
    });
    if (gasEstimate < 21000n) throw new Error('Gas estimate too low for transfer');
    logProgress(`Estimated gas: ${gasEstimate}`);
  });

  // ============================================================================
  // Block Methods
  // ============================================================================

  await runTest(ctx, 'eth_getBlockByNumber (latest)', 'evm', 'Query latest block by tag', async () => {
    logProgress('Fetching latest block...');
    const block = await publicClient.getBlock({ blockTag: 'latest' });
    if (!block) throw new Error('Block not found');
    logProgress(`Block ${block.number}: hash=${block.hash?.slice(0, 18)}..., timestamp=${block.timestamp}`);
  });

  await runTest(ctx, 'eth_getBlockByNumber (specific)', 'evm', 'Query block by number', async () => {
    logProgress('Fetching block 1...');
    const block = await publicClient.getBlock({ blockNumber: 1n });
    if (!block) throw new Error('Block 1 not found');
    logProgress(`Block 1: gasLimit=${block.gasLimit}, timestamp=${block.timestamp}`);
  });

  await runTest(ctx, 'eth_getBlockByHash', 'evm', 'Query block by hash', async () => {
    logProgress('Fetching latest block to get hash...');
    const latest = await publicClient.getBlock({ blockTag: 'latest' });
    if (!latest || !latest.hash) throw new Error('No block hash available');

    logProgress(`Fetching block by hash: ${latest.hash.slice(0, 18)}...`);
    const block = await publicClient.getBlock({ blockHash: latest.hash });
    if (!block) throw new Error('Block not found by hash');
    logProgress(`Block ${block.number} fetched successfully by hash`);
  });

  // ============================================================================
  // Call Methods
  // ============================================================================

  await runTest(ctx, 'eth_call (simple)', 'evm', 'Execute read-only call to zero address', async () => {
    logProgress('Executing eth_call to zero address...');
    // Call to zero address: either succeeds with empty result or reverts - both are valid EVM behavior
    try {
      const result = await publicClient.call({
        to: '0x0000000000000000000000000000000000000000',
        data: '0x',
      });
      logProgress(`Call result: ${result.data || '0x'}`);
    } catch (e) {
      // Revert is valid for calling zero address - the important thing is the RPC handled it
      const msg = (e as Error).message;
      if (msg.includes('revert') || msg.includes('execution') || msg.includes('halt')) {
        logProgress('Call reverted as expected for zero address');
      } else {
        throw e; // Unexpected error - propagate
      }
    }
  });

  await runTest(ctx, 'eth_call (precompile)', 'evm', 'Attempt to call HyperCore precompile', async () => {
    logProgress('Calling position precompile at 0x0800...');
    // Precompile may or may not be available - test that RPC handles the call without crashing
    try {
      const result = await publicClient.call({
        to: '0x0000000000000000000000000000000000000800',
        data: '0x',
      });
      logProgress(`Precompile response: ${result.data || '0x'}`);
    } catch (e) {
      const msg = (e as Error).message;
      if (msg.includes('revert') || msg.includes('execution') || msg.includes('halt') || msg.includes('precompile')) {
        logProgress('Precompile call reverted (expected without proper input encoding)');
      } else {
        throw e; // Unexpected error
      }
    }
  });

  // ============================================================================
  // Fee Methods
  // ============================================================================

  await runTest(ctx, 'eth_maxPriorityFeePerGas', 'evm', 'Query max priority fee per gas', async () => {
    logProgress('Fetching max priority fee...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_maxPriorityFeePerGas', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: string; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    logProgress(`Max priority fee: ${data.result}`);
  });

  await runTest(ctx, 'eth_feeHistory', 'evm', 'Query fee history for recent blocks', async () => {
    logProgress('Fetching fee history...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_feeHistory', params: ['0x5', 'latest', [25, 75]], id: 1 }),
    });
    const data = (await response.json()) as { result?: { baseFeePerGas: string[] }; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    logProgress(`Fee history: oldestBlock=${data.result?.baseFeePerGas?.length || 0} entries`);
  });

  // ============================================================================
  // Web3 & Net Methods
  // ============================================================================

  await runTest(ctx, 'web3_clientVersion', 'evm', 'Query client version string', async () => {
    logProgress('Fetching client version...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'web3_clientVersion', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: string; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    if (!data.result?.includes('HyperEVM')) throw new Error(`Unexpected client: ${data.result}`);
    logProgress(`Client: ${data.result}`);
  });

  await runTest(ctx, 'net_version', 'evm', 'Query network version (chain ID)', async () => {
    logProgress('Fetching network version...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'net_version', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: string; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    if (data.result !== CONFIG.CHAIN_ID.toString()) {
      throw new Error(`Network version mismatch: expected ${CONFIG.CHAIN_ID}, got ${data.result}`);
    }
    logProgress(`Network version: ${data.result}`);
  });

  await runTest(ctx, 'net_listening', 'evm', 'Check if node is listening for connections', async () => {
    logProgress('Checking if node is listening...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'net_listening', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: boolean; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    if (data.result !== true) throw new Error('Node should be listening');
    logProgress('Node is listening: true');
  });

  await runTest(ctx, 'net_peerCount', 'evm', 'Query number of connected peers', async () => {
    logProgress('Fetching peer count...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'net_peerCount', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: string; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    logProgress(`Peer count: ${data.result}`);
  });

  // ============================================================================
  // Misc Methods
  // ============================================================================

  await runTest(ctx, 'eth_accounts', 'evm', 'Query unlocked accounts (should be empty)', async () => {
    logProgress('Fetching accounts...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_accounts', params: [], id: 1 }),
    });
    const data = (await response.json()) as { result?: string[]; error?: { message: string } };
    if (data.error) throw new Error(data.error.message);
    logProgress(`Accounts: ${data.result?.length || 0} (expected 0 for signing-only RPC)`);
  });

  await runTest(ctx, 'eth_getLogs (empty filter)', 'evm', 'Query logs with empty filter', async () => {
    logProgress('Fetching logs...');
    const logs = await publicClient.getLogs({});
    logProgress(`Found ${logs.length} logs`);
  });
}
