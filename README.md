# HyperCore

A high-performance perpetual futures exchange with an integrated EVM environment, inspired by [Hyperliquid's](https://hyperliquid.xyz/) architecture.

[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/solidity-0.8.24-blue.svg)](https://soliditylang.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

## Architecture

HyperCore uses a **unified state model** where all components share the same balance sheet. This matches Hyperliquid's architecture - there's no bridging between layers, just different "views" of the same state.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Client Applications                                │
│                   (Trading Bots, Web UI, Mobile Apps)                        │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │
                        ┌───────────▼───────────┐
                        │   TypeScript/Python   │
                        │         SDKs          │
                        └───────────┬───────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    │                               │
                    ▼                               ▼
        ┌───────────────────┐           ┌───────────────────┐
        │   Gateway API     │           │   EVM JSON-RPC    │
        │   Port 3000       │           │   Port 8545       │
        │   /info, /exchange│           │   eth_*, web3_*   │
        └─────────┬─────────┘           └─────────┬─────────┘
                  │                               │
                  │     ┌───────────────────┐     │
                  └────►│  UNIFIED STATE    │◄────┘
                        │  (Single Process) │
                        │                   │
                        │  UnifiedBalance { │
                        │    total: 100,000 │
                        │    core_view: 80k │◄── SpotEngine reads
                        │    evm_view: 20k  │◄── EvmExecutor reads
                        │  }                │
                        └─────────┬─────────┘
                                  │
            ┌─────────────────────┼─────────────────────┐
            │                     │                     │
            ▼                     ▼                     ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│   SpotEngine      │   │   EvmExecutor     │   │     Indexer       │
│   - Orderbooks    │◄──│   - revm          │   │   (PostgreSQL)    │
│   - Matching      │   │   - Precompiles   │   │   - Trade History │
│   - Reserves      │   │   - Contracts     │   │   - Analytics     │
└─────────┬─────────┘   └───────────────────┘   └───────────────────┘
          │
          ▼
┌───────────────────┐
│     CometBFT      │
│   (Consensus)     │
│   [Phase 2B]      │
└───────────────────┘
```

### Key Architecture Concepts

| Concept | Description | Source |
|---------|-------------|--------|
| **Unified State** | Single balance sheet with views (core_view, evm_view) | `crates/primitives/src/unified_state.rs` |
| **Shared Process** | Gateway and EVM RPC run in same process, share state | `crates/node/src/main.rs:121-165` |
| **View Transfers** | Move funds between Core/EVM by adjusting views (no bridging!) | `unified_state.rs:203-259` |
| **Reserved Balances** | Resting orders reserve funds to prevent double-spend | `crates/engine/src/spot_engine.rs` |
| **Precompiles** | EVM contracts read exchange state via 0x0800-0x0808 | `crates/evm/src/precompiles.rs` |

## Features

### Trading Features
- **Perpetual Futures**: BTC-PERP, ETH-PERP with up to 50x leverage
- **Order Types**: Limit, Market, Post-Only, IOC (Immediate-or-Cancel), FOK (Fill-or-Kill)
- **Advanced Orders**: Reduce-only, Client Order IDs, Batch operations
- **Price-Time Priority**: Fair matching with deterministic execution

### Technical Features
- **Sub-second Finality**: ~500ms block time with instant finality
- **EVM Integration**: Deploy Solidity contracts that read exchange state
- **Custom Precompiles**: Gas-efficient state reads (positions, orderbook, funding)
- **Real-time Updates**: WebSocket feeds for trades, orderbook, user events

### Developer Features
- **TypeScript SDK**: Full-featured client with 86 integration tests
- **Python SDK**: Async client for algorithmic trading
- **Foundry Integration**: 49 Solidity tests for smart contracts
- **Comprehensive Docs**: Architecture, API, and protocol specifications

## Quick Start

### Prerequisites

- **Rust** 1.85+ - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Docker** & Docker Compose
- **Node.js** 18+ and pnpm (for TypeScript SDK)
- **Foundry** (for contracts) - `curl -L https://foundry.paradigm.xyz | bash && foundryup`

### 1. Build and Test

```bash
# Clone repository
git clone <repo-url>
cd iwannab-hyperliquid

# Build all Rust crates
cargo build

# Run tests (85 Rust tests)
cargo test
```

### 2. Start Services with Docker

```bash
# Start all services
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f gateway
```

### 3. Deploy Contracts (Local)

```bash
# Start local EVM (Anvil)
anvil --chain-id 1337

# Deploy contracts (in another terminal)
cd contracts
forge script script/Deploy.s.sol \
  --rpc-url http://127.0.0.1:8545 \
  --broadcast \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --tc DeployScript
```

### 4. Run SDK Integration Tests

```bash
cd sdk/typescript
pnpm install
pnpm test:integration  # 86 tests
```

See [QUICK_START.md](QUICK_START.md) for detailed setup instructions.

## Repository Structure

```
iwannab-hyperliquid/
├── crates/                    # Rust crates
│   ├── primitives/            # Core types (Decimal, Order, Position)
│   ├── engine/                # Matching engine, risk, funding
│   ├── chain/                 # CometBFT ABCI application
│   ├── evm/                   # HyperEVM with precompiles
│   ├── gateway/               # HTTP/WebSocket API server
│   ├── indexer/               # PostgreSQL data indexing
│   └── node/                  # Main binary entry point
│
├── contracts/                 # Solidity smart contracts
│   ├── src/
│   │   ├── CoreWriter.sol     # Queued write operations
│   │   ├── HyperCore.sol      # State reading library
│   │   └── interfaces/        # Precompile interfaces
│   └── test/                  # Foundry tests (49 tests)
│
├── sdk/                       # Client SDKs
│   ├── typescript/            # TypeScript SDK (86 integration tests)
│   └── python/                # Python SDK
│
├── docs/                      # Documentation
│   ├── ARCHITECTURE.md        # System design
│   ├── API.md                 # API reference
│   ├── PROTOCOL.md            # Protocol specification
│   └── SECURITY.md            # Security model
│
├── scripts/                   # Utility scripts
├── indexer-db/                # Database migrations
└── infra/                     # Docker configurations
```

## API Examples

### Get Market Data

```bash
# Get exchange metadata
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}'

# Get BTC-PERP orderbook
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "l2Book", "coin": "BTC-PERP"}'
```

### Place Order (TypeScript)

```typescript
import { HyperCore, OrderSide, OrderType } from '@hypercore/sdk';

const client = new HyperCore({
  baseUrl: 'http://localhost:3000',
  privateKey: '0x...',
  chainId: 1337,
});

// Place limit buy order
const result = await client.exchange.placeOrder({
  market: 'BTC-PERP',
  side: OrderSide.Buy,
  price: '65000',
  size: '0.1',
  type: OrderType.Limit,
});

console.log('Order placed:', result);
```

### Read State from Solidity

```solidity
import {HyperCore} from "./HyperCore.sol";

contract MyStrategy {
    function checkPosition(address user, uint8 marketId) external view {
        // Read position via precompile
        int256 size = HyperCore.getPositionSize(user, marketId);
        uint256 equity = HyperCore.getEquity(user);

        // Read market data
        uint256 markPrice = HyperCore.getMarkPrice(marketId);
        (uint256 bid, uint256 ask) = HyperCore.getBestBidAsk(marketId);
    }
}
```

## Test Coverage

| Component | Tests | Status |
|-----------|-------|--------|
| Rust Unit Tests | 85+ | ✅ Passing |
| Solidity Contracts | 49 | ✅ Passing |
| E2E Integration | 104+ | ✅ Passing |
| **Total** | **238+** | **All Passing** |

### Running All Tests

```bash
# Rust tests
cargo test

# Solidity tests
cd contracts && forge test

# TypeScript SDK integration tests
cd sdk/typescript && pnpm test:integration

# Full E2E test suite (starts services, runs tests, cleanup)
./scripts/e2e-test.sh
```

### E2E Test Suite

The comprehensive E2E test script (`scripts/e2e-test.sh`) provides:

- **Automatic environment setup**: Stops existing services, starts Docker containers
- **Service health checking**: Waits for all services to be healthy
- **Contract deployment**: Deploys Solidity contracts to local EVM
- **Complete test coverage**: Tests all APIs, order matching, EVM integration
- **Detailed reporting**: Progress output, category summaries, final report
- **Automatic cleanup**: Stops services after tests complete

```bash
# Full run with Docker
./scripts/e2e-test.sh

# Skip Docker, use existing services
./scripts/e2e-test.sh --no-docker

# Keep services running after tests
./scripts/e2e-test.sh --keep

# Verbose output
./scripts/e2e-test.sh --verbose
```

See [scripts/e2e/README.md](scripts/e2e/README.md) for detailed test documentation.

## Documentation

| Document | Description |
|----------|-------------|
| [QUICK_START.md](QUICK_START.md) | Setup guide and tutorials |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System design and unified state model |
| [docs/TRANSACTION_FLOW.md](docs/TRANSACTION_FLOW.md) | How transactions flow through the system |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | Protocol specification |
| [docs/API.md](docs/API.md) | REST and WebSocket API reference |
| [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) | Detailed implementation analysis |
| [TODO.md](TODO.md) | Development roadmap |
| [contracts/README.md](contracts/README.md) | Smart contract documentation |
| [sdk/typescript/README.md](sdk/typescript/README.md) | TypeScript SDK guide |
| [sdk/python/README.md](sdk/python/README.md) | Python SDK guide |

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GATEWAY_URL` | `http://localhost:3000` | Gateway endpoint |
| `DATABASE_URL` | - | PostgreSQL connection |
| `CHAIN_ID` | `1337` | EVM chain ID |
| `RUST_LOG` | `info` | Logging level |

### Service Ports

| Service | Port | Description |
|---------|------|-------------|
| **Node (Gateway)** | 3000 | REST API (/info, /exchange) |
| **Node (EVM RPC)** | 8545 | EVM JSON-RPC (eth_*, web3_*) |
| Node ABCI | 26658 | CometBFT ABCI |
| PostgreSQL | 5432 | Database |
| CometBFT RPC | 26657 | Consensus RPC |

**Note:** Gateway (port 3000) and EVM RPC (port 8545) run in the **same process** to share unified state.

## Development

### Building

```bash
# Development build
cargo build

# Release build
cargo build --release

# Build specific crate
cargo build -p hypercore-gateway
```

### Code Style

```bash
# Format Rust code
cargo fmt

# Lint Rust code
cargo clippy

# Format Solidity
cd contracts && forge fmt

# Lint TypeScript
cd sdk/typescript && pnpm lint
```

### Adding Tests

- **Rust**: Add tests in `crates/*/src/*.rs` using `#[cfg(test)]`
- **Solidity**: Add tests in `contracts/test/*.t.sol`
- **TypeScript**: Add tests in `sdk/typescript/tests/integration/*.test.ts`

## Implementation Status

This is a development implementation. See [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) for detailed analysis.

### What's Working (✅)

| Component | Status | Key Source |
|-----------|--------|------------|
| **Unified State Model** | ✅ | `crates/primitives/src/unified_state.rs` |
| **Spot Trading Engine** | ✅ | `crates/engine/src/spot_engine.rs` |
| **Order Matching** | ✅ | `crates/engine/src/matching.rs` |
| **Balance Reserves** | ✅ | Resting orders reserve funds |
| **EVM Execution (revm)** | ✅ | `crates/evm/src/executor.rs` |
| **EVM JSON-RPC** | ✅ | `crates/evm/src/rpc.rs` |
| **Precompiles (0x0800-0x0808)** | ✅ | `crates/evm/src/precompiles.rs` |
| **Gateway API** | ✅ | `crates/gateway/src/handlers.rs` |
| **View Transfers** | ✅ | Core ↔ EVM without bridging |
| **HIP-1 Spot Tokens** | ✅ | Token deployment and trading |
| **Risk/Margin Engine** | ✅ | `crates/engine/src/risk.rs` |
| **Funding Rate** | ✅ | `crates/engine/src/funding.rs` |

### What's Stubbed (❌)

| Component | Status | Notes |
|-----------|--------|-------|
| **CometBFT Connection** | ❌ Stub | ABCI server sleeps, no real consensus |
| **State Persistence** | ❌ Stub | In-memory only, lost on restart |
| **Signature Verification** | ❌ Stub | Bypassed for testing |
| **Historical Queries** | ❌ Stub | Fill/funding history not implemented |

### Development Phases

| Phase | Focus | Status |
|-------|-------|--------|
| Phase 1 | EVM Integration, Precompiles, HIP-1 Tokens | ✅ Complete |
| Phase 2A | Unified State Model | ✅ Complete |
| Phase 2B | CometBFT Consensus | ❌ Pending |
| Phase 3 | Signature Verification | ❌ Pending |
| Phase 4 | State Persistence (RocksDB) | ❌ Pending |

See [TODO.md](TODO.md) for the development roadmap.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit changes (`git commit -m 'Add amazing feature'`)
4. Push to branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [Hyperliquid](https://hyperliquid.xyz/) - Architecture inspiration
- [CometBFT](https://cometbft.com/) - Consensus layer
- [revm](https://github.com/bluealloy/revm) - EVM implementation
- [Foundry](https://getfoundry.sh/) - Solidity development toolchain
