/**
 * Test Accounts
 *
 * Deterministic test accounts from Foundry/Anvil derived from the mnemonic:
 * "test test test test test test test test test test test junk"
 *
 * IMPORTANT: These keys are PUBLIC and well-known across the Ethereum ecosystem.
 * NEVER use them on mainnet or any chain with real value!
 *
 * HOW ACCOUNTS GET FUNDED:
 * ------------------------
 * 1. HyperCore Balances (for trading):
 *    The node binary initializes these accounts with trading balances:
 *      - 100,000 USDC (token index 0) for placing orders
 *      - 10,000 TEST tokens (token index 1) for spot trading tests
 *
 * 2. EVM/Native ETH Balance (for contract deployment):
 *    The node credits native ETH to test accounts so they can deploy
 *    contracts and pay gas fees.
 *
 * ACCOUNT DERIVATION:
 * - Index 0 (Alice): m/44'/60'/0'/0/0
 * - Index 1 (Bob):   m/44'/60'/0'/0/1
 * - Index 2 (Charlie): m/44'/60'/0'/0/2
 */

export interface TestAccount {
  address: `0x${string}`;
  privateKey: `0x${string}`;
}

export const TEST_ACCOUNTS: Record<string, TestAccount> = {
  /** Alice - Primary test account, used for most tests */
  ALICE: {
    address: '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266',
    privateKey: '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80',
  },
  /** Bob - Secondary test account, used for matching tests (counterparty) */
  BOB: {
    address: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
    privateKey: '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d',
  },
  /** Charlie - Third test account, used for stress tests and isolation */
  CHARLIE: {
    address: '0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC',
    privateKey: '0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a',
  },
};
