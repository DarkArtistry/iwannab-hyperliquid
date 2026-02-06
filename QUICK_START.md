# HyperCore Quick Start Guide

This guide explains the monorepo structure and how to run the HyperCore perpetual futures exchange.

## Repository Structure

```
iwannab-hyperliquid/
├── Cargo.toml                 # Rust workspace root
├── Makefile                   # Build and run commands
├── docker-compose.yml         # Full stack deployment
│
├── crates/                    # Rust crates (8 crates)
│   ├── primitives/            # Core types (Decimal, Order, Position, UnifiedState)
│   ├── engine/                # Matching engine, risk, funding, liquidation
│   ├── chain/                 # CometBFT ABCI app, Merkle proofs, attestation
│   ├── evm/                   # HyperEVM with precompiles and JSON-RPC
│   ├── gateway/               # HTTP/WebSocket API server with rate limiting
│   ├── indexer/               # PostgreSQL data indexing and candle aggregation
│   ├── persistence/           # RocksDB state persistence (24 column families)
│   └── node/                  # Main binary (single-node & CometBFT modes)
│
├── contracts/                 # Solidity contracts
│   ├── src/
│   │   ├── interfaces/        # IPrecompiles.sol, ICoreWriter.sol
│   │   ├── CoreWriter.sol     # Queued write contract
│   │   ├── HyperCore.sol      # Helper library
│   │   └── examples/          # VaultExample.sol
│   ├── test/                  # Foundry tests
│   └── script/                # Deployment scripts
│
├── sdk/                       # Client SDKs
│   ├── typescript/            # TypeScript/JavaScript SDK
│   └── python/                # Python SDK
│
├── indexer-db/
│   └── migrations/            # PostgreSQL schema migrations
│
├── infra/
│   └── docker/                # Dockerfiles
│
├── scripts/                   # Utility scripts
│   ├── e2e/                   # E2E test suite (151 single-node + 52 multi-node tests)
│   ├── e2e-test.sh            # Single-node E2E orchestration
│   ├── e2e-multinode-full.sh  # 5-validator E2E orchestration
│   ├── seed-accounts.sh       # Create test accounts
│   └── generate-multi-validator-genesis.sh  # N-validator genesis generator
│
└── docs/                      # Documentation
    ├── ARCHITECTURE.md        # System architecture
    ├── PROTOCOL.md            # Protocol specification
    ├── API.md                 # API reference
    ├── CONSENSUS.md           # Consensus verification and BFT properties
    ├── SECURITY.md            # Security considerations
    └── IMPLEMENTATION_STATUS.md  # Detailed implementation analysis
```

## Prerequisites

### Required
- **Rust** 1.85+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Docker** & Docker Compose
- **PostgreSQL** 16+ (or use Docker)

### Optional (for contracts)
- **Foundry** (`curl -L https://foundry.paradigm.xyz | bash && foundryup`)

### Optional (for SDKs)
- **Node.js** 18+ and pnpm
- **Python** 3.10+ and poetry

## Quick Start (Development)

### 1. Build the Rust crates

```bash
# Clone the repository
git clone <repo-url>
cd iwannab-hyperliquid

# Build all crates
cargo build

# Run tests
cargo test --workspace --features cometbft
```

### 2. Start with Docker Compose (Recommended)

This starts all services including PostgreSQL, CometBFT, gateway, indexer, and the HyperCore node:

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f gateway  # API server logs
docker-compose logs -f node     # Node logs
docker-compose logs -f indexer  # Indexer logs

# Stop services
docker-compose down
```

**Services and Ports:**

| Service | Ports | Description |
|---------|-------|-------------|
| `postgres` | 5432 | PostgreSQL database |
| `node` | **3000 (Gateway)**, 8545 (EVM RPC), 26658 (ABCI) | **Unified node process** |
| `indexer` | - | Blockchain indexer |
| `cometbft` | 26656 (P2P), 26657 (RPC) | Consensus |

**Key Architecture Note:**
- Gateway API (port 3000) and EVM RPC (port 8545) run in the **same process**
- This ensures they share the same `UnifiedState` for consistent balance views
- The standalone gateway container is in the `standalone-gateway` profile (not started by default)

### 3. Or run services directly

```bash
# Start PostgreSQL first (needed by indexer)
docker-compose up -d postgres

