# HyperCore E2E Integration Tests

Comprehensive end-to-end testing suite for the HyperCore perpetual futures exchange.

## Overview

This E2E test suite validates the entire HyperCore stack with **151 single-node tests** across 15 categories and **52 multi-node tests** for 5-validator BFT consensus.

**Test Infrastructure:**
- **Language:** TypeScript with `tsx` runner
- **EVM Library:** `viem` for Ethereum interactions
- **Signing:** EIP-712 typed data signing for all exchange operations
- **Accounts:** 3 deterministic Foundry-derived test wallets (Alice, Bob, Charlie)

## Quick Start

### Full E2E Test Run (Single-Node)

```bash
# From the project root - handles Docker, services, tests, and cleanup
./scripts/e2e-test.sh

# Or use make
make test-e2e
```

### Multi-Node Tests (5-Validator Cluster)

```bash
# Full 52-test multinode suite with Docker build
make test-multinode-full

# 3-node basic tests (15 tests)
make test-multinode
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
| `VERBOSE` | `false` | Enable verbose output |
| `GATEWAY_URLS` | - | Multi-node: comma-separated gateway URLs |
| `EVM_RPC_URLS` | - | Multi-node: comma-separated EVM RPC URLs |
| `COMETBFT_RPC_URLS` | - | Multi-node: comma-separated CometBFT RPC URLs |

## Test Categories (151 Single-Node Tests)

### 1. Connection & Health (4 tests)
Validates basic connectivity to all services.

| Test | Description |
|------|-------------|
| Gateway health check | Verify Gateway service is responding |
| Info endpoint available | Verify /info POST endpoint with `universe` array |
| Exchange endpoint available | Verify /exchange POST endpoint exists |
| EVM RPC available | Verify EVM JSON-RPC endpoint is responding |

### 2. Market Data (7 tests)
Tests read-only market data queries with field-level validation.

| Test | Description |
|------|-------------|
| Get exchange metadata | Retrieve markets, validate BTC-PERP/ETH-PERP, maxLeverage |
| Get all mid prices | Validate all prices are valid positive numbers |
| Get L2 orderbook (BTC-PERP) | Validate [bids, asks] levels with px/sz fields |
| Get L2 orderbook (ETH-PERP) | Same validation for ETH-PERP market |
| Get recent trades | Validate trade structure (px, sz, side) |
| Get funding rates | Validate fundingRate field and timestamps |
| Get candles (1h) | Validate OHLCV structure and high >= low invariant |

### 3. Account State (5 tests)
Tests account queries with margin and balance validation.

| Test | Description |
|------|-------------|
| Get Alice account state | Validate marginSummary fields, withdrawable <= accountValue |
| Get Bob account state | Same validation for counterparty |
| Get open orders | Validate order structure (oid, side, limitPx, sz) |
| Get user fills | Validate fill structure (px, sz, side) |
| Get user funding | Validate funding payment timestamps |

### 4. Order Lifecycle (10 tests)
Tests order placement, modification, and cancellation with state verification.

| Test | Description |
|------|-------------|
| Place limit buy order | Verify resting/filled status from response |
| Place limit sell order | Verify resting/filled status from response |
| Place post-only order | Verify order is resting (Alo tif must rest on book) |
| Place IOC order | Verify order is NOT resting (below-market IOC cancelled) |
| Batch place orders | Verify 3 statuses returned, all resting or filled |
| Cancel single order | Query openOrders to verify order removed |
| Cancel all orders | Verify all orders cleared |
| Cancel by CLOID | Query openOrders to verify CLOID order removed |
| USD transfer | Verify Alice's balance decreased after transfer |
| Update leverage | Query state to confirm leverage updated |

### 5. Order Matching (4 tests)
Tests cross-account order matching with price and fill validation.

| Test | Description |
|------|-------------|
| Match limit orders | Assert fill price = $65,000 and size = 0.001 |
| Match with price improvement | Assert fill price in [$65,500, $66,000] range |
| Partial fill | Assert Bob has resting buy with size ~0.004 |
| Clean up | Verify both accounts have 0 orders after cancelAll |

### 6. Position Management (3 tests)
Tests position tracking, leverage, and margin calculations.

| Test | Description |
|------|-------------|
| Check position after trades | Validate BTC-PERP position exists from matching |
| Update leverage | Query clearinghouseState to confirm update |
| Check margin requirements | Validate all 5 marginSummary fields, withdrawable <= accountValue |

### 7. EVM Integration (25 tests)
Comprehensive EVM JSON-RPC method testing.

| Test | Description |
|------|-------------|
| eth_chainId | Query and validate chain ID |
| eth_blockNumber | Validate block height > 0 |
| eth_gasPrice | Validate gas price >= 0 |
| eth_getBalance | Validate bigint return type |
| eth_getTransactionCount | Validate nonce >= 0 |
| eth_getCode | Query code at EOA (should be empty) |
| eth_getStorageAt | Validate hex string response |
| Initialize EVM account | Zero-value self-transfer triggers auto-creation (10^20 wei) |
| eth_sendRawTransaction (ETH transfer) | Execute 0.001 ETH transfer |
| eth_getTransactionByHash | Validate from, to, value match sent tx |
| eth_getTransactionReceipt | Validate status=success, gasUsed > 0, blockNumber >= 1 |
| eth_estimateGas | Validate >= 21000 for transfer |
| eth_getBlockByNumber (latest) | Validate number >= 1, hash exists, timestamp > 0 |
| eth_getBlockByNumber (specific) | Validate number === 1, gasLimit > 0 |
| eth_getBlockByHash | Validate hash round-trip matches |
| eth_call (simple) | Smoke test calling zero address |
| eth_call (precompile) | Smoke test calling HyperCore precompile |
| eth_maxPriorityFeePerGas | Query max priority fee |
| eth_feeHistory | Query fee history for recent blocks |
| web3_clientVersion | Query client version string |
| net_version | Query network version |
| net_listening | Check if node is listening |
| net_peerCount | Query peer count |
| eth_accounts | Query unlocked accounts (empty) |
| eth_getLogs | Query and validate log structure |

### 8. Advanced EVM (9 tests)
Contract deployment and smart contract interactions.

| Test | Description |
|------|-------------|
| Deploy SimpleStorage contract | Deploy contract, verify receipt |
| Verify contract code | Check deployed contract has bytecode |
| Read initial contract state | Call value() getter |
| Write contract state | Call set() to update value |
| Read updated contract state | Verify set() changed the value |
| Read contract storage directly | eth_getStorageAt for raw storage |
| Estimate gas for contract call | Estimate gas for set() call |
| Multiple transactions in sequence | Execute multiple state changes |
| Check nonce increments correctly | Verify nonce >= 5 after transactions |

### 9. Token Standards (11 tests)
ERC20, ERC721, ERC1155 deployment and interaction with event log validation.

| Test | Description |
|------|-------------|
| Deploy ERC20 token | Deploy TEST token contract |
| Verify ERC20 token metadata | Assert symbol === 'TEST', decimals === 18 |
| ERC20 transfer | Verify Alice balance decreased, Bob received exact amount |
| ERC20 Transfer event in receipt | Validate Transfer topic, from/to indexed parameters |
| eth_getLogs returns ERC20 events | Query by contract address, verify event structure |
| Deploy ERC721 NFT | Deploy NFT contract |
| Mint ERC721 NFT | Assert balance === 1n, verify ownerOf(tokenId 0) === Alice |
| ERC721 Transfer event in receipt | Verify Transfer from 0x0 (mint) to Alice |
| Deploy ERC1155 multi-token | Deploy multi-token contract |
| Mint ERC1155 token | Verify token balance after mint |
| ERC1155 balance check | Validate balanceOf returns correct amount |

### 10. Spot Trading (12 tests)
HIP-1 style spot token trading with balance validation.

| Test | Description |
|------|-------------|
| Get spot metadata | Validate spot market structure |
| Get spot markets | Verify TEST-USDC market exists |
| Get spot mid prices | Validate all prices are valid positive numbers |
| Get spot L2 orderbook | Validate orderbook structure |
| Get spot token info | Validate name, weiDecimals, szDecimals, systemAddress |
| Get spot balances | Verify Alice has TEST tokens |
| Place spot buy order | Place and verify order |
| Get spot open orders | Validate order structure (oid, side, limitPx, sz) |
| Get spot open orders (filtered) | Verify returned orders match filter market |
| Place spot sell order | Place matching sell for fill |
| Cancel all spot orders | Assert canceledCount > 0 |
| Verify spot balance after trade | Check balance changes after matching |

### 11. Unified State (18 tests)
Core/EVM view transfers and balance invariant verification.

| Test | Description |
|------|-------------|
| Query unified balances | Retrieve balance breakdown by view |
| Core-to-EVM view transfer | Transfer and verify invariant: total == core + evm |
| EVM-to-Core view transfer | Reverse transfer, verify invariant |
| Invariant check after transfers | Verify total unchanged |
| Insufficient Core view | Specific error message validation |
| Insufficient EVM view | Specific error message validation |
| Multiple token view transfers | Verify core decreased, evm increased, total unchanged |
| Trade after view transfer | Verify trading still works |
| EVM balance reflects evm_view | eth_getBalance matches evm_view |
| Zero amount transfer | Accepts both rejection and no-op |
| Concurrent transfers | Multiple user transfers |
| Full lifecycle | deposit-trade-transfer-withdraw |
| Reserved balance prevents over-transfer | Specific error validation |
| Exact available amount transfer | Boundary condition test |
| Rapid view transfer stress | 10 sequential transfers |
| Large transfer stress | High-value transfer |
| View transfer fee check | No fees charged |
| Bidirectional transfer | Core→EVM→Core round-trip |

### 12. Stress & Performance (3 tests)
System behavior under load with tightened thresholds.

| Test | Description |
|------|-------------|
| Rapid order placement | 10 orders in quick succession, >= 9/10 must succeed |
| Concurrent API requests | 20 simultaneous requests, >= 19/20 must succeed |
| Large orderbook query | Full depth query with [bids, asks] structure validation |

### 13. Advanced Scenarios (18 tests)
Edge cases, error handling, funding mechanics with specific error message validation.

| Test | Description |
|------|-------------|
| Withdraw operation | Verify balance decreases |
| Reduce-only without position | Validate error contains 'Reduce-only' or 'position' |
| Self-trade prevention | Same account buy+sell don't match |
| Invalid price format | Validate error contains 'Invalid' or 'price' |
| Negative order size | Require explicit error, validate message |
| Order on invalid market | Validate error contains 'Market not found' |
| Exceed maximum leverage | Validate error contains 'leverage' |
| Dust amount handling | Validate error contains 'size' or 'minimum' |
| Multi-market order placement | Both BTC-PERP + ETH-PERP verified in book |
| Position lifecycle: open and close | Assert full closure (size ~0) |
| Query funding rate | Validate coin/fundingRate/time fields, rate within ±0.0005 |
| Query user funding payments | Validate all payment fields parseable |
| Funding rate within bounds | All rates within engine max bounds (±0.05%) |
| Funding history data format | Schema validation for funding responses |
| Funding settlement timing | No settlements in last 60s (8h interval) |
| Order modification | Modify existing order parameters |
| Multiple cancel scenarios | Various cancellation patterns |
| Error recovery | System continues after error conditions |

### 14. Risk & Margin (13 tests)
Margin requirements, leverage validation, and fee calculations.

| Test | Description |
|------|-------------|
| Cleanup existing orders | Assert both accounts have 0 orders |
| Margin requirement check | Validate margin fields |
| Leverage 1x, 10x, 25x, 50x | Step through leverage levels |
| Invalid leverage rejection | 100x > 50x max rejected |
| Order rejection on insufficient margin | Over-sized order rejected |
| Track balance change | Assert balance decreased (margin + fees) |
| Fee calculation verification | Verify fees applied correctly |
| Position value tracking | Track notional value changes |
| Risk parameter query | Validate risk fields |
| Multi-leverage scenario | Complex leverage interactions |
| Balance reconciliation | Verify all balances consistent |
| Margin utilization check | Verify margin usage |
| Final cleanup | Assert both accounts have 0 orders |

### 15. State Proofs (9 tests)
Merkle proof generation and client-side verification.

| Test | Description |
|------|-------------|
| Get state info | Validate blockHeight > 0, appHash is 66-char hex |
| Get Alice USDC balance proof | Verify Merkle proof generation |
| Get Bob balance proof | Verify proof for different user |
| Client-side proof verification | keccak256 proof verification |
| Non-existent user proof | Error response for unknown address |
| State root consistency | Match roots across requests |
| Multi-token proof retrieval | Both USDC and TEST tokens |
| Proof structure validation | All required fields with correct types |
| App hash derivation | keccak256(unifiedRoot || nonceRoot) |

## Multi-Node Tests (52 tests)

The multi-node test suite (`tests/multinode.ts`) validates 5-validator BFT consensus.

### Test Sections

| Section | Tests | Coverage |
|---------|-------|----------|
| Cluster Health | 5 | Node health, peers, validators, CometBFT/EVM RPC |
| Transaction Propagation | 10 | Order/cancel/leverage propagation across all nodes |
| State Consistency | 6 | AppHash, balances, markets across all nodes |
| Extended Stability | 3 | Block progression, hash agreement, extended run |
| Cross-Node EVM & Spot | 4 | Contract deploy, spot orders, view transfers, genesis |
| Cross-Node Advanced | 3 | Spot matching, nonce replay, EVM receipts |
| Mixed Transactions | 7 | EVM + perp, failing txs, concurrent storm |
| Node Resilience | 5 | Validator failure, catch-up, double failure, finality |
| Byzantine Fault Tolerance | 8 | Evidence, supermajority, divergence, BFT threshold |

## Test Accounts

Three deterministic Foundry-derived test wallets:

| Name | Address | USDC Balance | TEST Balance |
|------|---------|-------------|-------------|
| Alice | `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` | 100,000 | 10,000 |
| Bob | `0x70997970C51812dc3A010C7d01b50e0d17dc79C8` | 100,000 | 10,000 |
| Charlie | `0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC` | 100,000 | 10,000 |

## Project Structure

```
scripts/e2e/
├── runner.ts              # Main test runner (orchestrates all categories)
├── package.json           # Dependencies (viem, tsx, typescript)
├── tsconfig.json          # TypeScript configuration
├── lib/                   # Shared utilities
│   ├── index.ts           # Central re-export hub
│   ├── config.ts          # Gateway/EVM URLs, chain ID, verbosity
│   ├── accounts.ts        # Test wallet addresses and private keys
│   ├── api.ts             # HTTP helpers for /info and /exchange endpoints
│   ├── signing.ts         # EIP-712 typed data signing for all actions
│   ├── testing.ts         # runTest, skipTest, assertErrorContains, sleep
│   ├── logging.ts         # Colored ANSI output, progress, result formatting
│   └── types.ts           # TypeScript interfaces (TestResult, TestContext)
└── tests/                 # Test modules (one per category)
    ├── connection.ts      # 1. Connection & Health (4 tests)
    ├── market-data.ts     # 2. Market Data (7 tests)
    ├── account.ts         # 3. Account State (5 tests)
    ├── orders.ts          # 4. Order Lifecycle (10 tests)
    ├── matching.ts        # 5. Order Matching (4 tests)
    ├── positions.ts       # 6. Position Management (3 tests)
    ├── evm.ts             # 7. EVM Integration (25 tests)
    ├── evm-advanced.ts    # 8. Advanced EVM (9 tests)
    ├── tokens.ts          # 9. Token Standards (11 tests)
    ├── spot.ts            # 10. Spot Trading (12 tests)
    ├── unified.ts         # 11. Unified State (18 tests)
    ├── stress.ts          # 12. Stress & Performance (3 tests)
    ├── advanced.ts        # 13. Advanced Scenarios (18 tests)
    ├── risk.ts            # 14. Risk & Margin (13 tests)
    ├── state-proofs.ts    # 15. State Proofs (9 tests)
    └── multinode.ts       # Multi-Node: 52 tests for 5-validator BFT
