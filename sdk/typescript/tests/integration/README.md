# HyperCore Integration Test Suite

This directory contains end-to-end integration tests for the HyperCore perpetual exchange.
These tests serve as both verification of system functionality and **documentation** for API usage.

## Test Organization

| File | Description |
|------|-------------|
| `setup.ts` | Test configuration, utilities, and test account setup |
| `01-connection.test.ts` | Basic connectivity and API health checks |
| `02-market-data.test.ts` | Read-only market data queries (orderbook, prices, metadata) |
| `03-account.test.ts` | Account management (balance, state, deposits, withdrawals) |
| `04-orders.test.ts` | Order placement, cancellation, and modification |
| `05-matching.test.ts` | Order matching scenarios (fills, partial fills) |
| `06-positions.test.ts` | Position management (open, close, PnL tracking) |
| `07-leverage.test.ts` | Leverage updates and margin calculations |
| `08-websocket.test.ts` | Real-time WebSocket subscriptions |
| `09-advanced.test.ts` | Complex trading scenarios and edge cases |

## Prerequisites

Before running integration tests, ensure:

1. **Local environment is running:**
   ```bash
   # Terminal 1: Start the gateway
   cargo run --bin hypercore-gateway -- --port 3000

   # Terminal 2: Start Anvil (for EVM testing)
   anvil --chain-id 1337

   # Terminal 3 (optional): Start the indexer
   cargo run --bin hypercore-indexer
   ```

2. **Test accounts are funded:**
   The tests use Anvil's default accounts with known private keys.

## Running Tests

```bash
# Run all integration tests
pnpm test

# Run specific test file
pnpm test tests/integration/04-orders.test.ts

# Run with verbose output
pnpm test -- --reporter=verbose

# Run specific test by name
pnpm test -- -t "should place a limit buy order"
```

## Test Accounts

The tests use Anvil's deterministic accounts:

| Account | Address | Role |
|---------|---------|------|
| Alice | `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` | Primary trader |
| Bob | `0x70997970C51812dc3A010C7d01b50e0d17dc79C8` | Counter-party |
| Charlie | `0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC` | Market maker |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GATEWAY_URL` | `http://localhost:3000` | Gateway HTTP endpoint |
| `WS_URL` | `ws://localhost:3000/ws` | WebSocket endpoint |
| `CHAIN_ID` | `1337` | EVM chain ID |

## Writing New Tests

Each test should:

1. Have a clear, descriptive name
2. Include JSDoc comments explaining the scenario
3. Assert specific expected behaviors
4. Clean up any created state

Example:
```typescript
/**
 * @scenario Place and fill a limit order
 * @given Alice has 100,000 USDC balance
 * @when Alice places a limit buy for 1 BTC at $65,000
 * @and Bob places a market sell for 1 BTC
 * @then Alice receives 1 BTC position
 * @and Bob's sell is filled at $65,000
 */
test('limit order matching', async () => {
  // Test implementation
});
```

## Troubleshooting

### Tests timing out
- Increase `testTimeout` in `vitest.config.ts`
- Check if gateway is running and responsive

### Connection refused
- Ensure gateway is running on the correct port
- Check firewall settings

### Signature errors
- Verify chain ID matches between client and server
- Check private key format (should be 0x-prefixed hex)
