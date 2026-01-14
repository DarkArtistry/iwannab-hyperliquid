# HyperCore Quick Start Guide

This guide explains the monorepo structure and how to run the HyperCore perpetual futures exchange.

## Repository Structure

```
iwannab-hyperliquid/
├── Cargo.toml                 # Rust workspace root
├── Makefile                   # Build and run commands
├── docker-compose.yml         # Full stack deployment
│
├── crates/                    # Rust crates
│   ├── primitives/            # Core types (Decimal, Order, Position, etc.)
│   ├── engine/                # Matching engine, risk, funding, liquidation
│   ├── chain/                 # ABCI app for CometBFT consensus
│   ├── evm/                   # HyperEVM with precompiles
│   ├── gateway/               # HTTP/WebSocket API server
│   ├── indexer/               # PostgreSQL data indexing
│   └── node/                  # Main binary entry point
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
│   ├── seed-accounts.sh       # Create test accounts
│   └── place-orders.sh        # Sample order placement
│
└── docs/                      # Documentation
    ├── ARCHITECTURE.md        # System architecture
    ├── PROTOCOL.md            # Protocol specification
    ├── API.md                 # API reference
    ├── SECURITY.md            # Security considerations
    └── CODE_AUDIT.md          # Implementation status
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
cargo test
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

**Services and Ports (Phase 2A Architecture):**

| Service | Ports | Description |
|---------|-------|-------------|
| `postgres` | 5432 | PostgreSQL database |
| `node` | **3000 (Gateway)**, 8545 (EVM RPC), 26658 (ABCI) | **Unified node process** |
| `indexer` | - | Blockchain indexer |
| `cometbft` | 26656 (P2P), 26657 (RPC) | Consensus |

**Key Architecture Note (Phase 2A):**
- Gateway API (port 3000) and EVM RPC (port 8545) now run in the **same process**
- This ensures they share the same `UnifiedState` for consistent balance views
- The standalone gateway container is now in the `standalone-gateway` profile (not started by default)

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

## Architecture Overview (Phase 2A)

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
│  (Consensus)  │
│  [Phase 2B]   │
└───────────────┘
```

### Key Concepts (Phase 2A)

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

7. **CometBFT**: BFT consensus via ABCI (Phase 2B - pending integration).
   - Stub: `crates/chain/src/abci.rs`

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
  --abci-addr <ADDR>      CometBFT ABCI listen address [default: 0.0.0.0:26658]
  --http-addr <ADDR>      Node HTTP listen address [default: 0.0.0.0:4000]
  --chain-id <ID>         Chain ID [default: 1337]
```

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

**Resolved in Phase 2A:**
- ✅ ~~Separate state systems~~ - Now uses unified state model
- ✅ ~~Gateway and EVM have different views~~ - Now share same process/state

**Still Pending:**
1. **ABCI server is a stub** - Not actually connected to CometBFT (Phase 2B)
2. **Signature verification is simplified** - Production needs full EIP-712 (Phase 3)
3. **No state persistence** - State is in-memory only (Phase 4)
4. **Some perpetual API endpoints stubbed** - Focus was on spot trading (Phase 3)

## Running Tests

### Rust Tests

```bash
# Run all Rust unit tests (85 tests)
cargo test

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

# Run all integration tests (86 tests)
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

See `sdk/typescript/tests/integration/README.md` for detailed test documentation.

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

**E2E Test Coverage (38 tests):**
| Category | Tests | Description |
|----------|-------|-------------|
| Connection | 4 | Gateway health, endpoints, EVM RPC |
| Market Data | 7 | Orderbook, prices, trades, funding |
| Account | 5 | State, orders, fills, history |
| Orders | 7 | Place, cancel, batch, modify |
| Matching | 4 | Cross orders, price improvement |
| Positions | 3 | Tracking, leverage, margin |
| EVM | 5 | Blocks, balances, transactions |
| Stress | 3 | Rapid orders, concurrent requests |

See `scripts/e2e/README.md` for detailed E2E test documentation.

## Next Steps

1. Review `docs/CODE_AUDIT.md` for implementation gaps
2. Complete missing engine methods in `crates/engine/src/state.rs`
3. Implement proper signature verification
4. Wire up ABCI server with tendermint-abci crate
5. Add state persistence (RocksDB or similar)