```

## Adding New Tests

Use the `runTest` helper:

```typescript
import { runTest, logProgress, infoRequest } from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

await runTest(
  ctx,
  'Test Name',           // Display name
  'category',            // Category for grouping
  'Description',         // What this test validates
  async () => {
    logProgress('Executing test...');
    const result = await infoRequest('allMids');

    // Use concrete assertions, not just existence checks
    const mids = result as Record<string, string>;
    if (typeof mids !== 'object' || mids === null) {
      throw new Error('Expected object response');
    }

    logProgress('Test passed');
  }
);
```

## Running Standalone

```bash
cd scripts/e2e

# Install dependencies
pnpm install

# Run tests (requires services running)
pnpm test

# Run with verbose output
VERBOSE=true pnpm test
```

## Troubleshooting

### Services not responding

```bash
# Check if services are running
docker compose ps

# View service logs
docker compose logs -f node
```

### Port conflicts

```bash
# Check ports
lsof -i :3000  # Gateway
lsof -i :8545  # EVM RPC
```

### EVM tests fail with OutOfFunds

The EVM tests rely on the auto-creation mechanism in `execute_tx` (dev mode). The "Initialize EVM account" test sends a zero-value self-transfer to trigger account auto-creation with 10^20 wei balance. If this test fails, subsequent value transfer tests will also fail.

## CI/CD Integration

```yaml
# Example GitHub Actions
- name: Run E2E Tests
  run: ./scripts/e2e-test.sh --verbose
```

Exit codes: `0` = all tests passed, `1` = failures detected.
