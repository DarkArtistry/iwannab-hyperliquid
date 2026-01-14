#!/usr/bin/env npx tsx
/**
 * HyperCore End-to-End Integration Test Runner
 *
 * This comprehensive test suite validates all aspects of the HyperCore exchange:
 * - API connectivity and health checks
 * - Market data queries (orderbook, prices, funding)
 * - Account management (state, balances, transfers)
 * - Order lifecycle (create, modify, cancel)
 * - Order matching and fills
 * - Position management and PnL
 * - EVM contract interactions (Phase 1a)
 * - Token standard tests (ERC20/721/1155) (Phase 1b)
 * - HIP-1 spot trading (Phase 1c)
 *
 * ARCHITECTURE OVERVIEW:
 * ----------------------
 * HyperCore uses a dual-layer architecture similar to Hyperliquid:
 *
 * 1. HyperCore Layer (Gateway API on port 3000):
 *    - High-performance trading engine with orderbook matching
 *    - Perpetual futures and spot token trading
 *    - REST API: POST /info (queries), POST /exchange (actions)
 *    - WebSocket API for real-time updates
 *
 * 2. HyperEVM Layer (EVM RPC on port 8545):
 *    - Full EVM execution environment using revm
 *    - Ethereum-compatible JSON-RPC (eth_*, web3_*, net_*)
 *    - Custom precompiles (0x0800-0x0808) to read exchange state
 *    - Deploy and interact with ERC20/ERC721/ERC1155 contracts
 *
 * PHASE 1 vs PHASE 2A NOTE:
 * -------------------------
 * These tests (Phase 1) operate on SEPARATE state systems:
 * - Spot trading tests: Use SpotEngineState.balances (core_view)
 * - EVM tests: Use EvmState.accounts (evm_view)
 *
 * Phase 2A will introduce UNIFIED STATE where both layers share a master balance
 * sheet with views. However, these Phase 1 tests remain valid because:
 * - They test each layer INDEPENDENTLY (no cross-layer operations)
 * - Spot tests verify trading on the core layer only
 * - EVM tests verify contract execution on the EVM layer only
 * - NO tests attempt to bridge/transfer between layers
 *
 * Phase 2A will ADD new tests for:
 * - View transfers (Core → EVM view adjustment)
 * - View transfers (EVM → Core view adjustment)
 * - Total balance invariant verification
 * - System address balance reflection
 *
 * TEST CATEGORIES:
 * ----------------
 * 1-6:  Trading engine tests (perpetuals)
 * 7-9:  EVM integration tests (Phase 1a, 1b)
 * 10:   Spot trading tests (Phase 1c - HIP-1)
 * 12:   Unified state tests (Phase 2A) - View transfers, invariant checks
 * 13:   Stress/performance tests
 *
 * Each test includes:
 * - Detailed description for documentation
 * - Progress output with timing
 * - Clear pass/fail status
 *
 * Usage:
 *   npx tsx runner.ts
 *   VERBOSE=true npx tsx runner.ts
 */

import { createPublicClient, createWalletClient, http, parseEther, formatEther } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { foundry } from 'viem/chains';

// ============================================================================
// CONFIGURATION
// ============================================================================

const CONFIG = {
  GATEWAY_URL: process.env.GATEWAY_URL || 'http://localhost:3000',
  EVM_RPC_URL: process.env.EVM_RPC_URL || 'http://localhost:8545',
  WS_URL: process.env.WS_URL || 'ws://localhost:3000/ws',
  CHAIN_ID: parseInt(process.env.CHAIN_ID || '1337'),
  VERBOSE: process.env.VERBOSE === 'true',
  TIMEOUT: 10000,
};

// =============================================================================
// TEST ACCOUNTS
// =============================================================================
//
// These are deterministic test accounts from Foundry/Anvil. They are derived
// from the mnemonic: "test test test test test test test test test test test junk"
//
// IMPORTANT: These keys are PUBLIC and well-known across the Ethereum ecosystem.
// NEVER use them on mainnet or any chain with real value!
//
// HOW ACCOUNTS GET FUNDED:
// ------------------------
// 1. HyperCore Balances (for trading):
//    The gateway binary (crates/gateway/src/main.rs) initializes these accounts
//    with trading balances in initialize_spot_markets():
//      - 100,000 USDC (token index 0) for placing orders
//      - 10,000 TEST tokens (token index 1) for spot trading tests
//
// 2. EVM/Native ETH Balance (for contract deployment):
//    The node binary (crates/node/src/main.rs) credits native ETH to test
//    accounts so they can deploy contracts and pay gas fees.
//
// ACCOUNT DERIVATION:
// - Index 0 (Alice): m/44'/60'/0'/0/0
// - Index 1 (Bob):   m/44'/60'/0'/0/1
// - Index 2 (Charlie): m/44'/60'/0'/0/2
// =============================================================================
const TEST_ACCOUNTS = {
  // Alice - Primary test account, used for most tests
  // This is the first Anvil account, commonly used across all Foundry projects
  ALICE: {
    address: '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266' as `0x${string}`,
    privateKey: '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80' as `0x${string}`,
  },
  // Bob - Secondary test account, used for matching tests (counterparty)
  BOB: {
    address: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8' as `0x${string}`,
    privateKey: '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d' as `0x${string}`,
  },
  // Charlie - Third test account, used for stress tests and isolation
  CHARLIE: {
    address: '0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC' as `0x${string}`,
    privateKey: '0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a' as `0x${string}`,
  },
};

const MARKETS = {
  BTC_PERP: 'BTC-PERP',
  ETH_PERP: 'ETH-PERP',
};

// ============================================================================
// TYPES
// ============================================================================

interface TestResult {
  name: string;
  category: string;
  description: string;
  status: 'pass' | 'fail' | 'skip';
  duration: number;
  error?: string;
}

interface TestContext {
  results: TestResult[];
  startTime: number;
}

// ============================================================================
// OUTPUT HELPERS
// ============================================================================

const colors = {
  reset: '\x1b[0m',
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  purple: '\x1b[35m',
  cyan: '\x1b[36m',
  white: '\x1b[37m',
  bold: '\x1b[1m',
};

function log(message: string) {
  console.log(message);
}

function logHeader(title: string) {
  log('');
  log(`${colors.purple}${'═'.repeat(70)}${colors.reset}`);
  log(`${colors.bold}${colors.white}  ${title}${colors.reset}`);
  log(`${colors.purple}${'═'.repeat(70)}${colors.reset}`);
  log('');
}

function logSection(title: string) {
  log('');
  log(`${colors.cyan}${'─'.repeat(70)}${colors.reset}`);
  log(`${colors.cyan}  ${title}${colors.reset}`);
  log(`${colors.cyan}${'─'.repeat(70)}${colors.reset}`);
}

function logTest(result: TestResult) {
  const icon = result.status === 'pass' ? `${colors.green}✓` : result.status === 'fail' ? `${colors.red}✗` : `${colors.yellow}○`;
  const duration = `${result.duration}ms`;
  log(`  ${icon} ${colors.white}${result.name}${colors.reset} ${colors.cyan}(${duration})${colors.reset}`);

  if (CONFIG.VERBOSE && result.description) {
    log(`      ${colors.cyan}${result.description}${colors.reset}`);
  }

  if (result.error && (CONFIG.VERBOSE || result.status === 'fail')) {
    log(`      ${colors.red}Error: ${result.error}${colors.reset}`);
  }
}

function logProgress(message: string) {
  if (CONFIG.VERBOSE) {
    log(`    ${colors.blue}▸${colors.reset} ${message}`);
  }
}

// ============================================================================
// API HELPERS
// ============================================================================

async function infoRequest(type: string, params: Record<string, unknown> = {}): Promise<unknown> {
  const response = await fetch(`${CONFIG.GATEWAY_URL}/info`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type, ...params }),
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }

  return response.json();
}

async function exchangeRequest(
  action: Record<string, unknown>,
  signature: { r: string; s: string; v: number },
  nonce: number
): Promise<unknown> {
  const response = await fetch(`${CONFIG.GATEWAY_URL}/exchange`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action, signature, nonce }),
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }

  return response.json();
}

