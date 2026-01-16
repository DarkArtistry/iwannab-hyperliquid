/**
 * Advanced EVM Integration Tests
 *
 * Tests smart contract deployment, state management, and advanced EVM
 * features for HyperCore's Ethereum-compatible execution environment.
 *
 * This module tests:
 * 1. Contract deployment (constructor execution)
 * 2. Contract code verification
 * 3. State reads via eth_call
 * 4. State writes via eth_sendRawTransaction
 * 5. Direct storage access via eth_getStorageAt
 *
 * The bytecode is pre-compiled using Foundry (solc 0.8.29) with the Cancun
 * EVM target, which supports the PUSH0 opcode (EIP-3855).
 */

import {
  createPublicClient,
  createWalletClient,
  http,
  parseEther,
  formatEther,
  parseGwei,
  encodeFunctionData,
  keccak256,
  toHex,
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

export async function runAdvancedEVMTests(ctx: TestContext): Promise<void> {
  logSection('8. Advanced EVM Tests');
  log('');
  log('  Testing contract deployment and smart contract interactions');
  log('');

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

  // =========================================================================
  // SIMPLE STORAGE CONTRACT
  // =========================================================================
  //
  // Source (Solidity 0.8.29):
  //   contract SimpleStorage {
  //     uint256 public value;
  //     function set(uint256 v) public { value = v; }
  //   }
  //
  // Compilation:
  //   solc --bin --optimize SimpleStorage.sol
  //   Target: Cancun EVM (supports PUSH0 opcode from EIP-3855)
  //
  // ABI Functions:
  //   - value() returns (uint256)  [selector: 0x3fa4f245]
  //   - set(uint256 v)             [selector: 0x60fe47b1]
  //
  // Storage Layout:
  //   Slot 0: uint256 value
  //
  // This is a minimal contract for testing state read/write operations.
  // =========================================================================
  const SIMPLE_STORAGE_BYTECODE =
    '0x6080604052348015600e575f5ffd5b5060aa80601a5f395ff3fe6080604052348015600e575f5ffd5b50600436106030575f3560e01c80633fa4f24514603457806360fe47b114604d575b5f5ffd5b603b5f5481565b60405190815260200160405180910390f35b605c6058366004605e565b5f55565b005b5f60208284031215606d575f5ffd5b503591905056fea26469706673582212208ebce05eb5d8a1701c1e92db96ffe4bd509d0006e145d6045f115ae5330aab9664736f6c634300081d0033';

  // ABI for SimpleStorage
  const SIMPLE_STORAGE_ABI = [
    {
      inputs: [],
      name: 'value',
      outputs: [{ internalType: 'uint256', name: '', type: 'uint256' }],
      stateMutability: 'view',
      type: 'function',
    },
    {
      inputs: [{ internalType: 'uint256', name: 'v', type: 'uint256' }],
      name: 'set',
      outputs: [],
      stateMutability: 'nonpayable',
      type: 'function',
    },
  ] as const;

  let deployedAddress: `0x${string}` | null = null;

  await runTest(ctx, 'Deploy SimpleStorage contract', 'evm-advanced', 'Deploy a simple storage contract', async () => {
    logProgress('Deploying SimpleStorage contract...');

    const hash = await walletClient.deployContract({
      abi: SIMPLE_STORAGE_ABI,
      bytecode: SIMPLE_STORAGE_BYTECODE,
    });
    logProgress(`Deploy tx hash: ${hash}`);

    logProgress('Waiting for deployment confirmation...');
    const receipt = await publicClient.waitForTransactionReceipt({ hash });

    if (receipt.status !== 'success') {
      throw new Error('Contract deployment failed');
    }

    if (!receipt.contractAddress) {
      throw new Error('No contract address in receipt');
    }

    deployedAddress = receipt.contractAddress;
    logProgress(`Contract deployed at: ${deployedAddress}`);
  });

  await runTest(ctx, 'Verify contract code', 'evm-advanced', 'Check deployed contract has code', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress(`Fetching code at ${deployedAddress}...`);
    const code = await publicClient.getCode({ address: deployedAddress });

    if (!code || code === '0x') {
      throw new Error('Contract has no code after deployment');
    }

    logProgress(`Contract code length: ${(code.length - 2) / 2} bytes`);
  });

  await runTest(ctx, 'Read initial contract state', 'evm-advanced', 'Call value() getter on deployed contract', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress('Reading initial value from contract...');
    const value = await publicClient.readContract({
      address: deployedAddress,
      abi: SIMPLE_STORAGE_ABI,
      functionName: 'value',
    });

    logProgress(`Initial value: ${value}`);
    if (value !== 0n) {
      throw new Error('Initial value should be 0');
    }
  });

  await runTest(ctx, 'Write contract state', 'evm-advanced', 'Call set() to update contract state', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress('Setting value to 42...');
    const hash = await walletClient.writeContract({
      address: deployedAddress,
      abi: SIMPLE_STORAGE_ABI,
      functionName: 'set',
      args: [42n],
    });
    logProgress(`Set tx hash: ${hash}`);

    logProgress('Waiting for confirmation...');
    const receipt = await publicClient.waitForTransactionReceipt({ hash });

    if (receipt.status !== 'success') {
      throw new Error('Set transaction failed');
    }
    logProgress(`Set confirmed in block ${receipt.blockNumber}`);
  });

  await runTest(ctx, 'Read updated contract state', 'evm-advanced', 'Verify set() updated the value', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress('Reading updated value from contract...');
    const value = await publicClient.readContract({
      address: deployedAddress,
      abi: SIMPLE_STORAGE_ABI,
      functionName: 'value',
    });

    logProgress(`Updated value: ${value}`);
    if (value !== 42n) {
      throw new Error(`Expected value 42, got ${value}`);
    }
  });

  await runTest(ctx, 'Read contract storage directly', 'evm-advanced', 'Use eth_getStorageAt to read raw storage', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress('Reading storage slot 0 directly...');
    const storage = await publicClient.getStorageAt({
      address: deployedAddress,
      slot: '0x0',
    });

    // Storage should contain 42 (0x2a)
    const value = BigInt(storage || '0x0');
    logProgress(`Raw storage value: ${storage} (${value})`);
    if (value !== 42n) {
      throw new Error(`Expected storage value 42, got ${value}`);
    }
  });

  await runTest(ctx, 'Estimate gas for contract call', 'evm-advanced', 'Estimate gas for set() call', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    logProgress('Estimating gas for set(100)...');
    const gas = await publicClient.estimateContractGas({
      address: deployedAddress,
      abi: SIMPLE_STORAGE_ABI,
      functionName: 'set',
      args: [100n],
      account: TEST_ACCOUNTS.ALICE.address,
    });

    logProgress(`Estimated gas: ${gas}`);
    if (gas < 21000n) throw new Error('Gas estimate too low');
  });

  await runTest(ctx, 'Multiple transactions in sequence', 'evm-advanced', 'Execute multiple state changes', async () => {
    if (!deployedAddress) {
      throw new Error('No deployed contract from previous test');
    }

    const values = [100n, 200n, 300n];
    logProgress(`Setting values: ${values.join(', ')}...`);

    for (const v of values) {
      const hash = await walletClient.writeContract({
        address: deployedAddress,
        abi: SIMPLE_STORAGE_ABI,
        functionName: 'set',
        args: [v],
      });
      await publicClient.waitForTransactionReceipt({ hash });
    }

    const finalValue = await publicClient.readContract({
      address: deployedAddress,
      abi: SIMPLE_STORAGE_ABI,
      functionName: 'value',
    });

    logProgress(`Final value: ${finalValue}`);
    if (finalValue !== 300n) {
      throw new Error(`Expected final value 300, got ${finalValue}`);
    }
  });

  await runTest(ctx, 'Check nonce increments correctly', 'evm-advanced', 'Verify nonce increases after transactions', async () => {
    const nonce = await publicClient.getTransactionCount({ address: TEST_ACCOUNTS.ALICE.address });
    logProgress(`Alice nonce after all transactions: ${nonce}`);
    if (nonce < 2) {
      throw new Error(`Expected nonce >= 2 after multiple transactions, got ${nonce}`);
    }
  });
}
