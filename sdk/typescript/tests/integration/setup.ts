/**
 * Integration Test Setup & Utilities
 *
 * This file provides the foundation for all integration tests including:
 * - Test configuration and environment setup
 * - Pre-funded test accounts with known private keys
 * - Helper functions for common test operations
 * - Assertion utilities for exchange-specific validations
 *
 * @module tests/integration/setup
 */

import { HyperCore } from '../../src/client';
import { OrderSide, OrderType } from '../../src/types';

// ============================================================================
// CONFIGURATION
// ============================================================================

/**
 * Test environment configuration.
 * Override with environment variables for different environments.
 */
export const CONFIG = {
  /** Gateway HTTP API endpoint */
  GATEWAY_URL: process.env.GATEWAY_URL || 'http://localhost:3000',

  /** WebSocket endpoint for real-time updates */
  WS_URL: process.env.WS_URL || 'ws://localhost:3000/ws',

  /** EVM chain ID (1337 for local Anvil) */
  CHAIN_ID: parseInt(process.env.CHAIN_ID || '1337'),

  /** Request timeout in milliseconds */
  TIMEOUT: parseInt(process.env.TEST_TIMEOUT || '10000'),

  /** Delay between operations to allow processing (ms) */
  OPERATION_DELAY: 100,
} as const;

// ============================================================================
// TEST ACCOUNTS
// ============================================================================

/**
 * Anvil's deterministic test accounts.
 * These accounts have 10,000 ETH each on local Anvil.
 *
 * @example
 * ```typescript
 * const alice = createClient(TEST_ACCOUNTS.ALICE);
 * console.log(alice.address); // 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
 * ```
 */
export const TEST_ACCOUNTS = {
  /**
   * Alice - Primary test trader
   * Used for most trading scenarios
   */
  ALICE: {
    address: '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266',
    privateKey: '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80',
  },

  /**
   * Bob - Counter-party trader
   * Used for matching scenarios
   */
  BOB: {
    address: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
    privateKey: '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d',
  },

  /**
   * Charlie - Market maker
   * Used for liquidity provision scenarios
   */
  CHARLIE: {
    address: '0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC',
    privateKey: '0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a',
  },

  /**
   * Dave - Additional trader
   * Used for multi-party scenarios
   */
  DAVE: {
    address: '0x90F79bf6EB2c4f870365E785982E1f101E93b906',
    privateKey: '0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6',
  },

  /**
   * Eve - Additional trader
   * Used for stress tests and edge cases
   */
  EVE: {
    address: '0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65',
    privateKey: '0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a',
  },
} as const;

// ============================================================================
// MARKET CONSTANTS
// ============================================================================

/**
 * Available trading markets.
 * Use these constants instead of hardcoded strings.
 */
export const MARKETS = {
  BTC_PERP: 'BTC-PERP',
  ETH_PERP: 'ETH-PERP',
} as const;

/**
 * Reference prices for testing.
 * These are approximate market prices for order calculations.
 */
export const REFERENCE_PRICES = {
  BTC: '65000',
  ETH: '3500',
} as const;

// ============================================================================
// CLIENT FACTORY
// ============================================================================

/**
 * Create a HyperCore client for a test account.
 *
 * @param account - Test account with address and privateKey
 * @returns Configured HyperCore client
 *
 * @example
 * ```typescript
 * const alice = createClient(TEST_ACCOUNTS.ALICE);
 *
 * // Place an order
 * await alice.exchange.placeOrder({
 *   market: 'BTC-PERP',
 *   side: OrderSide.Buy,
 *   price: '65000',
 *   size: '0.1',
 * });
 * ```
 */
export function createClient(account: { address: string; privateKey: string }): HyperCore {
  return new HyperCore({
    baseUrl: CONFIG.GATEWAY_URL,
    wsUrl: CONFIG.WS_URL,
    privateKey: account.privateKey,
    chainId: CONFIG.CHAIN_ID,
    timeout: CONFIG.TIMEOUT,
  });
}

/**
 * Create an unauthenticated client for read-only operations.
 *
 * @returns HyperCore client without signing capability
 *
 * @example
 * ```typescript
 * const reader = createReadOnlyClient();
 *
 * // Read market data
 * const book = await reader.info.getL2Book('BTC-PERP');
 * ```
 */
