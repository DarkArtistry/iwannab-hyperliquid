# HyperCore E2E Integration Tests

Comprehensive end-to-end testing suite for the HyperCore perpetual futures exchange.

## Overview

This E2E test suite validates the entire HyperCore stack including:

- **API Gateway**: REST endpoints for market data and trading
- **Exchange Engine**: Order placement, matching, and cancellation
- **EVM Integration**: Contract interactions and precompiles
- **WebSocket**: Real-time updates (where applicable)

## Quick Start

### Full E2E Test Run

The main script handles everything: stopping existing services, starting fresh Docker containers, running tests, and cleanup.

```bash
# From the project root
./scripts/e2e-test.sh
```

### Options

```bash
# Skip Docker, use already running services
./scripts/e2e-test.sh --no-docker

# Keep services running after tests (for debugging)
./scripts/e2e-test.sh --keep

# Verbose output with detailed progress
./scripts/e2e-test.sh --verbose

# Combine options
./scripts/e2e-test.sh --no-docker --verbose
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GATEWAY_URL` | `http://localhost:3000` | Gateway HTTP endpoint |
| `EVM_RPC_URL` | `http://localhost:8545` | EVM JSON-RPC endpoint |
| `CHAIN_ID` | `1337` | EVM chain ID |
| `SKIP_CONTRACTS` | `false` | Skip contract deployment |
| `VERBOSE` | `false` | Enable verbose output |

## Test Categories

### 1. Connection & Health Tests
Validates basic connectivity to all services.

| Test | Description |
|------|-------------|
| Gateway health check | Verify Gateway service is responding |
| Info endpoint available | Verify /info POST endpoint accepts requests |
| Exchange endpoint available | Verify /exchange POST endpoint exists |
| EVM RPC available | Verify EVM JSON-RPC endpoint is responding |

### 2. Market Data Tests
Tests read-only market data queries.

| Test | Description |
|------|-------------|
| Get exchange metadata | Retrieve exchange configuration including available markets |
| Get all mid prices | Retrieve current mid prices for all markets |
| Get L2 orderbook (BTC-PERP) | Retrieve level 2 orderbook with bid/ask depth |
| Get L2 orderbook (ETH-PERP) | Retrieve ETH-PERP orderbook depth |
| Get recent trades | Retrieve recent trade history for a market |
| Get funding rates | Retrieve current funding rate for perpetual markets |
| Get candles (1h) | Retrieve OHLCV candlestick data |

### 3. Account State Tests
Tests account queries for test wallets.

| Test | Description |
|------|-------------|
| Get Alice account state | Retrieve full account state including margin and positions |
| Get Bob account state | Retrieve Bob account for matching tests |
| Get open orders (Alice) | Retrieve all open orders for an account |
| Get user fills (Alice) | Retrieve trade fill history for an account |
| Get user funding (Alice) | Retrieve funding payment history |

### 4. Order Lifecycle Tests
Tests order placement, modification, and cancellation.

| Test | Description |
|------|-------------|
| Place limit buy order | Place a limit buy order below market price |
| Place limit sell order | Place a limit sell order above market price |
| Place post-only order | Place maker-only order that rejects if would cross |
| Place IOC order | Place immediate-or-cancel order |
| Batch place orders | Place multiple orders in a single request |
| Cancel single order | Cancel a specific order by ID |
| Cancel all orders | Cancel all open orders for an account |

### 5. Order Matching Tests
Tests order matching between different accounts.

| Test | Description |
|------|-------------|
| Match limit orders | Test basic order matching between two accounts |
| Match with price improvement | Buy order at higher price should match sell at lower price |
| Partial fill test | Large order should partially fill against smaller order |
| Clean up matching test orders | Cancel remaining orders from both accounts |

### 6. Position Management Tests
Tests position tracking, leverage, and PnL.

| Test | Description |
|------|-------------|
| Check position after trades | Verify positions are correctly tracked after matching |
| Update leverage | Change leverage setting for a market |
| Check margin requirements | Verify margin calculations after leverage change |

### 7. EVM Integration Tests
Comprehensive tests for all EVM JSON-RPC methods.

#### Basic RPC Methods
| Test | Description |
|------|-------------|
| eth_chainId | Query chain ID from EVM RPC |
| eth_blockNumber | Query current block height from EVM |
| eth_gasPrice | Query current gas price from EVM |

#### Account State Methods
| Test | Description |
|------|-------------|
| eth_getBalance | Check native token balance for test accounts |
| eth_getTransactionCount | Query nonce for test accounts |
| eth_getCode | Query code at an address (EOA should be empty) |
| eth_getStorageAt | Query storage slot at an address |

#### Transaction Methods
| Test | Description |
|------|-------------|
| eth_sendRawTransaction (ETH transfer) | Execute a simple ETH transfer |
| eth_getTransactionByHash | Query transaction details by hash |
| eth_getTransactionReceipt | Query transaction receipt by hash |
| eth_estimateGas | Estimate gas for a transaction |

#### Block Methods
| Test | Description |
|------|-------------|
| eth_getBlockByNumber (latest) | Query latest block by tag |
| eth_getBlockByNumber (specific) | Query block by number |
| eth_getBlockByHash | Query block by hash |

