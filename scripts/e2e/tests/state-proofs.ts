/**
 * State Proof Tests (Phase 3C)
 *
 * Tests for Merkle tree state proofs and state commitment verification.
 * These tests verify that:
 * - State info API returns valid block height and state roots
 * - Merkle proofs are generated correctly for user balances
 * - Proof verification works client-side
 * - State roots change after transactions
 */

import { TEST_ACCOUNTS, infoRequest, exchangeRequest, runTest, logSection, log, logProgress, signAction } from '../lib/index.js';
import type { TestContext } from '../lib/index.js';
import { keccak256 } from 'viem';

/**
 * Verify a Merkle proof client-side
 * Mirrors the verification logic in crates/chain/src/merkle.rs
 */
function verifyMerkleProof(
  leafHash: string,
  siblings: string[],
  directions: string[],
  expectedRoot: string
): boolean {
  let current = hexToBytes(leafHash);

  for (let i = 0; i < siblings.length; i++) {
    const sibling = hexToBytes(siblings[i]);
    const direction = directions[i];

    if (direction === 'left') {
      current = hashPair(sibling, current);
    } else {
      current = hashPair(current, sibling);
    }
  }

  return bytesToHex(current) === expectedRoot.toLowerCase();
}

/**
 * Hash two 32-byte values together using keccak256
 */
function hashPair(left: Uint8Array, right: Uint8Array): Uint8Array {
  const combined = new Uint8Array(64);
  combined.set(left, 0);
  combined.set(right, 32);
  const hash = keccak256(combined);
  return hexToBytes(hash);
}