export function createReadOnlyClient(): HyperCore {
  return new HyperCore({
    baseUrl: CONFIG.GATEWAY_URL,
    wsUrl: CONFIG.WS_URL,
    chainId: CONFIG.CHAIN_ID,
    timeout: CONFIG.TIMEOUT,
  });
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/**
 * Wait for a specified duration.
 * Useful for allowing async operations to complete.
 *
 * @param ms - Milliseconds to wait
 */
export function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

/**
 * Wait for the standard operation delay.
 * Use between dependent operations.
 */
export function waitForProcessing(): Promise<void> {
  return sleep(CONFIG.OPERATION_DELAY);
}

/**
 * Generate a unique client order ID for tracking.
 *
 * @param prefix - Optional prefix for identification
 * @returns Unique client order ID
 *
 * @example
 * ```typescript
 * const cloid = generateClientOrderId('test');
 * // Returns something like 'test-1705123456789-abc123'
 * ```
 */
export function generateClientOrderId(prefix = 'test'): string {
  const timestamp = Date.now();
  const random = Math.random().toString(36).substring(2, 8);
  return `${prefix}-${timestamp}-${random}`;
}

/**
 * Format a number as a decimal string with specified precision.
 *
 * @param value - Numeric value
 * @param decimals - Number of decimal places (default: 8)
 * @returns Formatted string
 */
export function formatDecimal(value: number, decimals = 8): string {
  return value.toFixed(decimals);
}

/**
 * Calculate order value (price * size).
 *
 * @param price - Order price
 * @param size - Order size
 * @returns Order value as string
 */
export function calculateOrderValue(price: string, size: string): string {
  return (parseFloat(price) * parseFloat(size)).toFixed(2);
}

// ============================================================================
// ASSERTION HELPERS
// ============================================================================

/**
 * Assert that an order response indicates success.
 *
 * @param response - Order API response
 * @param message - Optional assertion message
 */
export function assertOrderSuccess(response: unknown, message?: string): void {
  const res = response as { status: string };
  if (res.status !== 'ok') {
    throw new Error(message || `Expected order success, got status: ${res.status}`);
  }
}

/**
 * Assert that a value is within expected range.
 *
 * @param actual - Actual value
 * @param expected - Expected value
 * @param tolerance - Acceptable deviation (default: 0.01 = 1%)
 */
export function assertWithinTolerance(
  actual: number | string,
  expected: number | string,
  tolerance = 0.01
): void {
  const a = typeof actual === 'string' ? parseFloat(actual) : actual;
  const e = typeof expected === 'string' ? parseFloat(expected) : expected;
  const diff = Math.abs(a - e) / e;

  if (diff > tolerance) {
    throw new Error(`Value ${a} is not within ${tolerance * 100}% of expected ${e}`);
  }
}

/**
 * Assert that account has minimum balance.
 *
 * @param client - HyperCore client
 * @param minBalance - Minimum expected balance
 */
export async function assertMinimumBalance(
  client: HyperCore,
  minBalance: string
): Promise<void> {
  const state = await client.info.getAccountState(client.address!);
  const balance = parseFloat(state.marginSummary.accountValue);
  const required = parseFloat(minBalance);

  if (balance < required) {
    throw new Error(`Insufficient balance: ${balance} < ${required}`);
  }
}

// ============================================================================
// TEST DATA GENERATORS
// ============================================================================

/**
 * Generate a batch of limit orders at different price levels.
 * Useful for testing order book depth.
 *
 * @param basePrice - Center price
 * @param spread - Price spread between levels (e.g., 0.01 = 1%)
 * @param levels - Number of price levels
 * @param sizePerLevel - Size at each level
 * @returns Array of order parameters
 *
 * @example
 * ```typescript
 * // Generate 5 buy orders below $65,000
 * const buyOrders = generateLadderOrders('65000', 0.001, 5, '0.1', 'buy');
 * // Prices: 64935, 64870, 64805, 64740, 64675
 * ```
 */
export function generateLadderOrders(
  basePrice: string,
  spread: number,
  levels: number,
  sizePerLevel: string,
  side: 'buy' | 'sell'
): Array<{
  market: string;
  side: OrderSide;
  price: string;
  size: string;
  type: OrderType;
}> {
  const base = parseFloat(basePrice);
  const orders = [];

  for (let i = 1; i <= levels; i++) {
    const multiplier = side === 'buy' ? 1 - spread * i : 1 + spread * i;
    orders.push({
      market: MARKETS.BTC_PERP,
      side: side === 'buy' ? OrderSide.Buy : OrderSide.Sell,
      price: (base * multiplier).toFixed(0),
      size: sizePerLevel,
      type: OrderType.Limit,
    });
  }

  return orders;
}

// ============================================================================
// CLEANUP UTILITIES
// ============================================================================

/**
 * Cancel all open orders for a client.
 * Use in test cleanup to ensure clean state.
 *
 * @param client - HyperCore client
 */
export async function cancelAllOrders(client: HyperCore): Promise<void> {
  try {
    await client.exchange.cancelAllOrders();
  } catch {
    // Ignore errors if no orders to cancel
  }
}

/**
 * Reset test state by canceling orders for all test accounts.
 */
export async function resetTestState(): Promise<void> {
  const clients = [
    createClient(TEST_ACCOUNTS.ALICE),
    createClient(TEST_ACCOUNTS.BOB),
    createClient(TEST_ACCOUNTS.CHARLIE),
  ];

  await Promise.all(clients.map(cancelAllOrders));
  await waitForProcessing();
}

// ============================================================================
// EXPORTS
// ============================================================================

export { OrderSide, OrderType } from '../../src/types';