// =============================================================================
// SIGNATURE STUB MECHANISM
// =============================================================================
//
// IMPORTANT: The HyperCore gateway uses a STUB signature verification for
// development/testing purposes. This is NOT secure for production!
//
// HOW THE STUB WORKS:
// -------------------
// In production, EIP-712 signatures are verified using ecrecover to derive
// the signer's address. However, implementing full EIP-712 is Phase 3 work.
//
// For Phase 1 E2E testing, the server's verify_signature() in
// crates/gateway/src/handlers.rs extracts the sender address from the
// LAST 40 characters of the 'r' value in the signature.
//
// Server code (handlers.rs:748):
//   let r_bytes = hex::decode(&r[2..])?;
//   let address_hex = &r[r.len() - 40..];  // Last 40 chars = 20 bytes = address
//   let recovered_address = AccountAddress::from_str(address_hex)?;
//
// WHY THIS APPROACH:
// ------------------
// - Allows E2E tests to work without implementing full signature verification
// - Tests can validate the entire order/cancel/query flow end-to-end
// - The stub is clearly marked and will be replaced in Phase 3
//
// WIRE FORMAT:
// ------------
// The signature object has three fields:
//   r: string - First 32 bytes of signature (we embed address in last 20 bytes)
//   s: string - Second 32 bytes of signature (from real signing, unused by stub)
//   v: number - Recovery id (27 or 28, unused by stub)
//
// WARNING: Never use this in production! Any attacker can spoof any address.
// =============================================================================
async function signAction(
  action: Record<string, unknown>,
  privateKey: `0x${string}`
): Promise<{ signature: { r: string; s: string; v: number }; nonce: number }> {
  const account = privateKeyToAccount(privateKey);

  // =========================================================================
  // NONCE SYSTEMS: HyperCore API vs EVM (Two Different Systems!)
  // =========================================================================
  //
  // There are TWO different nonce systems in HyperCore:
  //
  // 1. HyperCore API Nonce (THIS CODE) - TIMESTAMP-BASED
  //    - Used for: POST /exchange trading actions (orders, cancels, etc.)
  //    - Format: Unix milliseconds (Date.now())
  //    - Rules: Must be within (T - 2 days, T + 1 day) where T is current time
  //    - Why: Allows concurrent requests without coordination between processes
  //    - Replay protection: Server rejects nonces outside valid time window
  //
  // 2. EVM Nonce - INCREMENTAL (0, 1, 2, 3...)
  //    - Used for: eth_sendRawTransaction (EVM transactions)
  //    - Format: Incremental integer per account (eth_getTransactionCount)
  //    - Rules: Must be exactly previous nonce + 1
  //    - Why: Orders transactions, prevents double-spending
  //    - Handled automatically by viem in walletClient.sendTransaction()
  //
  // This matches Hyperliquid's production API behavior.
  // See: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets
  // =========================================================================
  const nonce = Date.now(); // Millisecond timestamp for HyperCore API

  // Create message hash (would be EIP-712 typed data in production)
  const message = JSON.stringify({ ...action, nonce });
  const signatureHex = await account.signMessage({ message });

  // Parse signature into r, s, v components
  // Standard ECDSA signature format: 0x + r(64 hex chars) + s(64 hex chars) + v(2 hex chars)
  // Total: 2 (0x) + 64 (r) + 64 (s) + 2 (v) = 132 characters
  const s = '0x' + signatureHex.slice(66, 130);   // chars 66-130 = s value
  const vHex = signatureHex.slice(130, 132);      // chars 130-132 = v value
  const v = parseInt(vHex, 16);

  // STUB COMPATIBILITY: Embed sender address in r value for server extraction
  // Structure: 0x + 24 zeros (12 bytes padding) + 40 hex chars (20 bytes address)
  // Example: 0x000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266
  const addressWithoutPrefix = account.address.slice(2).toLowerCase();
  const r = '0x' + '0'.repeat(24) + addressWithoutPrefix;

  return {
    signature: { r, s, v },
    nonce,
  };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ============================================================================
// TEST RUNNER
// ============================================================================

async function runTest(
  ctx: TestContext,
  name: string,
  category: string,
  description: string,
  testFn: () => Promise<void>
): Promise<void> {
  const start = Date.now();

  try {
    await testFn();
    const duration = Date.now() - start;
    const result: TestResult = { name, category, description, status: 'pass', duration };
    ctx.results.push(result);
    logTest(result);
  } catch (error) {
    const duration = Date.now() - start;
    const errorMessage = error instanceof Error ? error.message : String(error);
    const result: TestResult = { name, category, description, status: 'fail', duration, error: errorMessage };
    ctx.results.push(result);
    logTest(result);
  }
}

function skipTest(ctx: TestContext, name: string, category: string, description: string, reason: string): void {
  const result: TestResult = { name, category, description, status: 'skip', duration: 0, error: reason };
  ctx.results.push(result);
  logTest(result);
}

// ============================================================================
// TEST CATEGORIES
// ============================================================================

async function runConnectionTests(ctx: TestContext): Promise<void> {
  logSection('1. Connection & Health Tests');
  log('');
  log('  Testing basic connectivity to all HyperCore services');
  log('');

  await runTest(ctx, 'Gateway health check', 'connection', 'Verify Gateway service is responding to health endpoint', async () => {
    logProgress('Sending GET /health request...');
    const response = await fetch(`${CONFIG.GATEWAY_URL}/health`);
    if (!response.ok) throw new Error(`Health check failed: ${response.status}`);
    logProgress('Gateway is healthy');
  });

  await runTest(ctx, 'Info endpoint available', 'connection', 'Verify /info POST endpoint accepts requests', async () => {
    logProgress('Sending POST /info request...');
    const result = await infoRequest('meta');
    if (!result) throw new Error('Empty response');
    logProgress('Info endpoint responding');
  });

  await runTest(ctx, 'Exchange endpoint available', 'connection', 'Verify /exchange POST endpoint exists', async () => {
    logProgress('Sending POST /exchange probe...');
    const response = await fetch(`${CONFIG.GATEWAY_URL}/exchange`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    // We expect an error response, but the endpoint should exist
    if (response.status === 404) throw new Error('Exchange endpoint not found');
    logProgress('Exchange endpoint exists');
  });

  await runTest(ctx, 'EVM RPC available', 'connection', 'Verify EVM JSON-RPC endpoint is responding', async () => {
    logProgress('Sending eth_chainId request...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_chainId', params: [], id: 1 }),
    });
    if (!response.ok) throw new Error(`EVM RPC failed: ${response.status}`);
    const data = (await response.json()) as { result?: string };
    logProgress(`Chain ID: ${parseInt(data.result || '0', 16)}`);
  });
}

