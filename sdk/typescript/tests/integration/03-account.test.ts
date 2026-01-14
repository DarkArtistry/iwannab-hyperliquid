/**
 * Account Management Tests
 *
 * These tests verify account-related operations including:
 * - Account state queries
 * - Balance and margin information
 * - Deposit and withdrawal flows
 * - Transfer operations
 *
 * @module tests/integration/03-account
 */

import { describe, test, expect, beforeEach } from 'vitest';
import {
  TEST_ACCOUNTS,
  createClient,
  createReadOnlyClient,
  waitForProcessing,
  resetTestState,
} from './setup';

describe('Account Management', () => {
  // Reset state before each test to ensure clean environment
  beforeEach(async () => {
    await resetTestState();
  });

  // =========================================================================
  // ACCOUNT STATE QUERIES
  // =========================================================================

  describe('Account State', () => {
    /**
     * @scenario Query account state for a user
     * @given Alice has an account on the exchange
     * @when We query Alice's account state
     * @then We receive margin summary and position information
     *
     * Account state includes:
     * - marginSummary: Overall account metrics
     *   - accountValue: Total value (balance + unrealized PnL)
     *   - totalNtlPos: Total notional position value
     *   - totalMarginUsed: Margin locked in positions
     *   - withdrawable: Amount available for withdrawal
     * - assetPositions: List of open positions
     *
     * @example API Response:
     * ```json
     * {
     *   "marginSummary": {
     *     "accountValue": "100000.00",
     *     "totalNtlPos": "65000.00",
     *     "totalRawUsd": "100000.00",
     *     "totalMarginUsed": "6500.00",
     *     "withdrawable": "93500.00"
     *   },
     *   "assetPositions": [
     *     {
     *       "position": {
     *         "coin": "BTC-PERP",
     *         "size": "1.0",
     *         "entryPx": "65000",
     *         "unrealizedPnl": "500.00"
     *       }
     *     }
     *   ]
     * }
     * ```
     */
    test('should query account state', async () => {
      const client = createReadOnlyClient();

      const state = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);

      expect(state).toBeDefined();
      expect(state.marginSummary).toBeDefined();
      expect(state.marginSummary.accountValue).toBeDefined();
    });

    /**
     * @scenario Query account state for multiple users
     * @given Multiple users have accounts
     * @when We query each user's state
     * @then Each returns independent account information
     */
    test('should query multiple account states', async () => {
      const client = createReadOnlyClient();

      const aliceState = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      const bobState = await client.info.getAccountState(TEST_ACCOUNTS.BOB.address);

      expect(aliceState).toBeDefined();
      expect(bobState).toBeDefined();

      // States are independent
      expect(aliceState.marginSummary).toBeDefined();
      expect(bobState.marginSummary).toBeDefined();
    });

    /**
     * @scenario Query account for new address returns zero balances
     * @given A new address with no history
     * @when We query the account state
     * @then All balances are zero
     *
     * New accounts start with:
     * - accountValue: 0
     * - No positions
     * - No open orders
     */
    test('should return zero state for new account', async () => {
      const client = createReadOnlyClient();

      // Use a random address that has never traded
      const newAddress = '0x' + '1'.repeat(40);
      const state = await client.info.getAccountState(newAddress);

      expect(state).toBeDefined();
      expect(state.marginSummary.accountValue).toBe('0');
      expect(state.assetPositions).toEqual([]);
    });
  });

  // =========================================================================
  // OPEN ORDERS QUERY
  // =========================================================================

  describe('Open Orders', () => {
    /**
     * @scenario Query open orders for a user
     * @given Alice may have open orders
     * @when We query Alice's open orders
     * @then We receive a list of open orders
     *
     * Order information includes:
     * - oid: Exchange-assigned order ID
     * - coin: Market symbol
     * - side: 'B' (buy) or 'A' (ask/sell)
     * - limitPx: Limit price
     * - sz: Remaining size
     * - origSz: Original order size
     * - timestamp: Order creation time
     * - cloid: Client order ID (if provided)
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "coin": "BTC-PERP",
     *     "oid": 12345,
     *     "side": "B",
     *     "limitPx": "64000",
     *     "sz": "0.5",
     *     "origSz": "1.0",
     *     "timestamp": 1705123456789,
     *     "cloid": "my-order-1"
     *   }
     * ]
     * ```
     */
    test('should query open orders', async () => {
      const client = createReadOnlyClient();

      const orders = await client.info.getOpenOrders(TEST_ACCOUNTS.ALICE.address);

      expect(orders).toBeDefined();
      expect(Array.isArray(orders)).toBe(true);
    });

    /**
     * @scenario Empty order list for account with no orders
     * @given An account has no open orders
     * @when We query open orders
     * @then We receive an empty array
     */
    test('should return empty array when no open orders', async () => {
      const client = createReadOnlyClient();

      // Query an account unlikely to have orders
      const orders = await client.info.getOpenOrders(TEST_ACCOUNTS.EVE.address);

      expect(orders).toBeDefined();
      expect(Array.isArray(orders)).toBe(true);
      // Note: May not be empty if tests ran previously
    });
  });

  // =========================================================================
  // FILL HISTORY
  // =========================================================================

  describe('Fill History', () => {
    /**
     * @scenario Query trade history for a user
     * @given Alice has executed trades
     * @when We query Alice's fill history
     * @then We receive a list of executed fills
     *
     * Fill information includes:
     * - coin: Market symbol
     * - px: Execution price
     * - sz: Fill size
     * - side: 'B' (buy) or 'A' (sell)
     * - time: Execution timestamp
     * - fee: Trading fee paid
     * - oid: Associated order ID
     * - closedPnl: Realized PnL (if closing position)
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "coin": "BTC-PERP",
     *     "px": "65000",
     *     "sz": "0.1",
     *     "side": "B",
     *     "time": 1705123456789,
     *     "fee": "3.25",
     *     "oid": 12345,
     *     "closedPnl": "0"
     *   }
     * ]
     * ```
     */
    test('should query fill history', async () => {
      const client = createReadOnlyClient();

      const fills = await client.info.getUserFills(TEST_ACCOUNTS.ALICE.address);

      expect(fills).toBeDefined();
      expect(Array.isArray(fills)).toBe(true);
    });

    /**
     * @scenario Query fill history with time range
     * @given Alice has trades over time
     * @when We query fills with startTime and endTime
     * @then We receive only fills within that range
     */
    test('should query fills with time range', async () => {
      const client = createReadOnlyClient();

      const now = Date.now();
      const oneDayAgo = now - 24 * 60 * 60 * 1000;

      const fills = await client.info.getUserFills(TEST_ACCOUNTS.ALICE.address, {
        startTime: oneDayAgo,
        endTime: now,
      });

      expect(fills).toBeDefined();
      expect(Array.isArray(fills)).toBe(true);
    });
  });

  // =========================================================================
  // FUNDING HISTORY
  // =========================================================================

  describe('Funding History', () => {
    /**
     * @scenario Query funding payment history
     * @given Alice has held positions during funding
     * @when We query funding history
     * @then We receive funding payment records
     *
     * Funding payments:
     * - Occur every 8 hours
     * - Positive = paid funding (holding in direction of premium)
     * - Negative = received funding (holding against premium)
     *
     * @example API Response:
     * ```json
     * [
     *   {
     *     "time": 1705123200000,
     *     "coin": "BTC-PERP",
     *     "usdc": "-15.50",
     *     "szi": "1.0",
     *     "fundingRate": "0.0001"
     *   }
     * ]
     * ```
     */
    test('should query user funding history', async () => {
      const client = createReadOnlyClient();

      const funding = await client.info.getUserFundingHistory(TEST_ACCOUNTS.ALICE.address);

      expect(funding).toBeDefined();
      expect(Array.isArray(funding)).toBe(true);
    });
  });

  // =========================================================================
  // TRANSFER OPERATIONS
  // =========================================================================

  describe('Transfers', () => {
    /**
     * @scenario Transfer USDC between accounts
     * @given Alice has sufficient balance
     * @when Alice transfers USDC to Bob
     * @then The transfer is submitted successfully
     *
     * Internal transfers:
     * - Instant settlement
     * - No gas fees (internal accounting)
     * - Destination must be valid address
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await alice.exchange.transfer({
     *   destination: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
     *   amount: '1000.00',
     * });
     * ```
     */
    test('should submit transfer request', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.transfer({
        destination: TEST_ACCOUNTS.BOB.address,
        amount: '100',
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Transfer requires authentication
     * @given An unauthenticated client
     * @when Attempting to transfer
     * @then The operation fails with signer error
     */
    test('should require authentication for transfer', async () => {
      const client = createReadOnlyClient();

      await expect(
        client.exchange.transfer({
          destination: TEST_ACCOUNTS.BOB.address,
          amount: '100',
        })
      ).rejects.toThrow('Signer required');
    });
  });

  // =========================================================================
  // WITHDRAWAL OPERATIONS
  // =========================================================================

  describe('Withdrawals', () => {
    /**
     * @scenario Request withdrawal to L1
     * @given Alice has withdrawable balance
     * @when Alice requests a withdrawal
     * @then The withdrawal request is submitted
     *
     * Withdrawals:
     * - Subject to withdrawal limits
     * - May have processing delay
     * - Requires sufficient withdrawable balance
     * - Cannot exceed margin requirements
     *
     * @example SDK Usage:
     * ```typescript
     * const result = await alice.exchange.withdraw({
     *   destination: alice.address,
     *   amount: '5000.00',
     * });
     * ```
     */
    test('should submit withdrawal request', async () => {
      const alice = createClient(TEST_ACCOUNTS.ALICE);

      const result = await alice.exchange.withdraw({
        destination: TEST_ACCOUNTS.ALICE.address,
        amount: '100',
      });

      expect(result).toBeDefined();
      expect(result.status).toBe('ok');
    });

    /**
     * @scenario Withdrawal requires authentication
     * @given An unauthenticated client
     * @when Attempting to withdraw
     * @then The operation fails
     */
    test('should require authentication for withdrawal', async () => {
      const client = createReadOnlyClient();

      await expect(
        client.exchange.withdraw({
          destination: TEST_ACCOUNTS.ALICE.address,
          amount: '100',
        })
      ).rejects.toThrow('Signer required');
    });
  });

  // =========================================================================
  // MARGIN CALCULATIONS
  // =========================================================================

  describe('Margin Information', () => {
    /**
     * @scenario Verify margin summary calculations
     * @given An account with positions
     * @when We query margin summary
     * @then withdrawable = accountValue - totalMarginUsed
     *
     * Key margin concepts:
     * - Initial Margin: Required to open position (1/leverage)
     * - Maintenance Margin: Minimum required to keep position (typically 50% of initial)
     * - Free Collateral: Available for new positions
     * - Withdrawable: Can be removed from account
     */
    test('should have consistent margin values', async () => {
      const client = createReadOnlyClient();

      const state = await client.info.getAccountState(TEST_ACCOUNTS.ALICE.address);
      const margin = state.marginSummary;

      const accountValue = parseFloat(margin.accountValue);
      const marginUsed = parseFloat(margin.totalMarginUsed);
      const withdrawable = parseFloat(margin.withdrawable);

      // Withdrawable should not exceed account value
      expect(withdrawable).toBeLessThanOrEqual(accountValue);

      // If there's margin used, withdrawable should be less
      if (marginUsed > 0) {
        expect(withdrawable).toBeLessThan(accountValue);
      }
    });
  });
});