#### Call Methods
| Test | Description |
|------|-------------|
| eth_call (simple) | Execute read-only call to zero address |
| eth_call (precompile) | Attempt to call HyperCore precompile |

#### Fee Methods
| Test | Description |
|------|-------------|
| eth_maxPriorityFeePerGas | Query max priority fee per gas |
| eth_feeHistory | Query fee history for recent blocks |

#### Web3 & Net Methods
| Test | Description |
|------|-------------|
| web3_clientVersion | Query client version string |
| net_version | Query network version (chain ID) |
| net_listening | Check if node is listening for connections |
| net_peerCount | Query number of connected peers |

#### Misc Methods
| Test | Description |
|------|-------------|
| eth_accounts | Query unlocked accounts (should be empty) |
| eth_getLogs (empty filter) | Query logs with empty filter |

### 8. Advanced EVM Tests
Tests contract deployment and smart contract interactions.

| Test | Description |
|------|-------------|
| Deploy SimpleStorage contract | Deploy a simple storage contract |
| Verify contract code | Check deployed contract has code |
| Read initial contract state | Call value() getter on deployed contract |
| Write contract state | Call set() to update contract state |
| Read updated contract state | Verify set() updated the value |
| Read contract storage directly | Use eth_getStorageAt to read raw storage |
| Estimate gas for contract call | Estimate gas for set() call |
| Multiple transactions in sequence | Execute multiple state changes |
| Check nonce increments correctly | Verify nonce increases after transactions |

### 9. Stress & Performance Tests
Tests system under load.

| Test | Description |
|------|-------------|
| Rapid order placement | Place multiple orders in quick succession |
| Concurrent API requests | Make multiple API requests simultaneously |
| Large orderbook query | Query full orderbook depth |

## Test Accounts

The tests use Anvil's deterministic test accounts:

| Name | Address | Role |
|------|---------|------|
| Alice | `0xf39F...2266` | Primary trader |
| Bob | `0x7099...79C8` | Counter-party |
| Charlie | `0x3C44...93BC` | Market maker |

These accounts have 10,000 ETH each on local Anvil.

## Running Standalone

You can also run the TypeScript tests directly:

```bash
cd scripts/e2e

# Install dependencies
npm install

# Run tests
npx tsx runner.ts

# Run with verbose output
VERBOSE=true npx tsx runner.ts
```

## Test Output

The test runner provides:

1. **Progress Output**: Each test shows status with timing
2. **Category Summaries**: Results grouped by test category
3. **Final Summary**: Total pass/fail/skip counts
4. **Exit Code**: Returns 0 on success, 1 on failure

Example output:

```
════════════════════════════════════════════════════════════════════
  HyperCore E2E Integration Tests
════════════════════════════════════════════════════════════════════

────────────────────────────────────────────────────────────────────
  1. Connection & Health Tests
────────────────────────────────────────────────────────────────────

  ✓ Gateway health check (45ms)
  ✓ Info endpoint available (12ms)
  ✓ Exchange endpoint available (8ms)
  ✓ EVM RPC available (15ms)

...

════════════════════════════════════════════════════════════════════
  E2E Test Summary
════════════════════════════════════════════════════════════════════

  Total Tests:    56
  Passed:         56
  Failed:         0
  Skipped:        0

  Duration:       18.45s

  Results by Category:
    ✓ connection: 4/4
    ✓ market-data: 7/7
    ✓ account: 5/5
    ✓ orders: 7/7
    ✓ matching: 4/4
    ✓ positions: 3/3
    ✓ evm: 20/20
    ✓ evm-advanced: 9/9
    ✓ stress: 3/3

  ╔════════════════════════════════════╗
  ║     ALL TESTS PASSED! ✓            ║
  ╚════════════════════════════════════╝
```

## Adding New Tests

To add a new test, use the `runTest` helper:

```typescript
await runTest(
  ctx,
  'Test Name',           // Display name
  'category',            // Category for grouping
  'Description',         // For documentation
  async () => {
    // Your test logic
    logProgress('Step 1...');
    const result = await someApiCall();

    if (!result.ok) {
      throw new Error('Test failed');
    }

    logProgress('Test passed');
  }
);
```

## Troubleshooting

### Tests timeout

Increase timeout in the test configuration or check service health:

```bash
# Check if services are running
docker-compose ps

# View service logs
docker-compose logs gateway
```

### Port conflicts

The script attempts to kill processes on required ports. If issues persist:

```bash
# Manually check ports
lsof -i :3000
lsof -i :8545

# Kill specific process
kill -9 <PID>
```

### EVM tests fail

Ensure Anvil or the EVM service is running and accessible:

```bash
# Test EVM connectivity
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## Integration with CI/CD

The script returns appropriate exit codes for CI integration:

```yaml
# Example GitHub Actions
- name: Run E2E Tests
  run: |
    chmod +x ./scripts/e2e-test.sh
    ./scripts/e2e-test.sh --no-docker
  env:
    GATEWAY_URL: ${{ secrets.GATEWAY_URL }}
```

## License

MIT
