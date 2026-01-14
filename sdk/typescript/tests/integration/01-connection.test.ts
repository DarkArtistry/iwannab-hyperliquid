/**
 * Connection & Health Check Tests
 *
 * These tests verify basic connectivity to the HyperCore exchange.
 * Run these first to ensure the environment is properly configured.
 *
 * @module tests/integration/01-connection
 */

import { describe, test, expect, beforeAll } from 'vitest';
import {
  CONFIG,
  TEST_ACCOUNTS,
  createClient,
  createReadOnlyClient,
} from './setup';

describe('Connection & Health Checks', () => {
  /**
   * @scenario Verify gateway is reachable
   * @given The gateway is running on the configured URL
   * @when We attempt to fetch exchange metadata
   * @then We receive a valid response with market information
   *
   * This is the most basic connectivity test. If this fails, check:
   * - Gateway is running (`cargo run --bin hypercore-gateway`)
   * - URL is correct (default: http://localhost:3000)
   * - No firewall blocking the connection
   */
  test('gateway should be reachable', async () => {
    const client = createReadOnlyClient();

    const meta = await client.info.getMeta();

    expect(meta).toBeDefined();
    expect(meta.universe).toBeDefined();
    expect(Array.isArray(meta.universe)).toBe(true);
  });

  /**
   * @scenario Verify available trading markets
   * @given The exchange has configured perpetual markets
   * @when We fetch exchange metadata
   * @then We see BTC-PERP and ETH-PERP markets configured
   *
   * Markets should include:
   * - BTC-PERP: Bitcoin perpetual with up to 50x leverage
   * - ETH-PERP: Ethereum perpetual with up to 50x leverage
   */
  test('should list available markets', async () => {
    const client = createReadOnlyClient();

    const meta = await client.info.getMeta();

    expect(meta.universe.length).toBeGreaterThanOrEqual(2);

    const marketNames = meta.universe.map((m) => m.name);
    expect(marketNames).toContain('BTC-PERP');
    expect(marketNames).toContain('ETH-PERP');
  });

  /**
   * @scenario Verify client authentication
   * @given A valid private key is provided
   * @when We create an authenticated client
   * @then The client has a valid address and signer
   *
   * Authentication is required for:
   * - Placing orders
   * - Canceling orders
   * - Transferring funds
   * - Updating leverage
   */
  test('should create authenticated client', () => {
    const client = createClient(TEST_ACCOUNTS.ALICE);

    expect(client.isAuthenticated).toBe(true);
    expect(client.address).toBe(TEST_ACCOUNTS.ALICE.address);
  });

  /**
   * @scenario Verify read-only client has no signer
   * @given No private key is provided
   * @when We create a read-only client
   * @then The client cannot sign transactions
   *
   * Read-only clients are useful for:
   * - Fetching market data
   * - Viewing order books
   * - Public account queries
   */
  test('should create unauthenticated client', () => {
    const client = createReadOnlyClient();

    expect(client.isAuthenticated).toBe(false);
    expect(client.address).toBeNull();
  });

  /**
   * @scenario Verify configuration is correct
   * @given Test environment variables are set
   * @when We check the configuration
   * @then All required settings are present
   */
  test('should have valid configuration', () => {
    expect(CONFIG.GATEWAY_URL).toBeTruthy();
    expect(CONFIG.WS_URL).toBeTruthy();
    expect(CONFIG.CHAIN_ID).toBe(1337);
    expect(CONFIG.TIMEOUT).toBeGreaterThan(0);
  });

  /**
   * @scenario Verify all test accounts are configured
   * @given Anvil test accounts are defined
   * @when We enumerate test accounts
   * @then Each account has valid address and private key
   *
   * These accounts are deterministic and funded on local Anvil.
   */
  test('should have valid test accounts', () => {
    const accounts = [
      TEST_ACCOUNTS.ALICE,
      TEST_ACCOUNTS.BOB,
      TEST_ACCOUNTS.CHARLIE,
      TEST_ACCOUNTS.DAVE,
      TEST_ACCOUNTS.EVE,
    ];

    for (const account of accounts) {
      expect(account.address).toMatch(/^0x[a-fA-F0-9]{40}$/);
      expect(account.privateKey).toMatch(/^0x[a-fA-F0-9]{64}$/);
    }
  });

  /**
   * @scenario Verify multiple clients can be created
   * @given Multiple test accounts exist
   * @when We create clients for different accounts
   * @then Each client has the correct address
   *
   * This validates that the SDK properly handles multiple instances.
   */
  test('should support multiple concurrent clients', () => {
    const alice = createClient(TEST_ACCOUNTS.ALICE);
    const bob = createClient(TEST_ACCOUNTS.BOB);
    const charlie = createClient(TEST_ACCOUNTS.CHARLIE);

    expect(alice.address).toBe(TEST_ACCOUNTS.ALICE.address);
    expect(bob.address).toBe(TEST_ACCOUNTS.BOB.address);
    expect(charlie.address).toBe(TEST_ACCOUNTS.CHARLIE.address);

    // Each client should be independent
    expect(alice.address).not.toBe(bob.address);
    expect(bob.address).not.toBe(charlie.address);
  });
});
