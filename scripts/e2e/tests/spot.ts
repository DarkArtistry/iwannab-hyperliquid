import {
  TEST_ACCOUNTS,
  infoRequest,
  exchangeRequest,
  signAction,
  runTest,
  sleep,
  logSection,
  log,
  logProgress,
  TestContext,
} from '../lib/index.js';

export async function runSpotTests(ctx: TestContext): Promise<void> {
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

    // Verify USDC balance (credited 100,000 USDC, may be reduced by previous trades)
    const usdcBalance = balances?.find((b) => b.tokenIndex === 0);
    if (!usdcBalance) throw new Error('USDC balance not found for test account');
    logProgress(`  USDC: total=${usdcBalance.total}, available=${usdcBalance.available}`);
    // Allow for some reduction due to trades in previous test runs
    if (parseFloat(usdcBalance.total) < 90000) {
      throw new Error(`Expected at least 90000 USDC, got ${usdcBalance.total}`);
    }

    // Verify TEST balance (credited 10,000 TEST, may be reduced by previous trades)
    const testBalance = balances?.find((b) => b.tokenIndex === 1);
    if (!testBalance) throw new Error('TEST balance not found for test account');
    logProgress(`  TEST: total=${testBalance.total}, available=${testBalance.available}`);
    // Allow for some reduction due to trades in previous test runs
    if (parseFloat(testBalance.total) < 9000) {
      throw new Error(`Expected at least 9000 TEST, got ${testBalance.total}`);
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