# Run the node
cargo run -p hypercore-node -- start \
    --abci-addr 0.0.0.0:26658 \
    --http-addr 0.0.0.0:4000 \
    --chain-id 1337

# Run the gateway (in another terminal)
DATABASE_URL=postgres://hypercore:hypercore@localhost:5432/hypercore \
cargo run -p hypercore-gateway -- --http-addr 0.0.0.0:3000

# Run the indexer (in another terminal)
DATABASE_URL=postgres://hypercore:hypercore@localhost:5432/hypercore \
cargo run -p hypercore-indexer
```

### 4. Seed test accounts

```bash
# Make scripts executable
chmod +x scripts/*.sh

# Seed test accounts with USDC
./scripts/seed-accounts.sh
```

## API Endpoints

Once running, the gateway exposes (on port 3000):

| Endpoint | Description |
|----------|-------------|
| `POST /info` | Read-only queries (orderbook, positions, etc.) |
| `POST /exchange` | Signed trading operations |
| `GET /ws` | WebSocket for real-time updates |
| `GET /health` | Health check |

### Example: Get exchange metadata

```bash
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}'
```

### Example: Get orderbook

```bash
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "l2Book", "coin": "BTC-PERP"}'
```

### Example: Get account state

```bash
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "clearinghouseState", "user": "0xYourAddress"}'
```

## Using the SDKs

### TypeScript SDK

```bash
cd sdk/typescript
pnpm install
pnpm build
```

```typescript
import { HyperCore } from './src/client';

const client = new HyperCore({
  baseUrl: 'http://localhost:3000',
  privateKey: '0x...',
});

// Get account state
const state = await client.info.getAccountState(client.address);

// Place an order
const result = await client.exchange.placeOrder({
  market: 'BTC-PERP',
  side: 'buy',
  size: '0.01',
  price: '50000',
  orderType: 'limit',
});
```

### Python SDK

```bash
cd sdk/python
pip install -e .
# or with poetry
poetry install
```

```python
import asyncio
from hypercore import HyperCore

async def main():
    client = HyperCore(
        base_url="http://localhost:3000",
        private_key="0x..."
    )

    # Get account state
    state = await client.info.get_account_state(client.address)
    print(state)

    # Place an order
    result = await client.exchange.place_order(
        market="BTC-PERP",
        side="buy",
        size="0.01",
        price="50000",
    )

    await client.close()

asyncio.run(main())
```

## Building Contracts

```bash
cd contracts

# Install dependencies
forge install

# Build
forge build

# Test
forge test

# Deploy (local)
forge script script/Deploy.s.sol --rpc-url http://localhost:8545 --broadcast
```

## Architecture Overview

```
                    ┌─────────────────────────────────────────────────────────┐
                    │                    Client SDKs                           │
                    │              (TypeScript, Python, etc.)                  │
                    └─────────────────────────┬───────────────────────────────┘
                                              │
                              ┌───────────────┴───────────────┐
                              │                               │
                              ▼                               ▼
                    ┌─────────────────┐             ┌─────────────────┐
                    │  Gateway API    │             │   EVM RPC       │
                    │  POST /info     │             │   eth_*         │
                    │  POST /exchange │             │   Port 8545     │
                    │  Port 3000      │             │                 │
                    └────────┬────────┘             └────────┬────────┘
                             │                               │
                             │   ┌───────────────────────┐   │
                             └───│  SHARED UNIFIED STATE │───┘
                                 │  Arc<RwLock<...>>     │
                                 │                       │
                                 │  UnifiedBalance:      │
                                 │  ├─ total: 100,000    │
                                 │  ├─ core_view: 80,000 │ ◄── SpotEngine reads
                                 │  └─ evm_view: 20,000  │ ◄── EvmExecutor reads
                                 └───────────────────────┘
                                              │
        ┌─────────────────────────────────────┼─────────────────────────────────────┐
        │                                     │                                     │
        ▼                                     ▼                                     ▼
┌───────────────────┐               ┌───────────────────┐               ┌───────────────────┐
│    SpotEngine     │               │   EvmExecutor     │               │     Indexer       │
│  - Orderbook      │◄──Precompiles─│  - revm           │               │   (PostgreSQL)    │
│  - Matching       │               │  - Contracts      │               │  - Blocks, Trades │
│  - Balance Reserve│               │  - Gas accounting │               │  - Positions      │
└───────┬───────────┘               └───────────────────┘               └───────────────────┘
        │
┌───────▼───────┐
│   CometBFT    │
│ (BFT Consensus)│
│               │
└───────────────┘
```

### Key Concepts

1. **Unified State Model**: Single master balance sheet with views (`core_view`, `evm_view`). Both Gateway and EVM RPC see the same state because they run in the same process.

2. **SpotEngine**: Orderbook matching engine that reads from `core_view`. Reserves balance when orders are placed, releases on fill/cancel.
   - Source: `crates/engine/src/spot_engine.rs`

3. **EvmExecutor**: EVM execution using `revm`. Reads from `evm_view` for balance checks and gas deduction.
   - Source: `crates/evm/src/executor.rs`

4. **Precompiles**: Custom EVM precompiles (0x0800-0x0808) that read exchange state:
   - 0x0800-0x0805: Perpetual precompiles (positions, marks, funding)
   - 0x0806-0x0808: Spot precompiles (balances, orderbook)
   - Source: `crates/evm/src/precompiles.rs`

5. **View Transfers**: Moving funds between Core and EVM is NOT a bridge transfer - it just adjusts views:
   ```rust
   // total stays the same, only views change
   core_view -= amount;
   evm_view += amount;
   ```
   - Source: `crates/primitives/src/unified_state.rs:transfer_to_evm_view()`

6. **Gateway API**: REST-like API for trading operations. Exchange actions require EIP-712 signatures.
   - Source: `crates/gateway/src/handlers.rs`

7. **CometBFT**: BFT consensus via ABCI (Phase 2B - COMPLETE ✅).
   - ABCI Server: `crates/chain/src/cometbft/server.rs`
   - Application: `crates/chain/src/cometbft/app.rs`
   - Validators: `crates/chain/src/cometbft/validators.rs`

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HTTP_ADDR` | `0.0.0.0:3000` | Gateway HTTP listen address |
| `ABCI_ADDR` | `0.0.0.0:26658` | Node ABCI listen address |
| `CHAIN_ID` | `1337` | Chain ID for EIP-712 |
| `DATABASE_URL` | - | PostgreSQL connection string |
| `RUST_LOG` | `info` | Logging level |

### CLI Options

**Node:**
```bash
cargo run -p hypercore-node -- start --help

Options:
  --abci-addr <ADDR>         CometBFT ABCI listen address [default: 0.0.0.0:26658]
  --http-addr <ADDR>         Node HTTP listen address [default: 0.0.0.0:4000]
  --chain-id <ID>            Chain ID [default: 1337]
  --consensus-mode <MODE>    Consensus mode: single-node or cometbft [default: single-node]
  --block-time-ms <MS>       Block production interval in ms (single-node mode) [default: 100]
  --enable-persistence       Enable state persistence to RocksDB (requires --features persistence)
  --data-dir <PATH>          Directory for persistent state storage [default: ./data]
```

**Consensus Modes:**
- `single-node`: Uses BlockProducer for local development (default)
- `cometbft`: Connects to CometBFT network via ABCI (requires `--features cometbft`)

**Persistence Mode:**
- By default, state is in-memory only (lost on restart)
- Enable persistence with `--enable-persistence` (requires building with `--features persistence`)
- State is automatically saved after each block commit
- State is restored on node startup if persisted data exists

**Gateway:**
```bash
cargo run -p hypercore-gateway -- --help

Options:
  --http-addr <ADDR>      HTTP API listen address [default: 0.0.0.0:3000]
  --chain-id <ID>         Chain ID [default: 1337]
  --enable-websocket      Enable WebSocket support [default: true]
```

**Indexer:**
```bash
cargo run -p hypercore-indexer -- --help

Options:
  --database-url <URL>    PostgreSQL connection URL (required, or via DATABASE_URL env)
  --run-migrations        Run migrations on startup [default: true]
```

## Makefile Commands

```bash
make build          # Build all Rust crates
make test           # Run all tests
make devnet-up      # Start Docker Compose stack
make devnet-down    # Stop Docker Compose stack
make devnet-logs    # View logs
make seed           # Seed test accounts
make clean          # Clean build artifacts
```

## Troubleshooting

### Build fails with missing dependencies

```bash
# Install system dependencies (Ubuntu)
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev

# Install system dependencies (macOS)
brew install openssl pkg-config
```

### Database connection fails

```bash
# Ensure PostgreSQL is running
docker-compose up -d postgres

# Check connection
psql $DATABASE_URL -c "SELECT 1"
```

### Port already in use

```bash
# Check what's using port 3000 (gateway)
lsof -i :3000

# Kill the process or use a different port
cargo run -p hypercore-gateway -- --http-addr 0.0.0.0:3001
```

## Current Limitations

See `docs/IMPLEMENTATION_STATUS.md` for detailed implementation status. Key limitations:

All core features are implemented. The only remaining item is:
1. **External Security Audit** - Third-party audit before mainnet deployment

## Running Tests

### Rust Tests

```bash
# Run all Rust unit tests (556 tests)
cargo test --workspace --features cometbft

# Run tests for specific crate
cargo test -p hypercore-engine
cargo test -p hypercore-primitives
```

### Solidity Contract Tests

```bash
cd contracts

# Run all Foundry tests (49 tests)
forge test

# Run specific test file
forge test --match-path test/Integration.t.sol

# Run with verbosity
forge test -vvv
```

### TypeScript SDK Integration Tests

The SDK includes comprehensive integration tests that also serve as documentation:

```bash
cd sdk/typescript

# Install dependencies
pnpm install

# Run all integration tests
pnpm test:integration

# Run specific test file
pnpm test tests/integration/04-orders.test.ts

# Watch mode
pnpm test:watch
```

**Test Coverage:**
| Test File | Tests | Coverage |
|-----------|-------|----------|
| 01-connection.test.ts | 7 | Gateway connectivity, auth |
| 02-market-data.test.ts | 14 | Order book, prices, candles |
| 03-account.test.ts | 13 | Account state, fills, transfers |
| 04-orders.test.ts | 17 | Order types, cancel, modify |
| 05-matching.test.ts | 10 | Matching, partial fills |
| 06-positions.test.ts | 14 | Positions, leverage, PnL |
| 07-advanced.test.ts | 11 | Market making, stress tests |

### Full E2E Integration Tests

A comprehensive E2E test script that manages the entire test lifecycle:

```bash
# Full run: stops services, starts fresh Docker, runs all tests, cleanup
./scripts/e2e-test.sh

# Skip Docker setup (use already running services)
./scripts/e2e-test.sh --no-docker

# Keep services running after tests (for debugging)
./scripts/e2e-test.sh --keep

# Verbose output with progress details
./scripts/e2e-test.sh --verbose
```

**E2E Test Coverage (151 tests):**
| Category | Tests | Description |
|----------|-------|-------------|
| Connection | 4 | Gateway health, endpoints, EVM RPC |
| Market Data | 7 | Orderbook, prices, trades, funding |
| Account | 5 | State, orders, fills, history |
| Orders | 10 | Place, cancel, batch, USD transfer, leverage |
| Matching | 4 | Cross orders, price improvement |
| Positions | 3 | Tracking, leverage, margin |
| EVM | 25 | All eth_* methods, blocks, transactions |
| EVM Advanced | 9 | Contract deployment, storage, nonces |
| Token Standards | 11 | ERC20, ERC721, ERC1155 |
| Spot Trading | 12 | Spot orders, balances, cancellation |
| Unified State | 18 | View transfers, balance invariants |
| Stress | 3 | Rapid orders, concurrent requests |
| Risk & Margin | 13 | Margin requirements, leverage limits, liquidation |
| State Proofs | 9 | State commitment, Merkle proofs, verification |
| Advanced | 18 | Withdraw, reduce-only, error handling, position lifecycle |

See `scripts/e2e/README.md` for detailed E2E test documentation.

## Next Steps

All core phases are complete:
- ✅ EVM Integration with revm, HIP-1 Spot Trading (Phase 1)
- ✅ Unified state model, CometBFT consensus (Phase 2A/2B)
- ✅ Genesis state, gas fees, Merkle proofs, EIP-712 (Phase 3)
- ✅ RocksDB persistence with state save/restore (Phase 4)
- ✅ Indexer with candle aggregation (Phase 5)
- ✅ Rate limiting, input validation (Phase 6)
- ✅ State attestation, P2P gossip, re-org handling (Phase 7)
- ✅ Multi-node BFT consensus (5-validator, 52 E2E tests)

**Remaining Work:**
1. External security audit (required before mainnet)
2. Review `docs/IMPLEMENTATION_STATUS.md` for detailed status

## Genesis State Configuration

The blockchain initializes from a genesis configuration that includes:

**Genesis Structure (`crates/node/src/main.rs:create_genesis()`):**
```json
{
  "chain_id": "hypercore-1337",
  "app_state": {
    "markets": [
      { "id": 0, "symbol": "BTC-PERP", "max_leverage": 50 },
      { "id": 1, "symbol": "ETH-PERP", "max_leverage": 50 }
    ],
    "spot_tokens": [
      { "index": 1, "symbol": "TEST", "wei_decimals": 18 }
    ],
    "balances": [
      { "address": "0xf39F...", "token": 0, "amount": "100000", "view": "core" },
      { "address": "0xf39F...", "token": 1, "amount": "10000", "view": "core" }
    ]
  }
}
```

**Key Concepts:**
- **Token Index 0** = USDC (6 decimals)
- **Token Index 1+** = Custom tokens (configurable decimals)
- **View "core"** = Available for trading (SpotEngine reads this)
- **View "evm"** = Available for smart contracts (EvmExecutor reads this)

**Generating Genesis:**
```bash
# Generate genesis.json file
cargo run -p hypercore-node -- init --output genesis.json --chain-id 1337

# Start node (uses create_genesis() internally for single-node mode)
cargo run -p hypercore-node -- start --http-addr 0.0.0.0:3000
```

**Test Accounts (Standard Anvil/Hardhat):**
| Name | Address | USDC | TEST |
|------|---------|------|------|
| Alice | `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` | 100,000 | 10,000 |
| Bob | `0x70997970C51812dc3A010C7d01b50e0d17dc79C8` | 100,000 | 10,000 |
| Charlie | `0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC` | 100,000 | 10,000 |

## State Persistence (Phase 4)

HyperCore supports durable state persistence using RocksDB. This allows the node to survive restarts without losing state.

### Enabling Persistence

```bash
# Build with persistence feature
cargo build -p hypercore-node --features persistence

# Start with persistence enabled
cargo run -p hypercore-node --features persistence -- start \
    --enable-persistence \
    --data-dir ./data/chain \
    --http-addr 0.0.0.0:3000

# Or use release build for production
cargo build -p hypercore-node --features persistence --release
./target/release/hypercore-node start \
    --enable-persistence \
    --data-dir /var/lib/hypercore/data
```

### How It Works

1. **Automatic State Save**: After each block is committed, the `PostCommitHandler` callback triggers state extraction and persistence to RocksDB.

2. **State Restore on Startup**: When the node starts with `--enable-persistence`, it checks if persisted state exists in the data directory. If found, it restores the state instead of initializing from genesis.

3. **Column Families**: State is organized into 24 RocksDB column families:
   - `balances`, `positions`, `orders`, `accounts` (trading state)
   - `spot_tokens`, `spot_markets`, `spot_reserved` (spot state)
   - `evm_accounts`, `evm_storage`, `evm_code` (EVM state)
   - `nonces`, `block_meta`, `block_hashes` (chain state)
   - `fills`, `funding_payments`, `trades` (history)

4. **WAL (Write-Ahead Log)**: RocksDB's WAL ensures crash recovery - incomplete writes are replayed on startup.

### Data Directory Structure

```
./data/chain/
├── hypercore.db/          # RocksDB database
│   ├── 000001.log         # WAL files
│   ├── 000002.sst         # SST data files
│   ├── CURRENT            # Current manifest
│   ├── MANIFEST-000001    # Database manifest
│   └── OPTIONS-000001     # Database options
└── genesis.json           # Genesis configuration (if exported)
```

### Switching Between Modes

```bash
# In-memory mode (default, for development)
cargo run -p hypercore-node -- start

# Persistent mode (for production)
cargo run -p hypercore-node --features persistence -- start \
    --enable-persistence \
    --data-dir ./data/chain
```

**Note**: If you switch from persistent to in-memory mode, the persisted state is NOT loaded. Use persistent mode consistently for production deployments.