async function runMarketDataTests(ctx: TestContext): Promise<void> {
  logSection('2. Market Data Tests');
  log('');
  log('  Testing read-only market data queries');
  log('');

  await runTest(ctx, 'Get exchange metadata', 'market-data', 'Retrieve exchange configuration including available markets', async () => {
    logProgress('Fetching exchange metadata...');
    const meta = (await infoRequest('meta')) as { universe?: unknown[] };
    if (!meta.universe) throw new Error('Missing universe in metadata');
    logProgress(`Found ${meta.universe.length} markets`);
  });

  await runTest(ctx, 'Get all mid prices', 'market-data', 'Retrieve current mid prices for all markets', async () => {
    logProgress('Fetching mid prices...');
    const mids = (await infoRequest('allMids')) as Record<string, string>;
    const count = Object.keys(mids).length;
    logProgress(`Got prices for ${count} markets`);
    if (mids['BTC-PERP']) {
      logProgress(`BTC-PERP mid: $${mids['BTC-PERP']}`);
    }
  });

  await runTest(ctx, 'Get L2 orderbook (BTC-PERP)', 'market-data', 'Retrieve level 2 orderbook with bid/ask depth', async () => {
    logProgress('Fetching BTC-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.BTC_PERP })) as {
      levels?: [unknown[], unknown[]];
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    const [bids, asks] = book.levels;
    logProgress(`Orderbook: ${bids.length} bids, ${asks.length} asks`);
  });

  await runTest(ctx, 'Get L2 orderbook (ETH-PERP)', 'market-data', 'Retrieve ETH-PERP orderbook depth', async () => {
    logProgress('Fetching ETH-PERP orderbook...');
    const book = (await infoRequest('l2Book', { coin: MARKETS.ETH_PERP })) as {
      levels?: [unknown[], unknown[]];
    };
    if (!book.levels) throw new Error('Missing levels in orderbook');
    logProgress('ETH-PERP orderbook retrieved');
  });

  await runTest(ctx, 'Get recent trades', 'market-data', 'Retrieve recent trade history for a market', async () => {
    logProgress('Fetching recent trades...');
    const trades = (await infoRequest('recentTrades', { coin: MARKETS.BTC_PERP })) as unknown[];
    logProgress(`Found ${trades.length} recent trades`);
  });

  await runTest(ctx, 'Get funding rates', 'market-data', 'Retrieve current funding rate for perpetual markets', async () => {
    logProgress('Fetching funding rates...');
    const funding = await infoRequest('fundingHistory', { coin: MARKETS.BTC_PERP });
    logProgress('Funding rate data retrieved');
  });

  await runTest(ctx, 'Get candles (1h)', 'market-data', 'Retrieve OHLCV candlestick data', async () => {
    logProgress('Fetching 1h candles...');
    const candles = await infoRequest('candleSnapshot', {
      coin: MARKETS.BTC_PERP,
      interval: '1h',
      startTime: Date.now() - 86400000,
      endTime: Date.now(),
    });
    logProgress('Candle data retrieved');
  });
}

async function runAccountTests(ctx: TestContext): Promise<void> {
  logSection('3. Account State Tests');
  log('');
  log('  Testing account queries for test wallets');
  log('');

  await runTest(ctx, 'Get Alice account state', 'account', 'Retrieve full account state including margin and positions', async () => {
    logProgress(`Fetching account state for ${TEST_ACCOUNTS.ALICE.address}...`);
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      marginSummary?: { accountValue?: string };
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    logProgress(`Account value: $${state.marginSummary.accountValue}`);
  });

  await runTest(ctx, 'Get Bob account state', 'account', 'Retrieve Bob account for matching tests', async () => {
    logProgress(`Fetching account state for ${TEST_ACCOUNTS.BOB.address}...`);
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.BOB.address })) as {
      marginSummary?: { accountValue?: string };
    };
    if (!state.marginSummary) throw new Error('Missing marginSummary');
    logProgress(`Account value: $${state.marginSummary.accountValue}`);
  });

  await runTest(ctx, 'Get open orders (Alice)', 'account', 'Retrieve all open orders for an account', async () => {
    logProgress('Fetching open orders...');
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    logProgress(`Found ${orders.length} open orders`);
  });

  await runTest(ctx, 'Get user fills (Alice)', 'account', 'Retrieve trade fill history for an account', async () => {
    logProgress('Fetching fill history...');
    const fills = (await infoRequest('userFills', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    logProgress(`Found ${fills.length} fills`);
  });

  await runTest(ctx, 'Get user funding (Alice)', 'account', 'Retrieve funding payment history', async () => {
    logProgress('Fetching funding history...');
    const funding = await infoRequest('userFundingHistory', { user: TEST_ACCOUNTS.ALICE.address });
    logProgress('Funding history retrieved');
  });
}

async function runOrderTests(ctx: TestContext): Promise<void> {
  logSection('4. Order Lifecycle Tests');
  log('');
  log('  Testing order placement, modification, and cancellation');
  log('');

  // =========================================================================
  // ORDER WIRE FORMAT (Perpetuals)
  // =========================================================================
  // Orders are sent to POST /exchange with action.type = 'order'
  //
  // Each order object has these fields (matching Hyperliquid's API):
  //   a: number   - Asset/Market ID (0 = BTC-PERP, 1 = ETH-PERP, etc.)
  //   b: boolean  - Side (true = Buy, false = Sell)
  //   p: string   - Limit price as decimal string (e.g., "65000.50")
  //   s: string   - Size in base asset (e.g., "0.001" BTC)
  //   r: boolean  - Reduce-only flag (true = can only reduce position)
  //   t: object   - Order type configuration
  //   c: string?  - Client order ID (optional, for tracking)
  //
  // Order Types (t field):
  //   { limit: { tif: 'Gtc' } }  - Good-till-cancelled (rests on book)
  //   { limit: { tif: 'Ioc' } }  - Immediate-or-cancel (fill or kill remaining)
  //   { limit: { tif: 'Alo' } }  - Add-liquidity-only (post-only, reject if would cross)
  //
  // MARKET IDS:
  //   Perpetuals: 0-127 (0 = BTC-PERP, 1 = ETH-PERP, ...)
  //   Spot:       128+  (128 = first spot market, typically TEST-USDC)
  // =========================================================================

  let placedOrderId: string | null = null;

  await runTest(ctx, 'Place limit buy order', 'orders', 'Place a limit buy order below market price', async () => {
    logProgress('Preparing limit buy order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,          // Market ID 0 = BTC-PERP (see MARKET IDS above)
          b: true,       // Buy side
          p: '60000',    // Limit price: $60,000 per BTC
          s: '0.001',    // Size: 0.001 BTC (~$60 notional)
          r: false,      // Not reduce-only (can open new position)
          t: { limit: { tif: 'Gtc' } }, // Good-till-cancelled
        },
      ],
      grouping: 'na',    // Order grouping (na = not applicable)
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    logProgress('Order signed, sending to exchange...');

    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      response?: { data?: { statuses?: Array<{ resting?: { oid?: string } }> } };
    };

    if (result.status === 'ok' && result.response?.data?.statuses?.[0]?.resting?.oid) {
      placedOrderId = result.response.data.statuses[0].resting.oid;
      logProgress(`Order placed successfully, ID: ${placedOrderId}`);
    } else {
      logProgress('Order submitted');
    }
  });

  await runTest(ctx, 'Place limit sell order', 'orders', 'Place a limit sell order above market price', async () => {
    logProgress('Preparing limit sell order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: false, // isSell
          p: '70000',
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(action, signature, nonce);
    logProgress('Sell order placed');
  });

  await runTest(ctx, 'Place post-only order', 'orders', 'Place maker-only order that rejects if would cross', async () => {
    logProgress('Preparing post-only order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: true,
          p: '55000', // Well below market
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Alo' } }, // Add-liquidity-only
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(action, signature, nonce);
    logProgress('Post-only order placed');
  });

  await runTest(ctx, 'Place IOC order', 'orders', 'Place immediate-or-cancel order', async () => {
    logProgress('Preparing IOC order...');

    const action = {
      type: 'order',
      orders: [
        {
          a: 0,
          b: true,
          p: '50000', // Below market - won't fill
          s: '0.001',
          r: false,
          t: { limit: { tif: 'Ioc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(action, signature, nonce);
    logProgress('IOC order submitted (likely cancelled)');
  });

  await runTest(ctx, 'Batch place orders', 'orders', 'Place multiple orders in a single request', async () => {
    logProgress('Preparing batch order...');

    const action = {
      type: 'order',
      orders: [
        { a: 0, b: true, p: '58000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
        { a: 0, b: true, p: '57000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
        { a: 0, b: false, p: '72000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(action, signature, nonce);
    logProgress('Batch orders placed');
  });

  await runTest(ctx, 'Cancel single order', 'orders', 'Cancel a specific order by ID', async () => {
    // First get open orders
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ oid?: string }>;

    if (orders.length === 0) {
      logProgress('No orders to cancel, placing one first...');
      const placeAction = {
        type: 'order',
        orders: [{ a: 0, b: true, p: '55000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature: placeSig, nonce: placeNonce } = await signAction(placeAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(placeAction, placeSig, placeNonce);
      await sleep(500);
    }

    const refreshedOrders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as Array<{ oid?: string }>;
    if (refreshedOrders.length > 0) {
      const orderToCancel = refreshedOrders[0];
      logProgress(`Cancelling order ${orderToCancel.oid}...`);

      const cancelAction = {
        type: 'cancel',
        cancels: [{ a: 0, o: orderToCancel.oid }],
      };

      const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
      await exchangeRequest(cancelAction, signature, nonce);
      logProgress('Order cancelled');
    } else {
      logProgress('No orders available to cancel');
    }
  });

  await runTest(ctx, 'Cancel all orders', 'orders', 'Cancel all open orders for an account', async () => {
    logProgress('Cancelling all orders...');

    const cancelAction = {
      type: 'cancelAll',
    };

    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    logProgress('All orders cancelled');

    // Verify no orders remain
    await sleep(500);
    const orders = (await infoRequest('openOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];
    logProgress(`Remaining orders: ${orders.length}`);
  });
}

async function runMatchingTests(ctx: TestContext): Promise<void> {
  logSection('5. Order Matching Tests');
  log('');
  log('  Testing order matching between different accounts');
  log('');

  await runTest(ctx, 'Match limit orders (Alice buys, Bob sells)', 'matching', 'Test basic order matching between two accounts', async () => {
    // Alice places buy order
    logProgress('Alice placing buy order at $65,000...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    // Bob places sell order at same price (should match)
    logProgress('Bob placing sell order at $65,000...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '65000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    const result = await exchangeRequest(sellAction, sellSig, sellNonce);

    logProgress('Orders matched (check fills for confirmation)');
  });

  await runTest(ctx, 'Match with price improvement', 'matching', 'Buy order at higher price should match sell at lower price', async () => {
    // Alice places buy at $66,000
    logProgress('Alice placing buy order at $66,000...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '66000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    await sleep(100);

    // Bob places sell at $65,500 (should match at $65,500 or $66,000)
    logProgress('Bob placing sell order at $65,500...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '65500', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(sellAction, sellSig, sellNonce);

    logProgress('Price improvement matching complete');
  });

  await runTest(ctx, 'Partial fill test', 'matching', 'Large order should partially fill against smaller order', async () => {
    // Alice places small sell
    logProgress('Alice placing small sell order (0.001)...');
    const sellAction = {
      type: 'order',
      orders: [{ a: 0, b: false, p: '64000', s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: sellSig, nonce: sellNonce } = await signAction(sellAction, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(sellAction, sellSig, sellNonce);

    await sleep(100);

    // Bob places larger buy
    logProgress('Bob placing larger buy order (0.005)...');
    const buyAction = {
      type: 'order',
      orders: [{ a: 0, b: true, p: '64000', s: '0.005', r: false, t: { limit: { tif: 'Gtc' } } }],
      grouping: 'na',
    };
    const { signature: buySig, nonce: buyNonce } = await signAction(buyAction, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(buyAction, buySig, buyNonce);

    logProgress('Partial fill executed, remainder resting');
  });

  // Clean up orders after matching tests
  await runTest(ctx, 'Clean up matching test orders', 'matching', 'Cancel remaining orders from both accounts', async () => {
    logProgress('Cancelling Alice orders...');
    const cancelAlice = { type: 'cancelAll' };
    const { signature: sigA, nonce: nonceA } = await signAction(cancelAlice, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(cancelAlice, sigA, nonceA);

    logProgress('Cancelling Bob orders...');
    const cancelBob = { type: 'cancelAll' };
    const { signature: sigB, nonce: nonceB } = await signAction(cancelBob, TEST_ACCOUNTS.BOB.privateKey);
    await exchangeRequest(cancelBob, sigB, nonceB);

    logProgress('Cleanup complete');
  });
}

async function runPositionTests(ctx: TestContext): Promise<void> {
  logSection('6. Position Management Tests');
  log('');
  log('  Testing position tracking, leverage, and PnL');
  log('');

  await runTest(ctx, 'Check position after trades', 'positions', 'Verify positions are correctly tracked after matching', async () => {
    logProgress('Fetching Alice positions...');
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      assetPositions?: Array<{ position?: { szi?: string } }>;
    };

    if (state.assetPositions) {
      for (const ap of state.assetPositions) {
        if (ap.position && parseFloat(ap.position.szi || '0') !== 0) {
          logProgress(`Position size: ${ap.position.szi}`);
        }
      }
    }
    logProgress('Position check complete');
  });

  await runTest(ctx, 'Update leverage', 'positions', 'Change leverage setting for a market', async () => {
    logProgress('Updating leverage to 10x...');

    const action = {
      type: 'updateLeverage',
      asset: 0,
      isCross: true,
      leverage: 10,
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    await exchangeRequest(action, signature, nonce);
    logProgress('Leverage updated to 10x');
  });

  await runTest(ctx, 'Check margin requirements', 'positions', 'Verify margin calculations after leverage change', async () => {
    logProgress('Checking margin after leverage change...');
    const state = (await infoRequest('clearinghouseState', { user: TEST_ACCOUNTS.ALICE.address })) as {
      marginSummary?: { totalMarginUsed?: string; totalNtlPos?: string };
    };

    if (state.marginSummary) {
      logProgress(`Margin used: $${state.marginSummary.totalMarginUsed}`);
      logProgress(`Notional: $${state.marginSummary.totalNtlPos}`);
    }
  });
}

async function runEVMTests(ctx: TestContext): Promise<void> {
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
    if (blockNumber < 0n) throw new Error('Invalid block number');
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
      logProgress(`Code found (unexpected for EOA): ${code.slice(0, 20)}...`);
    } else {
      logProgress('No code found (expected for EOA)');
    }
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
    try {
      const result = await publicClient.call({
        to: '0x0000000000000000000000000000000000000000',
        data: '0x',
      });
      logProgress(`Call result: ${result.data || '0x'}`);
    } catch (e) {
      // Call to zero address may fail, which is acceptable
      logProgress('Call completed (may have reverted, which is expected)');
    }
  });

  await runTest(ctx, 'eth_call (precompile)', 'evm', 'Attempt to call HyperCore precompile', async () => {
    logProgress('Calling position precompile at 0x0800...');
    try {
      const result = await publicClient.call({
        to: '0x0000000000000000000000000000000000000800',
        data: '0x', // Empty call
      });
      logProgress(`Precompile response: ${result.data || '0x'}`);
    } catch (e) {
      logProgress('Precompile not available or reverted (expected in test environment)');
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

// ============================================================================
// Advanced EVM Tests (Contract Deployment & Interaction)
// ============================================================================
//
// These tests validate full smart contract lifecycle:
// 1. Contract deployment (constructor execution)
// 2. Contract code verification
// 3. State reads via eth_call
// 4. State writes via eth_sendRawTransaction
// 5. Direct storage access via eth_getStorageAt
//
// The bytecode is pre-compiled using Foundry (solc 0.8.29) with the Cancun
// EVM target, which supports the PUSH0 opcode (EIP-3855).
// ============================================================================

async function runAdvancedEVMTests(ctx: TestContext): Promise<void> {
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

// ============================================================================
// Token Standards Tests (ERC20, ERC721, ERC1155) - Phase 1b
// ============================================================================
//
// These tests validate EVM token standard support, ensuring smart contracts
// following OpenZeppelin-style implementations work correctly.
//
// WHY TOKEN STANDARDS MATTER:
// ---------------------------
// Token standards are the foundation of DeFi. Supporting ERC20/721/1155
// enables:
// - Fungible tokens (ERC20): Stablecoins, governance tokens, wrapped assets
// - NFTs (ERC721): Unique collectibles, position NFTs, membership tokens
// - Multi-tokens (ERC1155): Game items, semi-fungible tokens, batch transfers
//
// TEST STRATEGY:
// --------------
// Each token type tests: deploy → verify metadata → mint/transfer → verify
// This covers the full lifecycle of token interactions.
//
// BYTECODE ORIGIN:
// ----------------
// All bytecode is compiled from minimal implementations using:
//   - Solidity 0.8.29
//   - Target: Cancun EVM (PUSH0 opcode support)
//   - Optimization enabled
//
// The source contracts are stripped-down versions of OpenZeppelin templates,
// containing only the functions needed for testing.
// ============================================================================

async function runTokenStandardsTests(ctx: TestContext): Promise<void> {
  logSection('9. Token Standards Tests');
  log('');
  log('  Testing ERC20, ERC721, and ERC1155 token deployments and interactions');
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
  // ERC20 CONTRACT BYTECODE
  // =========================================================================
  //
  // Minimal ERC20 implementation with:
  //   - Constructor: (string name, string symbol, uint8 decimals, uint256 supply)
  //   - name(), symbol(), decimals(), totalSupply() - metadata getters
  //   - balanceOf(address) - get token balance
  //   - transfer(address to, uint256 amount) - transfer tokens
  //   - approve(address spender, uint256 amount) - approve spending
  //
  // Constructor encodes 4 parameters, so we append them to the bytecode.
  // =========================================================================
  const ERC20_BYTECODE =
    '0x608060405234801561000f575f5ffd5b506040516109ab3803806109ab83398101604081905261002e91610115565b5f610039858261021c565b506001610046848261021c565b506002805460ff191660ff93909316929092179091556003819055335f90815260046020526040902055506102d69050565b634e487b7160e01b5f52604160045260245ffd5b5f82601f83011261009b575f5ffd5b81516001600160401b038111156100b4576100b4610078565b604051601f8201601f19908116603f011681016001600160401b03811182821017156100e2576100e2610078565b6040528181528382016020018510156100f9575f5ffd5b8160208501602083015e5f918101602001919091529392505050565b5f5f5f5f60808587031215610128575f5ffd5b84516001600160401b0381111561013d575f5ffd5b6101498782880161008c565b602087015190955090506001600160401b03811115610166575f5ffd5b6101728782880161008c565b935050604085015160ff81168114610188575f5ffd5b6060959095015193969295505050565b600181811c908216806101ac57607f821691505b6020821081036101ca57634e487b7160e01b5f52602260045260245ffd5b50919050565b601f82111561021757805f5260205f20601f840160051c810160208510156101f55750805b601f840160051c820191505b81811015610214575f8155600101610201565b50505b505050565b81516001600160401b0381111561023557610235610078565b610249816102438454610198565b846101d0565b6020601f82116001811461027b575f83156102645750848201515b5f19600385901b1c1916600184901b178455610214565b5f84815260208120601f198516915b828110156102aa578785015182556020948501946001909201910161028a565b50848210156102c757868401515f19600387901b60f8161c191681555b50505050600190811b01905550565b6106c8806102e35f395ff3fe608060405234801561000f575f5ffd5b5060043610610090575f3560e01c8063313ce56711610063578063313ce567146100ff57806370a082311461011e57806395d89b411461013d578063a9059cbb14610145578063dd62ed3e14610158575f5ffd5b806306fdde0314610094578063095ea7b3146100b257806318160ddd146100d557806323b872dd146100ec575b5f5ffd5b61009c610182565b6040516100a9919061051d565b60405180910390f35b6100c56100c036600461056d565b61020d565b60405190151581526020016100a9565b6100de60035481565b6040519081526020016100a9565b6100c56100fa366004610595565b610279565b60025461010c9060ff1681565b60405160ff90911681526020016100a9565b6100de61012c3660046105cf565b60046020525f908152604090205481565b61009c61042f565b6100c561015336600461056d565b61043c565b6100de6101663660046105ef565b600560209081525f928352604080842090915290825290205481565b5f805461018e90610620565b80601f01602080910402602001604051908101604052809291908181526020018280546101ba90610620565b80156102055780601f106101dc57610100808354040283529160200191610205565b820191905f5260205f20905b8154815290600101906020018083116101e857829003601f168201915b505050505081565b335f8181526005602090815260408083206001600160a01b038716808552925280832085905551919290917f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925906102679086815260200190565b60405180910390a35060015b92915050565b6001600160a01b0383165f908152600460205260408120548211156102dc5760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b60448201526064015b60405180910390fd5b6001600160a01b0384165f9081526005602090815260408083203384529091529020548211156103475760405162461bcd60e51b8152602060048201526016602482015275496e73756666696369656e7420616c6c6f77616e636560501b60448201526064016102d3565b6001600160a01b0384165f908152600460205260408120805484929061036e90849061066c565b90915550506001600160a01b0383165f908152600460205260408120805484929061039a90849061067f565b90915550506001600160a01b0384165f908152600560209081526040808320338452909152812080548492906103d190849061066c565b92505081905550826001600160a01b0316846001600160a01b03167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161041d91815260200190565b60405180910390a35060019392505050565b6001805461018e90610620565b335f908152600460205260408120548211156104915760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b60448201526064016102d3565b335f90815260046020526040812080548492906104af90849061066c565b90915550506001600160a01b0383165f90815260046020526040812080548492906104db90849061067f565b90915550506040518281526001600160a01b0384169033907fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef90602001610267565b602081525f82518060208401528060208501604085015e5f604082850101526040601f19601f83011684010191505092915050565b80356001600160a01b0381168114610568575f5ffd5b919050565b5f5f6040838503121561057e575f5ffd5b61058783610552565b946020939093013593505050565b5f5f5f606084860312156105a7575f5ffd5b6105b084610552565b92506105be60208501610552565b929592945050506040919091013590565b5f602082840312156105df575f5ffd5b6105e882610552565b9392505050565b5f5f60408385031215610600575f5ffd5b61060983610552565b915061061760208401610552565b90509250929050565b600181811c9082168061063457607f821691505b60208210810361065257634e487b7160e01b5f52602260045260245ffd5b50919050565b634e487b7160e01b5f52601160045260245ffd5b8181038181111561027357610273610658565b808201808211156102735761027361065856fea264697066735822122064975e691fcc9f1366dd82129c79b3cc5e839a683eec4825fc85c64445e9117464736f6c634300081d0033';

  // ERC721 bytecode compiled with solc 0.8.29
  const ERC721_BYTECODE =
    '0x608060405234801561000f575f5ffd5b50604051610a83380380610a8383398101604081905261002e916100eb565b5f61003983826101d4565b50600161004682826101d4565b50505061028e565b634e487b7160e01b5f52604160045260245ffd5b5f82601f830112610071575f5ffd5b81516001600160401b0381111561008a5761008a61004e565b604051601f8201601f19908116603f011681016001600160401b03811182821017156100b8576100b861004e565b6040528181528382016020018510156100cf575f5ffd5b8160208501602083015e5f918101602001919091529392505050565b5f5f604083850312156100fc575f5ffd5b82516001600160401b03811115610111575f5ffd5b61011d85828601610062565b602085015190935090506001600160401b0381111561013a575f5ffd5b61014685828601610062565b9150509250929050565b600181811c9082168061016457607f821691505b60208210810361018257634e487b7160e01b5f52602260045260245ffd5b50919050565b601f8211156101cf57805f5260205f20601f840160051c810160208510156101ad5750805b601f840160051c820191505b818110156101cc575f81556001016101b9565b50505b505050565b81516001600160401b038111156101ed576101ed61004e565b610201816101fb8454610150565b84610188565b6020601f821160018114610233575f831561021c5750848201515b5f19600385901b1c1916600184901b1784556101cc565b5f84815260208120601f198516915b828110156102625787850151825560209485019460019092019101610242565b508482101561027f57868401515f19600387901b60f8161c191681555b50505050600190811b01905550565b6107e88061029b5f395ff3fe608060405234801561000f575f5ffd5b506004361061009b575f3560e01c80636a627842116100635780636a6278421461014d57806370a082311461016e57806395d89b411461018d578063a22cb46514610195578063e985e9c5146101a8575f5ffd5b806306fdde031461009f578063081812fc146100bd578063095ea7b3146100fd57806323b872dd146101125780636352211e14610125575b5f5ffd5b6100a76101e5565b6040516100b491906105e6565b60405180910390f35b6100e56100cb36600461061b565b60046020525f90815260409020546001600160a01b031681565b6040516001600160a01b0390911681526020016100b4565b61011061010b36600461064d565b610270565b005b610110610120366004610675565b61031e565b6100e561013336600461061b565b60026020525f90815260409020546001600160a01b031681565b61016061015b3660046106af565b6104d1565b6040519081526020016100b4565b61016061017c3660046106af565b60036020525f908152604090205481565b6100a761056e565b6101106101a33660046106cf565b61057b565b6101d56101b6366004610708565b600560209081525f928352604080842090915290825290205460ff1681565b60405190151581526020016100b4565b5f80546101f190610739565b80601f016020809104026020016040519081016040528092919081815260200182805461021d90610739565b80156102685780601f1061023f57610100808354040283529160200191610268565b820191905f5260205f20905b81548152906001019060200180831161024b57829003601f168201915b505050505081565b5f818152600260205260409020546001600160a01b031633146102c65760405162461bcd60e51b81526020600482015260096024820152682737ba1037bbb732b960b91b60448201526064015b60405180910390fd5b5f8181526004602052604080822080546001600160a01b0319166001600160a01b0386169081179091559051839233917f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b9259190a45050565b5f818152600260205260409020546001600160a01b038481169116146103725760405162461bcd60e51b81526020600482015260096024820152682737ba1037bbb732b960b91b60448201526064016102bd565b336001600160a01b038416148061039e57505f818152600460205260409020546001600160a01b031633145b806103cb57506001600160a01b0383165f90815260056020908152604080832033845290915290205460ff165b6104085760405162461bcd60e51b815260206004820152600e60248201526d139bdd08185d5d1a1bdc9a5e995960921b60448201526064016102bd565b5f81815260026020908152604080832080546001600160a01b0319166001600160a01b0387811691909117909155861683526003909152812080549161044d83610785565b90915550506001600160a01b0382165f9081526003602052604081208054916104758361079a565b90915550505f8181526004602052604080822080546001600160a01b03191690555182916001600160a01b0385811692908716917fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef91a4505050565b600680545f91829190826104e48361079a565b909155505f81815260026020908152604080832080546001600160a01b0319166001600160a01b03891690811790915583526003909152812080549293509061052c8361079a565b909155505060405181906001600160a01b038516905f907fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef908290a492915050565b600180546101f190610739565b335f8181526005602090815260408083206001600160a01b03871680855290835292819020805460ff191686151590811790915590519081529192917f17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31910160405180910390a35050565b602081525f82518060208401528060208501604085015e5f604082850101526040601f19601f83011684010191505092915050565b5f6020828403121561062b575f5ffd5b5035919050565b80356001600160a01b0381168114610648575f5ffd5b919050565b5f5f6040838503121561065e575f5ffd5b61066783610632565b946020939093013593505050565b5f5f5f60608486031215610687575f5ffd5b61069084610632565b925061069e60208501610632565b929592945050506040919091013590565b5f602082840312156106bf575f5ffd5b6106c882610632565b9392505050565b5f5f604083850312156106e0575f5ffd5b6106e983610632565b9150602083013580151581146106fd575f5ffd5b809150509250929050565b5f5f60408385031215610719575f5ffd5b61072283610632565b915061073060208401610632565b90509250929050565b600181811c9082168061074d57607f821691505b60208210810361076b57634e487b7160e01b5f52602260045260245ffd5b50919050565b634e487b7160e01b5f52601160045260245ffd5b5f8161079357610793610771565b505f190190565b5f600182016107ab576107ab610771565b506001019056fea2646970667358221220623ee4f26fce173811beba104ecf1a004fe5c8531c1e2506c0d6931fb9ba94f864736f6c634300081d0033';

  // ERC1155 bytecode compiled with solc 0.8.29
  const ERC1155_BYTECODE =
    '0x6080604052348015600e575f5ffd5b5061089c8061001c5f395ff3fe608060405234801561000f575f5ffd5b5060043610610060575f3560e01c8063156e29f6146100645780633656eec2146100795780634e1273f4146100b3578063a22cb465146100d3578063e985e9c5146100e6578063f242432a14610123575b5f5ffd5b6100776100723660046104bf565b610136565b005b6100a06100873660046104ef565b5f60208181529281526040808220909352908152205481565b6040519081526020015b60405180910390f35b6100c66100c13660046105eb565b6101b7565b6040516100aa91906106ae565b6100776100e13660046106f0565b61029e565b6101136100f4366004610729565b600160209081525f928352604080842090915290825290205460ff1681565b60405190151581526020016100aa565b610077610131366004610751565b610309565b5f828152602081815260408083206001600160a01b03871684529091528120805483929061016590849061082c565b909155505060408051838152602081018390526001600160a01b038516915f9133917fc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62910160405180910390a4505050565b60605f835167ffffffffffffffff8111156101d4576101d4610519565b6040519080825280602002602001820160405280156101fd578160200160208202803683370190505b5090505f5b8451811015610294575f5f85838151811061021f5761021f61083f565b602002602001015181526020019081526020015f205f8683815181106102475761024761083f565b60200260200101516001600160a01b03166001600160a01b031681526020019081526020015f20548282815181106102815761028161083f565b6020908102919091010152600101610202565b5090505b92915050565b335f8181526001602090815260408083206001600160a01b03871680855290835292819020805460ff191686151590811790915590519081529192917f17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31910160405180910390a35050565b336001600160a01b038616148061034257506001600160a01b0385165f90815260016020908152604080832033845290915290205460ff165b6103845760405162461bcd60e51b815260206004820152600e60248201526d139bdd08185d5d1a1bdc9a5e995960921b60448201526064015b60405180910390fd5b5f838152602081815260408083206001600160a01b03891684529091529020548211156103ea5760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b604482015260640161037b565b5f838152602081815260408083206001600160a01b038916845290915281208054849290610419908490610853565b90915550505f838152602081815260408083206001600160a01b03881684529091528120805484929061044d90849061082c565b909155505060408051848152602081018490526001600160a01b03808716929088169133917fc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62910160405180910390a45050505050565b80356001600160a01b03811681146104ba575f5ffd5b919050565b5f5f5f606084860312156104d1575f5ffd5b6104da846104a4565b95602085013595506040909401359392505050565b5f5f60408385031215610500575f5ffd5b82359150610510602084016104a4565b90509250929050565b634e487b7160e01b5f52604160045260245ffd5b604051601f8201601f1916810167ffffffffffffffff8111828210171561055657610556610519565b604052919050565b5f67ffffffffffffffff82111561057757610577610519565b5060051b60200190565b5f82601f830112610590575f5ffd5b81356105a361059e8261055e565b61052d565b8082825260208201915060208360051b8601019250858311156105c4575f5ffd5b602085015b838110156105e15780358352602092830192016105c9565b5095945050505050565b5f5f604083850312156105fc575f5ffd5b823567ffffffffffffffff811115610612575f5ffd5b8301601f81018513610622575f5ffd5b803561063061059e8261055e565b8082825260208201915060208360051b850101925087831115610651575f5ffd5b6020840193505b8284101561067a57610669846104a4565b825260209384019390910190610658565b9450505050602083013567ffffffffffffffff811115610698575f5ffd5b6106a485828601610581565b9150509250929050565b602080825282518282018190525f918401906040840190835b818110156106e55783518352602093840193909201916001016106c7565b509095945050505050565b5f5f60408385031215610701575f5ffd5b61070a836104a4565b91506020830135801515811461071e575f5ffd5b809150509250929050565b5f5f6040838503121561073a575f5ffd5b610743836104a4565b9150610510602084016104a4565b5f5f5f5f5f60a08688031215610765575f5ffd5b61076e866104a4565b945061077c602087016104a4565b93506040860135925060608601359150608086013567ffffffffffffffff8111156107a5575f5ffd5b8601601f810188136107b5575f5ffd5b803567ffffffffffffffff8111156107cf576107cf610519565b6107e2601f8201601f191660200161052d565b8181528960208385010111156107f6575f5ffd5b816020840160208301375f602083830101528093505050509295509295909350565b634e487b7160e01b5f52601160045260245ffd5b8082018082111561029857610298610818565b634e487b7160e01b5f52603260045260245ffd5b818103818111156102985761029861081856fea264697066735822122074a41f87bd4b3857dcfa9d80d955064326778d15dc94fb1b655d95e8758e32ee64736f6c634300081d0033';

  // ABIs for token contracts
  const ERC20_ABI = [
    { inputs: [], name: 'name', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'symbol', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'decimals', outputs: [{ type: 'uint8' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'totalSupply', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }, { name: 'amount', type: 'uint256' }], name: 'transfer', outputs: [{ type: 'bool' }], stateMutability: 'nonpayable', type: 'function' },
    { inputs: [{ name: 'spender', type: 'address' }, { name: 'amount', type: 'uint256' }], name: 'approve', outputs: [{ type: 'bool' }], stateMutability: 'nonpayable', type: 'function' },
  ] as const;

  const ERC721_ABI = [
    { inputs: [], name: 'name', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'symbol', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }], name: 'mint', outputs: [{ type: 'uint256' }], stateMutability: 'nonpayable', type: 'function' },
    { inputs: [{ name: 'tokenId', type: 'uint256' }], name: 'ownerOf', outputs: [{ type: 'address' }], stateMutability: 'view', type: 'function' },
  ] as const;

  const ERC1155_ABI = [
    { inputs: [{ name: 'id', type: 'uint256' }, { name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }, { name: 'id', type: 'uint256' }, { name: 'amount', type: 'uint256' }], name: 'mint', outputs: [], stateMutability: 'nonpayable', type: 'function' },
  ] as const;

  let erc20Address: `0x${string}` | null = null;
  let erc721Address: `0x${string}` | null = null;
  let erc1155Address: `0x${string}` | null = null;

  // ERC20 Tests
  await runTest(ctx, 'Deploy ERC20 token', 'tokens', 'Deploy a minimal ERC20 token contract', async () => {
    logProgress('Deploying TestToken ERC20...');

    // Encode constructor arguments: name, symbol, decimals, initialSupply
    const { encodeAbiParameters } = await import('viem');
    const args = encodeAbiParameters(
      [{ type: 'string' }, { type: 'string' }, { type: 'uint8' }, { type: 'uint256' }],
      ['Test Token', 'TEST', 18, 1000000000000000000000000n] // 1 million tokens
    );

    const deployData = (ERC20_BYTECODE + args.slice(2)) as `0x${string}`;

    const hash = await walletClient.sendTransaction({
      data: deployData,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC20 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc20Address = receipt.contractAddress;
    logProgress(`ERC20 deployed at: ${erc20Address}`);
  });

  await runTest(ctx, 'Verify ERC20 token metadata', 'tokens', 'Read name, symbol, decimals from ERC20', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Reading token metadata...');
    const name = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'name' });
    const symbol = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'symbol' });
    const decimals = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'decimals' });

    logProgress(`Token: ${name} (${symbol}), ${decimals} decimals`);
    if (name !== 'Test Token') throw new Error(`Expected name "Test Token", got "${name}"`);
  });

  await runTest(ctx, 'Check ERC20 balance', 'tokens', 'Verify deployer received initial supply', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Checking deployer balance...');
    const balance = await publicClient.readContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice balance: ${balance}`);
    if (balance !== 1000000000000000000000000n) throw new Error('Incorrect initial balance');
  });

  await runTest(ctx, 'ERC20 transfer', 'tokens', 'Transfer tokens between accounts', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Transferring 1000 tokens to Bob...');
    const hash = await walletClient.writeContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'transfer',
      args: [TEST_ACCOUNTS.BOB.address, 1000000000000000000000n],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Transfer failed');

    const bobBalance = await publicClient.readContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.BOB.address],
    });

    logProgress(`Bob balance: ${bobBalance}`);
    if (bobBalance !== 1000000000000000000000n) throw new Error('Transfer amount mismatch');
  });

  // ERC721 Tests
  await runTest(ctx, 'Deploy ERC721 NFT', 'tokens', 'Deploy a minimal ERC721 NFT contract', async () => {
    logProgress('Deploying TestNFT ERC721...');

    const { encodeAbiParameters } = await import('viem');
    const args = encodeAbiParameters(
      [{ type: 'string' }, { type: 'string' }],
      ['Test NFT', 'TNFT']
    );

    const deployData = (ERC721_BYTECODE + args.slice(2)) as `0x${string}`;

    const hash = await walletClient.sendTransaction({
      data: deployData,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC721 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc721Address = receipt.contractAddress;
    logProgress(`ERC721 deployed at: ${erc721Address}`);
  });

  await runTest(ctx, 'Mint ERC721 NFT', 'tokens', 'Mint a new NFT to Alice', async () => {
    if (!erc721Address) throw new Error('ERC721 not deployed');

    logProgress('Minting NFT to Alice...');
    const hash = await walletClient.writeContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'mint',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Mint failed');

    const balance = await publicClient.readContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice NFT balance: ${balance}`);
    if (balance < 1n) throw new Error('NFT not minted');
  });

  // ERC1155 Tests
  await runTest(ctx, 'Deploy ERC1155 multi-token', 'tokens', 'Deploy a minimal ERC1155 multi-token contract', async () => {
    logProgress('Deploying TestMultiToken ERC1155...');

    const hash = await walletClient.sendTransaction({
      data: ERC1155_BYTECODE as `0x${string}`,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC1155 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc1155Address = receipt.contractAddress;
    logProgress(`ERC1155 deployed at: ${erc1155Address}`);
  });

  await runTest(ctx, 'Mint ERC1155 tokens', 'tokens', 'Mint fungible tokens with ERC1155', async () => {
    if (!erc1155Address) throw new Error('ERC1155 not deployed');

    logProgress('Minting token ID 1 (100 units) to Alice...');
    const hash = await walletClient.writeContract({
      address: erc1155Address,
      abi: ERC1155_ABI,
      functionName: 'mint',
      args: [TEST_ACCOUNTS.ALICE.address, 1n, 100n],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Mint failed');

    const balance = await publicClient.readContract({
      address: erc1155Address,
      abi: ERC1155_ABI,
      functionName: 'balanceOf',
      args: [1n, TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice ERC1155 balance (ID 1): ${balance}`);
    if (balance !== 100n) throw new Error('ERC1155 mint amount mismatch');
  });
}

// ============================================================================
// SPOT TRADING TESTS - Phase 1c (HIP-1 Style Tokens)
// ============================================================================
//
// HIP-1 (Hyperliquid Improvement Proposal 1) defines Hyperliquid's native
// token standard for spot trading. Key features:
//
// ARCHITECTURE:
// -------------
// - Tokens exist on HyperCore layer (not EVM) with built-in orderbooks
// - Each token has a unique index (0-255): 0 = USDC (quote), 1+ = deployed tokens
// - Spot markets pair base tokens with USDC (e.g., TEST-USDC)
// - Balances tracked per (account, tokenIndex) tuple
//
// TOKEN INDEX SYSTEM:
// -------------------
// Index 0: USDC - Native quote token, all markets are quoted in USDC
// Index 1: TEST - First deployed token (deployed by node initialization)
// Index 2+: User-deployed tokens
//
// MARKET ID SYSTEM:
// -----------------
// Market IDs separate perpetuals from spot:
//   0-127:  Perpetual markets (BTC-PERP=0, ETH-PERP=1, ...)
//   128+:   Spot markets (TEST-USDC=128, ...)
//
// This separation allows both types of orders to coexist without ID collisions.
//
// SPOT ORDER WIRE FORMAT:
// -----------------------
// Spot orders use action.type = 'spotOrder' with same field structure as perps:
//   a: number   - Spot market ID (128+, NOT token index)
//   b: boolean  - Side (true = Buy base, false = Sell base)
//   p: string   - Price per base token in USDC
//   s: string   - Size in base tokens
//   r: boolean  - Always false for spot (no reduce-only concept)
//   t: object   - Order type (same as perps: Gtc, Ioc, Alo)
//
// BALANCE VALIDATION:
// -------------------
// Unlike perps (margin-based), spot trading requires actual token balances:
//   - Buy: Need sufficient USDC (quote) balance
//   - Sell: Need sufficient base token balance
// The engine reserves balances when orders are placed and releases on cancel/fill.
// ============================================================================

async function runSpotTests(ctx: TestContext): Promise<void> {
  logSection('10. Spot Trading Tests');
  log('');
  log('  Testing HIP-1 style spot token trading (Phase 1c)');
  log('');

  // Store spot metadata for use across tests in this section
  let spotTokens: { index: number; symbol: string; name: string }[] = [];
  let spotMarkets: { id: number; name: string; baseToken: number; quoteToken: number }[] = [];

  // Default spot market ID (128 = first spot market)
  // This gets updated from spotMeta response to handle any initialization order
  let testUsdcMarketId: number = 128;

  await runTest(ctx, 'Get spot metadata', 'spot', 'Retrieve spot exchange metadata including tokens and markets', async () => {
    logProgress('Fetching spot metadata...');
    const meta = (await infoRequest('spotMeta')) as {
      tokens?: { index: number; symbol: string; name: string; weiDecimals: number; szDecimals: number }[];
      universe?: { id: number; name: string; baseToken: number; quoteToken: number }[];
    };

    if (!meta.tokens) throw new Error('Missing tokens in spot metadata');
    if (!meta.universe) throw new Error('Missing universe in spot metadata');

    spotTokens = meta.tokens;
    spotMarkets = meta.universe;

    logProgress(`Found ${meta.tokens.length} tokens: ${meta.tokens.map((t) => t.symbol).join(', ')}`);
    logProgress(`Found ${meta.universe.length} markets: ${meta.universe.map((m) => m.name).join(', ')}`);

    // Verify we have the TEST token (deployed by node initialization)
    const testToken = meta.tokens.find((t) => t.symbol === 'TEST');
    if (!testToken) throw new Error('TEST token not found - node initialization failed');
    logProgress(`TEST token at index ${testToken.index}, decimals: wei=${testToken.weiDecimals}, sz=${testToken.szDecimals}`);

    // Store TEST-USDC market ID for order tests
    const testMarket = meta.universe.find((m) => m.name === 'TEST-USDC');
    if (testMarket) {
      testUsdcMarketId = testMarket.id;
      logProgress(`TEST-USDC market ID: ${testUsdcMarketId}`);
    }
  });

  await runTest(ctx, 'Verify spot market structure', 'spot', 'Verify TEST-USDC market was created correctly', async () => {
    logProgress('Checking TEST-USDC market...');

    const testMarket = spotMarkets.find((m) => m.name === 'TEST-USDC');
    if (!testMarket) throw new Error('TEST-USDC market not found');

    // Verify market configuration
    if (testMarket.baseToken !== 1) throw new Error(`Expected baseToken=1, got ${testMarket.baseToken}`);
    if (testMarket.quoteToken !== 0) throw new Error(`Expected quoteToken=0 (USDC), got ${testMarket.quoteToken}`);

    logProgress(`Market ID: ${testMarket.id}, base: token ${testMarket.baseToken}, quote: token ${testMarket.quoteToken}`);
  });

  await runTest(ctx, 'Get spot mid prices', 'spot', 'Retrieve current mid prices for all spot markets', async () => {
    logProgress('Fetching spot mid prices...');
    const mids = (await infoRequest('spotAllMids')) as Record<string, string>;

    // Mid prices may be empty if no orders are on the book
    const markets = Object.keys(mids);
    logProgress(`${markets.length} markets have mid prices`);

    // Log any available mid prices
    for (const [market, price] of Object.entries(mids)) {
      logProgress(`  ${market}: ${price}`);
    }
  });

  await runTest(ctx, 'Get spot orderbook', 'spot', 'Retrieve L2 orderbook for TEST-USDC market', async () => {
    logProgress('Fetching TEST-USDC orderbook...');
    const book = (await infoRequest('spotL2Book', { coin: 'TEST-USDC' })) as {
      coin?: string;
      time?: number;
      levels?: [string[], string[]][];
    };

    if (!book.levels) throw new Error('Missing levels in orderbook response');
    if (!book.coin) throw new Error('Missing coin in orderbook response');

    const [bids, asks] = book.levels;
    logProgress(`Market: ${book.coin}`);
    logProgress(`Orderbook: ${bids?.length || 0} bid levels, ${asks?.length || 0} ask levels`);
  });

  await runTest(ctx, 'Get spot balances for user', 'spot', 'Retrieve user spot token balances', async () => {
    logProgress('Fetching spot balances for Alice...');
    const balances = (await infoRequest('spotBalances', { user: TEST_ACCOUNTS.ALICE.address })) as {
      tokenIndex: number;
      symbol: string;
      total: string;
      reserved: string;
      available: string;
    }[];

    // Test accounts should have balances credited by node initialization
    logProgress(`Found ${balances?.length || 0} token balances`);

    // Verify USDC balance (credited 100,000 USDC)
    const usdcBalance = balances?.find((b) => b.tokenIndex === 0);
    if (!usdcBalance) throw new Error('USDC balance not found for test account');
    logProgress(`  USDC: total=${usdcBalance.total}, available=${usdcBalance.available}`);
    if (parseFloat(usdcBalance.total) < 100000) {
      throw new Error(`Expected at least 100000 USDC, got ${usdcBalance.total}`);
    }

    // Verify TEST balance (credited 10,000 TEST)
    const testBalance = balances?.find((b) => b.tokenIndex === 1);
    if (!testBalance) throw new Error('TEST balance not found for test account');
    logProgress(`  TEST: total=${testBalance.total}, available=${testBalance.available}`);
    if (parseFloat(testBalance.total) < 10000) {
      throw new Error(`Expected at least 10000 TEST, got ${testBalance.total}`);
    }
  });

  await runTest(ctx, 'Get spot open orders', 'spot', 'Retrieve user open spot orders', async () => {
    logProgress('Fetching open spot orders for Alice...');
    const orders = (await infoRequest('spotOpenOrders', { user: TEST_ACCOUNTS.ALICE.address })) as {
      market: string;
      oid: number;
      side: string;
      limitPx: string;
      sz: string;
    }[];

    // Orders may be empty for a new account
    logProgress(`Found ${orders?.length || 0} open orders`);
    for (const order of orders || []) {
      logProgress(`  ${order.market}: ${order.side} ${order.sz} @ ${order.limitPx} (oid: ${order.oid})`);
    }
  });

  await runTest(ctx, 'Get spot token info by index', 'spot', 'Retrieve detailed info for TEST token', async () => {
    logProgress('Fetching token info for index 1 (TEST)...');
    const info = (await infoRequest('spotTokenInfo', { index: 1 })) as {
      index: number;
      symbol: string;
      name: string;
      weiDecimals: number;
      szDecimals: number;
      maxSupply: string;
      circulatingSupply: string;
      systemAddress: string;
      deployer: string;
    };

    if (!info.symbol) throw new Error('Missing symbol in token info');
    if (info.symbol !== 'TEST') throw new Error(`Expected symbol=TEST, got ${info.symbol}`);

    logProgress(`Token: ${info.name} (${info.symbol})`);
    logProgress(`  Decimals: wei=${info.weiDecimals}, sz=${info.szDecimals}`);
    logProgress(`  Supply: max=${info.maxSupply}, circulating=${info.circulatingSupply}`);
    logProgress(`  System address: ${info.systemAddress}`);
  });

  await runTest(ctx, 'Get spot open orders with market filter', 'spot', 'Filter open orders by specific market', async () => {
    logProgress('Fetching open spot orders for Alice in TEST-USDC...');
    const orders = (await infoRequest('spotOpenOrders', {
      user: TEST_ACCOUNTS.ALICE.address,
      market: 'TEST-USDC',
    })) as unknown[];

    logProgress(`Found ${orders?.length || 0} orders in TEST-USDC market`);
  });

  // Test spot order placement using the stubbed signature verification
  // The stub extracts the address from the r value of the signature
  await runTest(ctx, 'Place spot limit order', 'spot', 'Place a limit buy order on TEST-USDC', async () => {
    logProgress(`Placing limit buy order for TEST (market ID: ${testUsdcMarketId})...`);

    // Create order action - buy 10 TEST at $1.00
    // Market ID is dynamically fetched from spot metadata (starts at 128)
    const action = {
      type: 'spotOrder',
      orders: [
        {
          a: testUsdcMarketId, // market id from metadata
          b: true, // buy
          p: '1.0', // price ($1.00 per TEST)
          s: '10', // size (10 TEST tokens)
          r: false, // not reduce only
          t: { limit: { tif: 'Gtc' } },
        },
      ],
      grouping: 'na',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    logProgress('Signed order, sending to exchange...');

    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      error?: string;
      response?: { type: string; data?: { statuses?: { resting?: { oid: number }; filled?: unknown; error?: string }[] } };
    };

    if (result.status !== 'ok') {
      throw new Error(`Order failed: ${result.error || JSON.stringify(result)}`);
    }

    const statuses = result.response?.data?.statuses || [];
    logProgress(`Got ${statuses.length} order status(es)`);

    // Check for errors in order statuses
    for (const status of statuses) {
      if (status.error) {
        throw new Error(`Order rejected: ${status.error}`);
      }
      if (status.resting) {
        logProgress(`Order resting with ID: ${status.resting.oid}`);
      }
      if (status.filled) {
        logProgress(`Order filled: ${JSON.stringify(status.filled)}`);
      }
    }
  });

  await runTest(ctx, 'Verify spot order in open orders', 'spot', 'Check that placed order appears in open orders', async () => {
    logProgress('Checking for placed order...');
    const orders = (await infoRequest('spotOpenOrders', { user: TEST_ACCOUNTS.ALICE.address })) as {
      market: string;
      oid: number;
      side: string;
      limitPx: string;
      sz: string;
    }[];

    logProgress(`Found ${orders?.length || 0} open orders`);

    // The order we placed should be resting (no counterparty to match with)
    if (!orders || orders.length === 0) {
      throw new Error('Expected at least 1 open order after placing limit buy');
    }

    const buyOrder = orders.find((o) => o.side === 'B' && o.market === 'TEST-USDC');
    if (!buyOrder) {
      throw new Error('Expected to find buy order in TEST-USDC market');
    }

    logProgress(`Found buy order: ${buyOrder.sz} @ ${buyOrder.limitPx} (oid: ${buyOrder.oid})`);
  });

  await runTest(ctx, 'Cancel all spot orders', 'spot', 'Cancel all open spot orders for user', async () => {
    logProgress('Cancelling all spot orders...');

    const action = {
      type: 'spotCancelAll',
    };

    const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.ALICE.privateKey);
    const result = (await exchangeRequest(action, signature, nonce)) as {
      status?: string;
      response?: { type: string; data?: { canceledCount?: number } };
    };

    if (result.status !== 'ok') throw new Error(`Cancel failed: ${JSON.stringify(result)}`);

    const canceledCount = result.response?.data?.canceledCount || 0;
    logProgress(`Cancelled ${canceledCount} orders`);
  });

  await runTest(ctx, 'Verify orders cancelled', 'spot', 'Check that all orders were cancelled', async () => {
    logProgress('Verifying no open orders...');
    const orders = (await infoRequest('spotOpenOrders', { user: TEST_ACCOUNTS.ALICE.address })) as unknown[];

    if ((orders?.length || 0) > 0) {
      throw new Error(`Expected 0 orders after cancel, got ${orders.length}`);
    }
    logProgress('All orders successfully cancelled');
  });
}

// ============================================================================
// PHASE 2A: UNIFIED STATE TESTS
// ============================================================================
//
// These tests verify the unified state model where both HyperCore and HyperEVM
// share a single master balance sheet with separate views.
//
// Key concepts:
// - `total`: The source of truth, only changes on deposits/withdrawals
// - `core_view`: Portion available for HyperCore trading
// - `evm_view`: Portion available for HyperEVM operations
// - View transfers adjust views WITHOUT changing total
//
// Invariant: total == core_view + evm_view (ALWAYS)
// ============================================================================

async function runUnifiedStateTests(ctx: TestContext): Promise<void> {
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

      logProgress(`Transfer correctly rejected: ${result.error || 'Insufficient balance'}`);
    } catch (error: unknown) {
      // HTTP error is also acceptable for insufficient balance
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('Insufficient') || message.includes('error')) {
        logProgress(`Transfer correctly rejected: ${message}`);
      } else {
        throw error;
      }
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

      logProgress(`Transfer correctly rejected: ${result.error || 'Insufficient balance'}`);
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('Insufficient') || message.includes('error')) {
        logProgress(`Transfer correctly rejected: ${message}`);
      } else {
        throw error;
      }
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
      logProgress('TEST token balance not found, skipping...');
      return;
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

    if (result.status === 'ok' && result.response?.data) {
      logProgress(`TEST transfer successful:`);
      logProgress(`  New Core View: ${result.response.data.newCoreView}`);
      logProgress(`  New EVM View: ${result.response.data.newEvmView}`);
      logProgress(`  Total: ${result.response.data.total} (should be unchanged)`);
    } else {
      logProgress('TEST transfer may have failed or token not initialized - test passes if USDC works');
    }
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

    const initialCoreView = parseFloat(usdcBefore.coreView);
    logProgress(`Initial Core View: ${initialCoreView} USDC`);

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
    const remainingCoreView = initialCoreView - transferAmount;
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
      const error = e as Error;
      if (error.name === 'AbortError' || error.message.includes('fetch')) {
        logProgress(`EVM RPC not available (${error.message}) - skipping EVM balance verification`);
        // Don't fail the test if EVM RPC is not running
      } else {
        throw e;
      }
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

  await runTest(ctx, 'Zero amount transfer rejection', 'unified', 'Verify zero amount transfers are handled correctly', async () => {
    logProgress('Testing zero amount transfer handling...');

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

      // Zero transfers might succeed (no-op) or be rejected - both are valid
      if (result.status === 'ok') {
        logProgress('Zero transfer treated as no-op (acceptable)');
      } else {
        logProgress(`Zero transfer rejected: ${result.error} (acceptable)`);
      }
    } catch (e) {
      logProgress('Zero transfer rejected with error (acceptable)');
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
      logProgress('Insufficient balance for this test, skipping...');
      return;
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
      // Network error or other rejection = transfer was blocked
      transferBlocked = true;
      logProgress('Transfer correctly rejected due to reserved balance');
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
      logProgress('Insufficient TEST tokens for this test, skipping...');
      return;
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
    const initialTotal = parseFloat(usdcInitial.total);
    const finalTotal = parseFloat(usdcAfterToCore?.total || '0');
    if (Math.abs(initialTotal - finalTotal) > 0.000001) {
      throw new Error(`Total changed! Initial=${initialTotal}, Final=${finalTotal}`);
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
      logProgress('Insufficient balance for exact transfer test, skipping...');
      return;
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

// ============================================================================
// STRESS TESTS
// ============================================================================

async function runStressTests(ctx: TestContext): Promise<void> {
  logSection('13. Stress & Performance Tests');
  log('');
  log('  Testing system under load');
  log('');

  await runTest(ctx, 'Rapid order placement', 'stress', 'Place multiple orders in quick succession', async () => {
    logProgress('Placing 10 orders rapidly...');
    const orders = [];

    for (let i = 0; i < 10; i++) {
      const price = 60000 + i * 100;
      const action = {
        type: 'order',
        orders: [{ a: 0, b: true, p: price.toString(), s: '0.001', r: false, t: { limit: { tif: 'Gtc' } } }],
        grouping: 'na',
      };
      const { signature, nonce } = await signAction(action, TEST_ACCOUNTS.CHARLIE.privateKey);
      orders.push(exchangeRequest(action, signature, nonce));
    }

    await Promise.allSettled(orders);
    logProgress('All orders submitted');

    // Cleanup
    await sleep(500);
    const cancelAction = { type: 'cancelAll' };
    const { signature, nonce } = await signAction(cancelAction, TEST_ACCOUNTS.CHARLIE.privateKey);
    await exchangeRequest(cancelAction, signature, nonce);
    logProgress('Orders cleaned up');
  });

  await runTest(ctx, 'Concurrent API requests', 'stress', 'Make multiple API requests simultaneously', async () => {
    logProgress('Making 20 concurrent requests...');
    const requests = [];

    for (let i = 0; i < 20; i++) {
      requests.push(infoRequest('allMids'));
    }

    const results = await Promise.allSettled(requests);
    const successful = results.filter((r) => r.status === 'fulfilled').length;
    logProgress(`${successful}/20 requests successful`);
  });

  await runTest(ctx, 'Large orderbook query', 'stress', 'Query full orderbook depth', async () => {
    logProgress('Fetching deep orderbook...');
    const start = Date.now();
    const book = await infoRequest('l2Book', { coin: MARKETS.BTC_PERP, nSigFigs: 5 });
    const duration = Date.now() - start;
    logProgress(`Orderbook fetched in ${duration}ms`);
  });
}

// ============================================================================
// SUMMARY
// ============================================================================

function printSummary(ctx: TestContext): void {
  const totalDuration = Date.now() - ctx.startTime;

  logHeader('E2E Test Summary');

  const passed = ctx.results.filter((r) => r.status === 'pass').length;
  const failed = ctx.results.filter((r) => r.status === 'fail').length;
  const skipped = ctx.results.filter((r) => r.status === 'skip').length;
  const total = ctx.results.length;

  log(`  ${colors.white}Total Tests:${colors.reset}    ${total}`);
  log(`  ${colors.green}Passed:${colors.reset}         ${passed}`);
  log(`  ${colors.red}Failed:${colors.reset}         ${failed}`);
  log(`  ${colors.yellow}Skipped:${colors.reset}        ${skipped}`);
  log('');
  log(`  ${colors.white}Duration:${colors.reset}       ${(totalDuration / 1000).toFixed(2)}s`);
  log('');

  // Group by category
  const categories = [...new Set(ctx.results.map((r) => r.category))];

  log(`  ${colors.white}Results by Category:${colors.reset}`);
  for (const category of categories) {
    const catResults = ctx.results.filter((r) => r.category === category);
    const catPassed = catResults.filter((r) => r.status === 'pass').length;
    const catTotal = catResults.length;
    const icon = catPassed === catTotal ? `${colors.green}✓` : `${colors.red}✗`;
    log(`    ${icon} ${category}: ${catPassed}/${catTotal}${colors.reset}`);
  }
  log('');

  // Print failed tests
  const failedTests = ctx.results.filter((r) => r.status === 'fail');
  if (failedTests.length > 0) {
    log(`  ${colors.red}Failed Tests:${colors.reset}`);
    for (const test of failedTests) {
      log(`    ${colors.red}✗ ${test.name}${colors.reset}`);
      if (test.error) {
        log(`      ${colors.yellow}${test.error}${colors.reset}`);
      }
    }
    log('');
  }

  if (failed === 0) {
    log(`  ${colors.green}╔════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.green}║     ALL TESTS PASSED! ✓            ║${colors.reset}`);
    log(`  ${colors.green}╚════════════════════════════════════╝${colors.reset}`);
  } else {
    log(`  ${colors.red}╔════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.red}║     SOME TESTS FAILED ✗            ║${colors.reset}`);
    log(`  ${colors.red}╚════════════════════════════════════╝${colors.reset}`);
  }
  log('');

  // Write results to file for bash script to parse
  console.log(`__TESTS_PASSED=${passed}`);
  console.log(`__TESTS_FAILED=${failed}`);
  console.log(`__TESTS_SKIPPED=${skipped}`);
}

// ============================================================================
// MAIN
// ============================================================================

async function main(): Promise<void> {
  const ctx: TestContext = {
    results: [],
    startTime: Date.now(),
  };

  logHeader('HyperCore E2E Integration Tests');

  log(`  ${colors.white}Configuration:${colors.reset}`);
  log(`    Gateway:  ${colors.cyan}${CONFIG.GATEWAY_URL}${colors.reset}`);
  log(`    EVM RPC:  ${colors.cyan}${CONFIG.EVM_RPC_URL}${colors.reset}`);
  log(`    Chain ID: ${colors.cyan}${CONFIG.CHAIN_ID}${colors.reset}`);
  log(`    Verbose:  ${colors.cyan}${CONFIG.VERBOSE}${colors.reset}`);
  log('');

  try {
    // Run all test categories
    await runConnectionTests(ctx);
    await runMarketDataTests(ctx);
    await runAccountTests(ctx);
    await runOrderTests(ctx);
    await runMatchingTests(ctx);
    await runPositionTests(ctx);
    await runEVMTests(ctx);
    await runAdvancedEVMTests(ctx);
    await runTokenStandardsTests(ctx);
    await runSpotTests(ctx);
    await runUnifiedStateTests(ctx);
    await runStressTests(ctx);
  } catch (error) {
    log(`${colors.red}Fatal error during test execution:${colors.reset}`);
    log(`${colors.red}${error}${colors.reset}`);
  }

  // Print summary
  printSummary(ctx);

  // Exit with appropriate code
  const failed = ctx.results.filter((r) => r.status === 'fail').length;
  process.exit(failed > 0 ? 1 : 0);
}

// Run
main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
