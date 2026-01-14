# HyperCore TypeScript SDK

Official TypeScript/JavaScript SDK for the HyperCore perpetual futures exchange.

## Installation

```bash
# Using pnpm (recommended)
pnpm add @hypercore/sdk

# Using npm
npm install @hypercore/sdk

# Using yarn
yarn add @hypercore/sdk
```

## Quick Start

```typescript
import { HyperCore, OrderSide, OrderType } from '@hypercore/sdk';

// Create client
const client = new HyperCore({
  baseUrl: 'http://localhost:3000',
  privateKey: '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80',
  chainId: 1337,
});

// Get account state
const state = await client.info.getAccountState(client.address!);
console.log('Account value:', state.marginSummary.accountValue);

// Place a limit order
const result = await client.exchange.placeOrder({
  market: 'BTC-PERP',
  side: OrderSide.Buy,
  price: '65000',
  size: '0.1',
  type: OrderType.Limit,
});
console.log('Order result:', result);
```

## API Reference

### Client Configuration

```typescript
interface HyperCoreConfig {
  baseUrl?: string;      // Default: 'https://api.hypercore.xyz'
  wsUrl?: string;        // Default: 'wss://ws.hypercore.xyz'
  privateKey?: string;   // Required for trading operations
  chainId?: number;      // Default: 1337
  timeout?: number;      // Default: 30000 (ms)
}
```

### Info API (Read-Only)

All info methods work without authentication.

```typescript
// Exchange metadata
const meta = await client.info.getMeta();
// Returns: { universe: [{ name: 'BTC-PERP', szDecimals: 8, maxLeverage: 50, ... }] }

// Mid prices for all markets
const mids = await client.info.getAllMids();
// Returns: { 'BTC-PERP': '65000.5', 'ETH-PERP': '3500.25' }

// L2 Order book
const book = await client.info.getL2Book('BTC-PERP');
// Returns: { levels: [[bids], [asks]], time: 1705123456789 }

// Account state
const state = await client.info.getAccountState('0x...');
// Returns: { marginSummary: {...}, assetPositions: [...] }

// Open orders
const orders = await client.info.getOpenOrders('0x...');
// Returns: [{ coin, oid, side, limitPx, sz, origSz, timestamp }]

// Fill history
const fills = await client.info.getUserFills('0x...', {
  startTime: Date.now() - 86400000,
  endTime: Date.now(),
});

// Funding history
const funding = await client.info.getFundingHistory('BTC-PERP');

// Recent trades
const trades = await client.info.getRecentTrades('BTC-PERP', 100);

// Candle data (OHLCV)
const candles = await client.info.getCandles('BTC-PERP', '1h');
```

### Exchange API (Authenticated)

All exchange methods require a private key.

#### Place Orders

```typescript
// Single order
const result = await client.exchange.placeOrder({
  market: 'BTC-PERP',      // Market symbol or ID
  side: OrderSide.Buy,     // 'buy' or 'sell'
  price: '65000',          // Limit price (optional for market orders)
  size: '0.1',             // Order size
  type: OrderType.Limit,   // 'limit', 'market', 'postOnly', 'ioc', 'fok'
  reduceOnly: false,       // Only reduce position
  clientOrderId: 'my-id',  // Optional tracking ID
});

// Batch orders (more efficient)
const result = await client.exchange.placeOrders([
  { market: 'BTC-PERP', side: OrderSide.Buy, price: '64000', size: '0.1' },
  { market: 'BTC-PERP', side: OrderSide.Buy, price: '63000', size: '0.1' },
  { market: 'BTC-PERP', side: OrderSide.Sell, price: '66000', size: '0.1' },
]);
```

#### Cancel Orders

```typescript
// Cancel by order ID
await client.exchange.cancelOrder({
  market: 'BTC-PERP',
  orderId: 12345,
});

// Cancel by client order ID
await client.exchange.cancelByCloid({
  market: 'BTC-PERP',
  clientOrderId: 'my-id',
});

// Cancel all orders in market
await client.exchange.cancelAllOrders('BTC-PERP');

// Cancel all orders globally
await client.exchange.cancelAllOrders();
```

#### Modify Orders

```typescript
await client.exchange.modifyOrder(orderId, {
  market: 'BTC-PERP',
  side: OrderSide.Buy,
  price: '65500',    // New price
  size: '0.15',      // New size
  type: OrderType.Limit,
});
```

#### Leverage & Transfers