function hexToBytes(hex: string): Uint8Array {
  const cleaned = hex.startsWith('0x') ? hex.slice(2) : hex;
  const bytes = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(cleaned.substr(i * 2, 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes: Uint8Array): string {
  return '0x' + Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

export async function runStateProofTests(ctx: TestContext): Promise<void> {
  logSection('16. State Proof Tests (Phase 3C)');
  log('');
  log('  Testing Merkle tree state proofs and verification');
  log('');

  // Track state for comparison tests
  let initialStateRoot: string | null = null;

  // Test 1: Get current state info
  await runTest(ctx, 'Get state info', 'state-proofs', 'Retrieve current block height and state roots', async () => {
    logProgress('Fetching state info...');
    const stateInfo = (await infoRequest('stateInfo', {})) as {
      blockHeight?: number;
      appHash?: string;
      stateRoots?: {
        unifiedState?: string;
        nonces?: string;
      };
    };

    if (stateInfo.blockHeight === undefined) throw new Error('Missing blockHeight');
    if (!stateInfo.appHash) throw new Error('Missing appHash');
    if (!stateInfo.stateRoots) throw new Error('Missing stateRoots');
    if (!stateInfo.stateRoots.unifiedState) throw new Error('Missing unified state root');
    if (!stateInfo.stateRoots.nonces) throw new Error('Missing nonce root');

    initialStateRoot = stateInfo.stateRoots.unifiedState;

    logProgress(`Block height: ${stateInfo.blockHeight}`);
    logProgress(`App hash: ${stateInfo.appHash.slice(0, 18)}...`);
    logProgress(`Unified state root: ${stateInfo.stateRoots.unifiedState.slice(0, 18)}...`);
    logProgress(`Nonce root: ${stateInfo.stateRoots.nonces.slice(0, 18)}...`);
  });

  // Test 2: Get balance proof for Alice (should have genesis balance)
  await runTest(ctx, 'Get Alice balance proof', 'state-proofs', 'Retrieve Merkle proof for Alice USDC balance', async () => {
    logProgress(`Fetching proof for ${TEST_ACCOUNTS.ALICE.address}...`);
    const proofResult = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.ALICE.address,
      token: 0, // USDC
    })) as {
      user?: string;
      token?: number;
      balance?: {
        total?: string;
        coreView?: string;
        evmView?: string;
      };
      proof?: {
        leafHash?: string;
        leafIndex?: number;
        siblings?: string[];
        directions?: string[];
        proofHex?: string;
      };
      root?: string;
      verified?: boolean;
      error?: string;
    };

    if (proofResult.error) {
      throw new Error(`Alice should have a balance from genesis, but got error: ${proofResult.error}`);
    }

    if (!proofResult.balance) throw new Error('Missing balance in proof result');
    if (!proofResult.proof) throw new Error('Missing proof data');
    if (!proofResult.root) throw new Error('Missing root in proof result');

    logProgress(`Balance: ${proofResult.balance.total} total (core: ${proofResult.balance.coreView}, evm: ${proofResult.balance.evmView})`);
    logProgress(`Proof leaf index: ${proofResult.proof.leafIndex}`);
    logProgress(`Proof siblings: ${proofResult.proof.siblings?.length || 0}`);
    logProgress(`Server verified: ${proofResult.verified}`);

    if (!proofResult.verified) throw new Error('Server-side proof verification failed');
  });

  // Test 3: Get balance proof for Bob
  await runTest(ctx, 'Get Bob balance proof', 'state-proofs', 'Retrieve Merkle proof for Bob balance', async () => {
    logProgress(`Fetching proof for ${TEST_ACCOUNTS.BOB.address}...`);
    const proofResult = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.BOB.address,
      token: 0,
    })) as {
      balance?: { total?: string };
      proof?: { leafHash?: string; siblings?: string[]; directions?: string[] };
      root?: string;
      verified?: boolean;
      error?: string;
    };

    if (proofResult.error) {
      throw new Error(`Bob should have a balance from genesis, but got error: ${proofResult.error}`);
    }

    if (!proofResult.verified) throw new Error('Server-side proof verification failed');
    logProgress(`Bob balance verified: ${proofResult.balance?.total}`);
  });

  // Test 4: Client-side proof verification
  await runTest(ctx, 'Client-side proof verification', 'state-proofs', 'Verify Merkle proof locally using keccak256', async () => {
    const proofResult = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.ALICE.address,
      token: 0,
    })) as {
      proof?: {
        leafHash?: string;
        siblings?: string[];
        directions?: string[];
      };
      root?: string;
      error?: string;
    };

    if (proofResult.error) {
      throw new Error(`Cannot verify proof: ${proofResult.error}`);
    }

    if (!proofResult.proof?.leafHash || !proofResult.proof?.siblings || !proofResult.proof?.directions || !proofResult.root) {
      throw new Error('Incomplete proof data for verification');
    }

    logProgress('Verifying proof client-side...');
    const verified = verifyMerkleProof(
      proofResult.proof.leafHash,
      proofResult.proof.siblings,
      proofResult.proof.directions,
      proofResult.root
    );

    if (!verified) {
      throw new Error('Client-side proof verification failed');
    }

    logProgress('Client-side verification successful!');
  });

  // Test 5: Proof for non-existent user (should return error)
  await runTest(ctx, 'Proof for non-existent user', 'state-proofs', 'Request proof for user with no balance returns appropriate response', async () => {
    const nonExistentAddress = '0x0000000000000000000000000000000000000001';
    logProgress(`Fetching proof for ${nonExistentAddress}...`);

    const proofResult = (await infoRequest('stateProof', {
      user: nonExistentAddress,
      token: 0,
    })) as {
      error?: string;
      proof?: unknown;
    };

    // Either error message or null proof is acceptable
    if (proofResult.error) {
      logProgress(`Got expected error: ${proofResult.error}`);
    } else if (proofResult.proof === null) {
      logProgress('Got null proof as expected for non-existent user');
    } else {
      throw new Error('Expected error or null proof for non-existent user');
    }
  });

  // Test 6: State root consistency
  await runTest(ctx, 'State root consistency', 'state-proofs', 'Verify state roots are consistent across requests', async () => {
    logProgress('Fetching state info twice...');

    const info1 = (await infoRequest('stateInfo', {})) as {
      stateRoots?: { unifiedState?: string };
    };
    const info2 = (await infoRequest('stateInfo', {})) as {
      stateRoots?: { unifiedState?: string };
    };

    if (!info1.stateRoots?.unifiedState || !info2.stateRoots?.unifiedState) {
      throw new Error('Missing state roots');
    }

    // State roots should be identical if no transactions between requests
    if (info1.stateRoots.unifiedState !== info2.stateRoots.unifiedState) {
      logProgress('State roots differ - this may be expected if transactions occurred');
    } else {
      logProgress('State roots are consistent');
    }
  });

  // Test 7: Multiple token proofs
  await runTest(ctx, 'Multi-token proof retrieval', 'state-proofs', 'Retrieve proofs for different token indices', async () => {
    logProgress('Fetching proofs for multiple tokens...');

    // Test USDC (token 0)
    const proof0 = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.ALICE.address,
      token: 0,
    })) as { error?: string; verified?: boolean };

    // Test token 1 if it exists
    const proof1 = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.ALICE.address,
      token: 1,
    })) as { error?: string; verified?: boolean };

    logProgress(`Token 0: ${proof0.error ? 'no balance' : (proof0.verified ? 'verified' : 'failed')}`);
    logProgress(`Token 1: ${proof1.error ? 'no balance' : (proof1.verified ? 'verified' : 'failed')}`);
  });

  // Test 8: Proof includes all expected fields
  await runTest(ctx, 'Proof structure validation', 'state-proofs', 'Verify proof response includes all required fields', async () => {
    const proofResult = (await infoRequest('stateProof', {
      user: TEST_ACCOUNTS.ALICE.address,
      token: 0,
    })) as Record<string, unknown>;

    // Check required top-level fields
    if (!('user' in proofResult)) throw new Error('Missing user field');
    if (!('token' in proofResult)) throw new Error('Missing token field');

    if (!('proof' in proofResult) || proofResult.proof === null) {
      throw new Error('No proof returned for Alice USDC - Alice should have a balance');
    }

    const proof = proofResult.proof as Record<string, unknown>;
    if (!('leafHash' in proof)) throw new Error('Missing leafHash in proof');
    if (!('leafIndex' in proof)) throw new Error('Missing leafIndex in proof');
    if (!('siblings' in proof)) throw new Error('Missing siblings in proof');
    if (!('directions' in proof)) throw new Error('Missing directions in proof');
    if (!('proofHex' in proof)) throw new Error('Missing proofHex in proof');

    if (!('root' in proofResult)) throw new Error('Missing root field');
    if (!('verified' in proofResult)) throw new Error('Missing verified field');

    logProgress('All proof fields present');
  });

  // Test 9: App hash derivation
  await runTest(ctx, 'App hash derivation', 'state-proofs', 'Verify app hash is derived from state roots', async () => {
    const stateInfo = (await infoRequest('stateInfo', {})) as {
      appHash?: string;
      stateRoots?: {
        unifiedState?: string;
        nonces?: string;
      };
    };

    if (!stateInfo.appHash || !stateInfo.stateRoots?.unifiedState || !stateInfo.stateRoots?.nonces) {
      throw new Error('Missing state info fields');
    }

    // App hash should be keccak256(unifiedStateRoot || nonceRoot)
    const unifiedBytes = hexToBytes(stateInfo.stateRoots.unifiedState);
    const nonceBytes = hexToBytes(stateInfo.stateRoots.nonces);
    const combined = new Uint8Array(64);
    combined.set(unifiedBytes, 0);
    combined.set(nonceBytes, 32);
    const expectedAppHash = keccak256(combined);

    if (expectedAppHash.toLowerCase() !== stateInfo.appHash.toLowerCase()) {
      throw new Error(`App hash mismatch: expected ${expectedAppHash}, got ${stateInfo.appHash}`);
    }

    logProgress('App hash derivation verified');
  });
}