```typescript
// Update leverage
await client.exchange.updateLeverage({
  market: 'BTC-PERP',
  leverage: 20,
  isCross: true,     // Cross margin mode
});

// Transfer USDC
await client.exchange.transfer({
  destination: '0x...',
  amount: '1000',
});

// Withdraw to L1
await client.exchange.withdraw({
  destination: '0x...',
  amount: '500',
});
```

### WebSocket API

```typescript
// Connect to WebSocket
await client.connect();

// Subscribe to market data
client.subscribeToMarket('BTC-PERP', {
  onTrade: (trade) => console.log('Trade:', trade),
  onL2Book: (book) => console.log('Book update:', book),
});

// Subscribe to user updates (requires auth)
client.subscribeToUser((update) => {
  if (update.fills) console.log('Fill:', update.fills);
  if (update.orders) console.log('Order update:', update.orders);
});

// Disconnect
client.disconnect();
```

## Order Types

| Type | Description | Use Case |
|------|-------------|----------|
| `limit` | Good-til-canceled limit order | Default for passive orders |
| `market` | Execute immediately at best price | Aggressive entry/exit |
| `postOnly` | Maker-only, rejects if would cross | Market making |
| `ioc` | Immediate-or-cancel | Aggressive, no resting |
| `fok` | Fill-or-kill (all or nothing) | Exact size execution |

## Error Handling

```typescript
try {
  await client.exchange.placeOrder({ ... });
} catch (error) {
  if (error instanceof HyperCoreError) {
    console.log('Error code:', error.code);
    console.log('Message:', error.message);
  }
}
```

### Error Codes

| Code | Description |
|------|-------------|
| 1001 | Invalid signature |
| 1002 | Invalid nonce |
| 1003 | Insufficient margin |
| 1004 | Invalid price |
| 1005 | Invalid size |
| 1006 | Order not found |
| 1007 | Market not found |
| 1013 | Market paused |
| 1014 | Rate limited |

## Testing

The SDK includes comprehensive integration tests that also serve as documentation:

```bash
# Run all tests
pnpm test

# Run integration tests
pnpm test:integration

# Watch mode
pnpm test:watch
```

### Test Coverage (86 tests)

| File | Tests | Coverage |
|------|-------|----------|
| 01-connection.test.ts | 7 | Connectivity, auth |
| 02-market-data.test.ts | 14 | Orderbook, prices |
| 03-account.test.ts | 13 | State, transfers |
| 04-orders.test.ts | 17 | All order types |
| 05-matching.test.ts | 10 | Matching logic |
| 06-positions.test.ts | 14 | Positions, PnL |
| 07-advanced.test.ts | 11 | Market making |

See [tests/integration/README.md](tests/integration/README.md) for test documentation.

## Building

```bash
# Install dependencies
pnpm install

# Build
pnpm build

# Type check
pnpm typecheck

# Lint
pnpm lint

# Format
pnpm format
```

## Examples

### Market Making Bot

```typescript
import { HyperCore, OrderSide, OrderType } from '@hypercore/sdk';

async function marketMake(client: HyperCore) {
  // Get current mid price
  const mids = await client.info.getAllMids();
  const mid = parseFloat(mids['BTC-PERP']);

  // Calculate quotes with 0.1% spread
  const spread = 0.001;
  const bidPrice = (mid * (1 - spread / 2)).toFixed(0);
  const askPrice = (mid * (1 + spread / 2)).toFixed(0);

  // Place two-sided quotes
  await client.exchange.placeOrders([
    {
      market: 'BTC-PERP',
      side: OrderSide.Buy,
      price: bidPrice,
      size: '0.1',
      type: OrderType.PostOnly,
    },
    {
      market: 'BTC-PERP',
      side: OrderSide.Sell,
      price: askPrice,
      size: '0.1',
      type: OrderType.PostOnly,
    },
  ]);
}
```

### Position Monitor

```typescript
import { HyperCore } from '@hypercore/sdk';

async function monitorPosition(client: HyperCore) {
  await client.connect();

  client.subscribeToUser((update) => {
    if (update.fills) {
      for (const fill of update.fills) {
        console.log(`Filled: ${fill.sz} @ ${fill.px}`);
      }
    }
  });

  // Check position periodically
  setInterval(async () => {
    const state = await client.info.getAccountState(client.address!);
    for (const { position } of state.assetPositions) {
      console.log(`${position.coin}: ${position.size} @ ${position.entryPx}`);
      console.log(`  PnL: ${position.unrealizedPnl}`);
    }
  }, 5000);
}
```

## License

MIT
