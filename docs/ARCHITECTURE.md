# Architecture Overview

## System Design

HyperCore is a high-performance perpetual futures exchange with an integrated EVM environment. The architecture follows a dual-execution model inspired by Hyperliquid's HyperCore + HyperEVM design.

**Phase 2A Architecture: Unified State Model** (IMPLEMENTED)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Client Applications                            │
│                 (Trading Bots, Web UI, Mobile Apps)                      │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
              ┌───────────────────┴───────────────────┐
              │                                       │
              ▼                                       ▼
┌─────────────────────────┐               ┌─────────────────────────┐
│   HyperCore Gateway     │               │    EVM JSON-RPC         │
│   POST /info, /exchange │               │    Port 8545            │
│   Port 3000             │               │    (eth_*, web3_*)      │
└────────────┬────────────┘               └────────────┬────────────┘
             │                                         │
             │      ┌─────────────────────────────┐   │
             │      │                             │   │
             └─────►│   SHARED UNIFIED STATE      │◄──┘
                    │   Arc<RwLock<UnifiedState>> │
                    │                             │
                    │  ┌───────────────────────┐  │
                    │  │    UnifiedBalance     │  │
                    │  │  ┌─────────────────┐  │  │
                    │  │  │ total: 100,000  │  │  │
                    │  │  │ core_view: 80k  │◄─┼──┼── SpotEngine reads
                    │  │  │ evm_view: 20k   │──┼──┼──► EvmExecutor reads
                    │  │  └─────────────────┘  │  │
                    │  └───────────────────────┘  │
                    └─────────────────────────────┘
                                  │
              ┌───────────────────┴───────────────────┐
              │                                       │
              ▼                                       ▼
┌─────────────────────────┐               ┌─────────────────────────┐
│     SpotEngine          │               │     EvmExecutor         │
│  - Order matching       │◄──Precompiles─│  - Contract execution   │
│  - Balance reserves     │               │  - Gas accounting       │
│  - Spot markets         │               │  - State reads          │
└─────────────────────────┘               └─────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           CometBFT Consensus                             │
│                    (Block Production, P2P, Finality)                     │
│                         [Phase 2B - Pending]                             │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key Architecture Decisions (Phase 2A)

1. **Single Process**: Gateway (port 3000) and EVM RPC (port 8545) run in the **same process**
   - Source: `crates/node/src/main.rs:121-165`
   - This ensures both share the same `SharedUnifiedState`

2. **Unified Balance Sheet**: All balances stored in one structure with views
   - Source: `crates/primitives/src/unified_state.rs`
   - `UnifiedBalance { total, core_view, evm_view }`

3. **View-Based Separation**: Trading and EVM use different "views" of the same balance
   - `core_view`: Available for HyperCore trading (orders, margin)
   - `evm_view`: Available for HyperEVM (gas, DeFi contracts)
   - View transfers don't change total - just move between views

## Component Responsibilities

### Primitives (`crates/primitives`)
Shared types used across all components:
- Order, Position, Market types
- Fixed-point decimal arithmetic
- Serialization formats
- Error types

### Engine (`crates/engine`)
Core matching and risk logic (no network/consensus awareness):
- OrderBook implementation (price-time priority)
- Deterministic matching algorithm
- Margin calculations
- Funding rate computation
- Liquidation logic

### Chain (`crates/chain`)
Consensus integration layer:
- ABCI application implementation
- Block processing pipeline
- State commitment (Merkle roots)
- Genesis handling
- Validator set management (future)

### EVM (`crates/evm`)
EVM execution environment:
- revm-based executor
- Custom precompiles for engine reads
- CoreWriter action queue processing
- EVM state management

### Gateway (`crates/gateway`)
External API surface:
- HTTP REST API (/info, /exchange)
- WebSocket feeds (orderbook, trades)
- Request signing/verification
- Rate limiting

### Indexer (`crates/indexer`)
Historical data service:
- Block/event ingestion
- PostgreSQL persistence
- Query optimization
- Candle generation

### Node (`crates/node`)
Binary entry point combining all components.

## Data Flow

### Order Placement (Direct to Engine)
```
1. User signs order → POST /exchange
2. Gateway validates signature
3. Gateway forwards to node via internal RPC
4. Node includes in next block proposal
5. Block committed → Engine executes match
6. Fills emitted as events
7. Indexer captures fills
8. WebSocket broadcasts to subscribers
```

### Order Placement (via EVM CoreWriter)
```
1. User calls CoreWriter.placeOrder() on EVM
2. EVM tx included in block
3. CoreWriter emits ActionQueued event
4. End of block: Node reads ActionQueued events
5. Actions queued for NEXT block's engine processing
6. Next block: Engine executes queued actions
7. Results available via precompile reads
```

### State Reads (EVM Precompile)
```
1. Solidity contract calls precompile (e.g., 0x0800)
2. revm routes to custom precompile handler
3. Handler reads from Engine state (read-only)
4. Returns ABI-encoded result to contract
```

## Consensus Model

We use CometBFT (Tendermint) for BFT consensus:
- **Block time**: 500ms target
- **Finality**: Instant (single-slot)
- **Validator set**: Initially single validator, expand in hardening phase

### Block Processing Order
```
BeginBlock
  → Settle funding (if interval elapsed)
  → Process liquidations

DeliverTx (repeated for each tx)
  → If Engine action: execute immediately, emit events
  → If EVM tx: execute EVM, collect CoreWriter events

EndBlock
  → Queue CoreWriter actions for next block
  → Compute state commitment
  → Emit block-level events

Commit
  → Persist state
  → Notify indexer
```

## State Management

### Consensus State (Replicated)
All validators must agree:
- Account balances
- Positions
- Market parameters
- Insurance fund
- Nonces

### Engine State (Derived)
Reconstructable from action history:
- Orderbook state
- Open orders per user
Can be rebuilt from genesis + all blocks.

### EVM State (Replicated)
Standard EVM state trie:
- Contract code
- Contract storage
- EVM account balances

### Indexed State (Off-chain)
Not consensus-critical:
- Trade history
- Candles
- User activity

## Precompile Architecture

Read precompiles are stateless functions that:
1. Accept ABI-encoded input
2. Read from engine/consensus state
3. Return ABI-encoded output

They execute synchronously during EVM execution and have no side effects.

| Address | Name | Gas Cost |
|---------|------|----------|
| 0x0800 | PositionReader | 2000 |
| 0x0801 | AccountReader | 2500 |
| 0x0802 | MarketReader | 1500 |
| 0x0803 | OrderReader | 3000 + 500/order |
| 0x0804 | FundingReader | 1000 |
| 0x0805 | OrderBookReader | 2000 + 200/level |

## CoreWriter Queue

The CoreWriter contract emits events that the node processes:

```solidity
event ActionQueued(
    bytes32 indexed actionId,
    address indexed sender,
    uint8 actionType,
    bytes data
);
```

Processing flow:
1. EVM executes CoreWriter call → event emitted
2. EndBlock: Collect all ActionQueued events from block
3. Validate actions (signature not needed; msg.sender is auth)
4. Queue for next block's engine processing
5. Next BeginBlock: Execute queued actions in order
6. Emit ActionExecuted events with results

This non-atomic design prevents sandwich attacks on engine state within a single EVM transaction.

## Performance Considerations

### Hot Path Optimization
- Orderbook uses `BTreeMap` with composite keys for O(log n) operations
- Matching is single-threaded but highly optimized
- State reads use zero-copy where possible

### Parallelization Strategy
- EVM tx execution can be parallelized (with conflict detection)
- Precompile reads are inherently parallel-safe (read-only)
- Indexer runs independently, processes blocks in parallel

### Memory Management
- Orderbook kept in-memory for performance
- Periodic snapshots to disk
- Positions/balances in-memory with write-ahead log

## Inspiration & Sources

This project draws inspiration from and references several open-source projects:

### Architecture Inspiration

**[Hyperliquid](https://hyperliquid.xyz/)**
- Primary architectural model (HyperCore + HyperEVM dual execution)
- API format designed for compatibility with Hyperliquid's public API
- CoreWriter MEV-prevention pattern follows their design philosophy
- Precompile address convention (0x0800-0x0805)
- This is a clean-room implementation inspired by their public documentation

### Core Dependencies

| Library | Purpose | Link |
|---------|---------|------|
| **revm** | Rust EVM implementation | [github.com/bluealloy/revm](https://github.com/bluealloy/revm) |
| **CometBFT** | BFT consensus via ABCI | [github.com/cometbft/cometbft](https://github.com/cometbft/cometbft) |
| **alloy** | Ethereum primitives/ABI | [github.com/alloy-rs/alloy](https://github.com/alloy-rs/alloy) |
| **Axum** | HTTP/WebSocket server | [github.com/tokio-rs/axum](https://github.com/tokio-rs/axum) |
| **sqlx** | Async PostgreSQL | [github.com/launchbadge/sqlx](https://github.com/launchbadge/sqlx) |

### SDK Dependencies

| Library | Purpose | Link |
|---------|---------|------|
| **viem** | TypeScript Ethereum library | [github.com/wevm/viem](https://github.com/wevm/viem) |
| **eth-account** | Python signing | [github.com/ethereum/eth-account](https://github.com/ethereum/eth-account) |

### Development Tools

| Tool | Purpose | Link |
|------|---------|------|
| **Foundry** | Solidity development | [github.com/foundry-rs/foundry](https://github.com/foundry-rs/foundry) |
| **Anvil** | Local EVM node | Part of Foundry |

### Design Pattern References

- **Order Matching**: Standard price-time priority (FIFO at price level), common in traditional exchanges
- **Margin Model**: Cross-margin perpetual futures model based on industry standards
- **Funding Rate**: 8-hour funding interval with premium index, following perpetual swap conventions
- **EIP-712**: Typed structured data signing per Ethereum standard

### Notable Implementations Used as Reference

When implementing specific features, these projects served as reference:
- **[reth](https://github.com/paradigmxyz/reth)** - EVM JSON-RPC patterns
- **[penumbra](https://github.com/penumbra-zone/penumbra)** - ABCI integration patterns
- **[dydx-v4](https://github.com/dydxprotocol/v4-chain)** - Perpetuals exchange patterns

## Current Implementation Status

See [IMPLEMENTATION_STATUS.md](./IMPLEMENTATION_STATUS.md) for detailed analysis of what's implemented vs stubbed.

### Quick Status (Phase 1 + 2A Complete)

| Component | Status | Key File |
|-----------|--------|----------|
| Matching Engine | ✅ Complete | `crates/engine/src/matching.rs` |
| Risk Engine | ✅ Complete | `crates/engine/src/risk.rs` |
| Funding Engine | ✅ Complete | `crates/engine/src/funding.rs` |
| Precompiles (Perps) | ✅ Complete (0x0800-0x0805) | `crates/evm/src/precompiles.rs` |
| Precompiles (Spot) | ✅ Complete (0x0806-0x0808) | `crates/evm/src/precompiles.rs` |
| Gateway (read) | ✅ Complete | `crates/gateway/src/handlers.rs` |
| Gateway (write/spot) | ✅ Complete | `crates/gateway/src/handlers.rs` |
| Gateway (write/perps) | ⚠️ Partial | `crates/gateway/src/handlers.rs` |
| EVM JSON-RPC | ✅ Complete | `crates/evm/src/rpc.rs` |
| EVM Execution | ✅ Complete (revm) | `crates/evm/src/executor.rs` |
| HIP-1 Spot Tokens | ✅ Complete | `crates/engine/src/spot_engine.rs` |
| **Unified State** | ✅ **Complete** | `crates/primitives/src/unified_state.rs` |
| **Shared Process** | ✅ **Complete** | `crates/node/src/main.rs` |
| Consensus | ❌ Stub (Phase 2B) | `crates/chain/src/abci.rs` |
| Persistence | ❌ Stub (Phase 4) | N/A |

---

## Detailed State Architecture

### Unified State Model (Phase 2A - IMPLEMENTED ✅)

HyperCore now implements Hyperliquid's unified state architecture:

1. **Single Master Balance Sheet** - ONE source of truth for all balances (`crates/primitives/src/unified_state.rs`)
2. **Two "Views"** - HyperCore view and HyperEVM view of the SAME data
3. **Shared Process** - Gateway and EVM RPC run in same process (`crates/node/src/main.rs:121-165`)
4. **View Transfers** - Atomic view adjustments, no bridging needed

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    HYPERCORE ARCHITECTURE (Phase 2A - IMPLEMENTED)               │
│                                                                                  │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│                           HyperBFT Consensus                                    │
│                         (Single consensus layer)                                │
│                                    │                                             │
│                    ┌───────────────┴───────────────┐                            │
│                    │                               │                             │
│              HyperCore                        HyperEVM                           │
│              (RustVM)                          (EVM)                             │
│            Trading API                      EVM JSON-RPC                         │
│                    │                               │                             │
│                    │      Read Precompiles        │                             │
│                    │◄──────────────────────────────                             │
│                    │                               │                             │
│                    │      CoreWriter Actions       │                             │
│                    │──────────────────────────────►│                             │
│                    │                               │                             │
│                    └───────────────┬───────────────┘                            │
│                                    │                                             │
│                    ┌───────────────┴───────────────┐                            │
│                    │    UNIFIED STATE LAYER        │                            │
│                    │   (Master Balance Sheet)      │                            │
│                    │                               │                            │
│                    │   User 0xf39F:                │                            │
│                    │   - Total USDC: 100,000       │                            │
│                    │   - HyperCore view: 80,000    │                            │
│                    │   - HyperEVM view: 20,000     │                            │
│                    │                               │                            │
│                    │   Token TEST (index 1):       │                            │
│                    │   - System Addr: 0x2000...01  │                            │
│                    │   - Total supply: 1,000,000   │                            │
│                    │                               │                            │
│                    │   ⚡ ONE BALANCE, TWO VIEWS   │                            │
│                    └───────────────────────────────┘                            │
│                                                                                  │
│   KEY: When you "move" 100 USDC from HyperCore to HyperEVM:                     │
│   - The master ledger adjusts views (Core: -100, EVM: +100)                     │
│   - NO actual transfer, NO bridge, NO wrapped tokens                            │
│   - Atomic update within same consensus round                                   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**Source:** [Inside Hyperliquid's Technical Architecture](https://www.blockhead.co/2025/06/05/inside-hyperliquids-technical-architecture/)

### System Address Mechanism

Each HIP-1 token has a **system address** that represents the "EVM view" of the token:

```
System Address = 0x20 + 00...00 + token_index (big-endian)

Examples:
- Token 0 (USDC):  0x2000000000000000000000000000000000000000
- Token 1 (TEST):  0x2000000000000000000000000000000000000001
- Token 1385:      0x2000000000000000000000000000000000000569
```

When a user "transfers" to HyperEVM, the master ledger:
1. Decreases their HyperCore view balance
2. Increases their HyperEVM view balance
3. The ERC20 contract at the system address reflects this

**There is NO actual bridge** - just accounting views of the same underlying balance.

---

### Our Current Implementation (Phase 2A) - NOW MATCHES HYPERLIQUID

**✅ COMPLETED**: Our implementation now uses a unified state model:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    CURRENT IMPLEMENTATION (Phase 2A - COMPLETE ✅)               │
│                    ✅ NOW MATCHES HYPERLIQUID ARCHITECTURE ✅                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│                    ┌──────────────────────────────────────┐                     │
│                    │       SHARED UNIFIED STATE           │                     │
│                    │    Arc<RwLock<UnifiedState>>         │                     │
│                    │                                       │                     │
│                    │   UnifiedBalance {                    │                     │
│                    │     total: 100,000 USDC,             │                     │
│                    │     core_view: 80,000, ◄── SpotEngine │                     │
│                    │     evm_view: 20,000   ◄── EvmState   │                     │
│                    │   }                                   │                     │
│                    └──────────────────────────────────────┘                     │
│                                     │                                            │
│              ┌──────────────────────┼──────────────────────┐                    │
│              │                      │                      │                    │
│              ▼                      ▼                      ▼                    │
│   ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐            │
│   │  SpotEngine     │    │   Gateway API   │    │   EvmExecutor   │            │
│   │  (trading)      │    │   (port 3000)   │    │   (port 8545)   │            │
│   │  reads core_view│    │                 │    │  reads evm_view │            │
│   └─────────────────┘    └─────────────────┘    └─────────────────┘            │
│                                                                                  │
│              ✅ SHARED STATE - ALL IN SAME PROCESS ✅                            │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Phase 2A Migration - COMPLETED ✅

All migration steps have been implemented:

| Migration Step | Status | Evidence |
|----------------|--------|----------|
| 1. Create UnifiedState with existing balances | ✅ | `crates/primitives/src/unified_state.rs` |
| 2. Point SpotEngine to unified_state.core_view | ✅ | `spot_engine.rs:187`: `get_core_view()` |
| 3. Point EvmState to unified_state.evm_view | ✅ | `state.rs:134`: `get_evm_view()` |
| 4. Add view transfer methods | ✅ | `spot_engine.rs:681-688` |
| 5. Update tests to verify invariants | ✅ | Unit tests verify `core_view` and `evm_view` |

### State Storage Locations (Phase 2A)

| State Type | Storage | Rust Type | Purpose |
|------------|---------|-----------|---------|
| **All Balances** | `UnifiedState.balances` | `HashMap<(AccountAddress, TokenIndex), UnifiedBalance>` | **Master balance sheet** |
| Spot Available | `unified_state.core_view` | via `get_core_view()` | Trading (orders, fills) |
| EVM Available | `unified_state.evm_view` | via `get_evm_view()` | Gas, native transfers |
| Reserved | `SpotEngineState.reserved` | `HashMap<(AccountAddress, TokenIndex), Decimal>` | Open orders |
| Perp Balances | `EngineState.accounts` | `HashMap<AccountAddress, Account>` | Margin, positions |
| EVM Storage | `EvmState.storage` | `HashMap<Address, ContractStorage>` | Smart contracts |

### Entry Points (Phase 2A - Shared State)

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 2A: SHARED UNIFIED STATE (IMPLEMENTED)                  │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   Gateway API (POST /exchange)                 EVM RPC (eth_sendRawTransaction) │
│        │                                                │                        │
│        │     ┌────────────────────────────────────┐     │                        │
│        └────►│   SharedUnifiedState               │◄────┘                        │
│              │   Arc<RwLock<UnifiedState>>        │                              │
│              │                                    │                              │
│              │   File: crates/node/src/main.rs   │                              │
│              │   Line 121: new_shared_unified_state()                           │
│              │   Line 129: SpotEngine uses Arc::clone()                         │
│              │   Line 149: EvmExecutor uses Arc::clone()                        │
│              └────────────────────────────────────┘                              │
│                          │           │                                           │
│                          ▼           ▼                                           │
│              SpotEngine.place_order()  EvmExecutor.execute()                    │
│              reads core_view           reads evm_view                            │
│                                                                                  │
│   ✅ BOTH share the SAME Arc<RwLock<UnifiedState>>                              │
│   ❌ No consensus ordering (Phase 2B)                                           │
│   ✅ Unified balance views working                                              │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## Current Vulnerabilities (Phase 2A)

### V0: Architectural Mismatch - ✅ RESOLVED

**Previous Problem**: Two separate state systems (SpotEngine and EVM) that didn't share balances.

**Solution Implemented (Phase 2A)**:
- Created `UnifiedState` with `UnifiedBalance { total, core_view, evm_view }`
- Both SpotEngine and EvmExecutor share the same `Arc<RwLock<UnifiedState>>`
- Gateway and EVM RPC run in the same process
- View transfers implemented for moving funds between layers

**Verification**: `crates/node/src/main.rs:121-149` shows shared state setup.

---

## Remaining Vulnerabilities (Phase 2B+)

### V1: No Consensus (CRITICAL)

**Problem**: State mutations happen directly without consensus ordering.

```rust
// Current: Direct state mutation
let mut engine = spot_engine.write().await;
engine.place_order(...);  // No consensus!
```

**Impact**:
- Multiple nodes would diverge in state
- No Byzantine fault tolerance
- Race conditions between concurrent requests
- Cannot run multi-node network

**Solution (Phase 2)**: Route all mutations through CometBFT:
```
Gateway → CometBFT Mempool → Consensus → ABCI finalize_block → Engine
```

### V2: No State Persistence (CRITICAL)

**Problem**: All state is in-memory; lost on restart.

```rust
// Current: In-memory only
pub struct SpotEngineState {
    balances: HashMap<...>,  // Lost on restart!
    tokens: HashMap<...>,
    markets: HashMap<...>,
}
```

**Impact**:
- All balances lost on node restart
- Cannot recover from crashes
- No way to sync new nodes

**Solution (Phase 4)**: Add RocksDB persistence:
```
State Change → Write-Ahead Log → Commit to RocksDB → Snapshot periodically
```

### V3: Stub Signature Verification (SECURITY)

**Problem**: Signatures are not cryptographically verified.

```rust
// Current: Address extracted from signature r-value (STUB!)
let address_hex = &r[r.len() - 40..];  // Anyone can spoof any address!
```

**Impact**:
- Any user can impersonate any other user
- No authentication whatsoever
- Completely insecure for production

**Solution (Phase 3)**: Implement EIP-712 verification:
```rust
// Target: Real signature verification
let recovered = ecrecover(typed_data_hash, v, r, s)?;
if recovered != claimed_address {
    return Err(InvalidSignature);
}
```

### V4: No Bridge Between Layers (DESIGN GAP)

**Problem**: No mechanism to transfer value between HyperCore and HyperEVM.

**Impact**:
- Users cannot use trading profits in EVM contracts
- ERC20 tokens cannot be traded on HyperCore orderbook
- Two isolated ecosystems

**Solution (Phase 3)**: Implement CoreWriter precompile + SpotToken bridge:
```
HyperCore USDC ←──── CoreWriter (0x0820) ────→ HyperEVM Wrapped USDC
       ↑                                               ↓
       └────────── SpotToken.sol bridge ───────────────┘
```

### V5: Race Conditions on Concurrent Access

**Problem**: Multiple async tasks can read/write state concurrently.

```rust
// Current: RwLock provides basic safety but no ordering
let engine = spot_engine.write().await;  // What if two requests arrive simultaneously?
```

**Impact**:
- Non-deterministic execution order
- State could depend on timing of requests
- Different nodes could see different results

**Solution (Phase 2)**: CometBFT provides deterministic ordering:
```
All transactions ordered by consensus → Execute in order → Deterministic state
```

---

## Target Architecture (Post-Phase 2)

### Transaction Flow with Consensus

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        TARGET ARCHITECTURE (WITH COMETBFT)                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   ┌─────────────┐                                                               │
│   │   Gateway   │◄──── User submits signed action                               │
│   └──────┬──────┘                                                               │
│          │                                                                       │
│          │ Submit to mempool (no direct state mutation!)                        │
│          ▼                                                                       │
│   ┌──────────────────────────────────────────────────────────┐                  │
│   │                        CometBFT                           │                  │
│   │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │                  │
│   │  │   check_tx   │  │  Consensus   │  │finalize_block│    │                  │
│   │  │  (validate)  │→ │  (BFT vote)  │→ │  (execute)   │    │                  │
│   │  └──────────────┘  └──────────────┘  └──────┬───────┘    │                  │
│   └─────────────────────────────────────────────┼────────────┘                  │
│                                                 │                                │
│                                           ABCI Deliver                           │
│                                                 │                                │
│                                                 ▼                                │
│   ┌──────────────────────────────────────────────────────────┐                  │
│   │                   HyperCore App (ABCI)                    │                  │
│   │                                                           │                  │
│   │  Transaction Type?                                        │                  │
│   │        │                                                  │                  │
│   │        ├─── Order/Cancel ───► SpotEngine/EngineState     │                  │
│   │        │                                                  │                  │
│   │        └─── EVM Tx ─────────► EvmExecutor                │                  │
│   │                                    │                      │                  │
│   │                                    ▼                      │                  │
│   │                             CoreWriter events?            │                  │
│   │                                    │                      │                  │
│   │                                    ▼                      │                  │
│   │                          Queue for next block             │                  │
│   │                                                           │                  │
│   └──────────────────────────────────────────────────────────┘                  │
│                                                                                  │
│   KEY PROPERTIES:                                                               │
│   ✓ All state mutations go through consensus                                    │
│   ✓ Deterministic execution order across all nodes                              │
│   ✓ Byzantine fault tolerance (up to 1/3 malicious nodes)                       │
│   ✓ Single source of truth                                                      │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### ABCI Integration Points

| ABCI Method | Purpose | Current Status |
|-------------|---------|----------------|
| `check_tx` | Validate tx for mempool (balance, signature, nonce) | ✅ Implemented |
| `prepare_proposal` | Leader selects txs for block | ✅ Implemented |
| `process_proposal` | Validators verify proposal | ✅ Implemented |
| `finalize_block` | Execute txs, return results | ✅ Implemented |
| `commit` | Persist state, return app_hash | ✅ Implemented |
| **Connection to CometBFT** | Actually receive transactions | ❌ **STUBBED** |

The ABCI methods are implemented but the server just sleeps:
```rust
// crates/node/src/main.rs:193-200
// In production, integrate with tendermint-abci crate
// For now, just keep the task alive
tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
```

### Bridging Architecture (Future)

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           LAYER BRIDGING (FUTURE)                                │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   HyperCore Layer                              HyperEVM Layer                    │
│   ──────────────                               ──────────────                    │
│                                                                                  │
│   ┌─────────────────┐                         ┌─────────────────┐               │
│   │  Spot Balances  │                         │ Wrapped Tokens  │               │
│   │  (USDC, TEST)   │                         │ (ERC20 wUSDC)   │               │
│   └────────┬────────┘                         └────────┬────────┘               │
│            │                                           │                         │
│            │    ┌─────────────────────────────────┐   │                         │
│            │    │      BRIDGE MECHANISMS           │   │                         │
│            │    ├─────────────────────────────────┤   │                         │
│            │    │                                  │   │                         │
│            ├────┤  1. CoreWriter Precompile       │───┤                         │
│            │    │     (EVM → HyperCore)           │   │                         │
│            │    │     Address: 0x0820             │   │                         │
│            │    │     Actions: placeOrder,        │   │                         │
│            │    │              transferToCore     │   │                         │
│            │    │                                  │   │                         │
│            ├────┤  2. SpotToken.sol Bridge        │───┤                         │
│            │    │     (HyperCore → EVM)           │   │                         │
│            │    │     Functions: bridgeToEvm(),   │   │                         │
│            │    │                bridgeFromEvm()  │   │                         │
│            │    │                                  │   │                         │
│            │    │  3. System Address              │   │                         │
│            │    │     Each HIP-1 token has a      │   │                         │
│            │    │     derived system address for  │   │                         │
│            │    │     holding bridged amounts     │   │                         │
│            │    │                                  │   │                         │
│            │    └─────────────────────────────────┘   │                         │
│            │                                           │                         │
│            ▼                                           ▼                         │
│   ┌─────────────────────────────────────────────────────────────────┐           │
│   │                        CometBFT Consensus                        │           │
│   │         (All bridge transfers are consensus transactions)        │           │
│   └─────────────────────────────────────────────────────────────────┘           │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Bridge Transfer Flow

**HyperCore → HyperEVM (Withdraw to EVM)**:
```
1. User calls SpotToken.bridgeToEvm(amount) on HyperCore API
2. Transaction goes through CometBFT consensus
3. ABCI handler:
   a. Debit user's HyperCore balance
   b. Credit system address (locks tokens)
   c. Emit BridgeToEvm event
4. Next block: EVM mints wrapped tokens to user
```

**HyperEVM → HyperCore (Deposit to Trading)**:
```
1. User calls wrappedToken.bridgeToCore(amount) on EVM
2. EVM tx executes, burns wrapped tokens
3. CoreWriter event emitted: BridgeToCore(user, token, amount)
4. End of block: Queue bridge action
5. Next block: Credit user's HyperCore balance
```

---

## Phase Roadmap Summary

⚠️ **RESTRUCTURED**: Based on the discovery that Hyperliquid uses a unified state model.

| Phase | Focus | Key Deliverables | Solves |
|-------|-------|------------------|--------|
| **Phase 1** ✅ | EVM Integration | EVM RPC, Precompiles, HIP-1 Tokens | Basic functionality |
| **Phase 2A** ✅ | **Unified State** | **Master balance sheet, view separation** | **V0 (architecture)** |
| **Phase 2B** | Consensus | CometBFT connection, ABCI wiring | V1, V5 (consensus, ordering) |
| **Phase 3** | Gateway Security | EIP-712 signatures, view transfer API | V3 (auth) |
| **Phase 4** | Persistence | RocksDB, WAL, Snapshots | V2 (durability) |
| **Phase 5** | Production | Rate limiting, monitoring, hardening | Security |

---

## Phase 2A: Unified State Refactor (Critical Path)

### Why This Is The Next Step

The unified state model is **foundational** and must come before CometBFT integration:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        DEPENDENCY CHAIN                                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   Phase 1 (Done)          Phase 2A (Next)           Phase 2B (After)            │
│   ┌─────────────┐        ┌──────────────┐         ┌──────────────┐              │
│   │ Separate    │   →    │ Unified      │    →    │ CometBFT     │              │
│   │ States      │        │ State Model  │         │ Integration  │              │
│   │ (Testing)   │        │ (Foundation) │         │ (Consensus)  │              │
│   └─────────────┘        └──────────────┘         └──────────────┘              │
│                                                                                  │
│   Why 2A before 2B:                                                              │
│   ┌─────────────────────────────────────────────────────────────────────────┐   │
│   │ • CometBFT commits a SINGLE state root                                  │   │
│   │ • That state root must cover unified balance views                      │   │
│   │ • Connecting consensus to wrong architecture = wasted work              │   │
│   │ • View transfers must be consensus transactions                         │   │
│   └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Phase 2A Implementation Steps

#### Step 1: Create UnifiedState Primitive

Create `crates/primitives/src/unified_state.rs`:

```rust
use crate::{AccountAddress, Decimal, TokenIndex};
use std::collections::HashMap;

/// Balance with separate views for Core and EVM layers
#[derive(Debug, Clone, Default)]
pub struct UnifiedBalance {
    /// Total balance (source of truth, immutable except for deposits/withdrawals)
    pub total: Decimal,
    /// Portion available for HyperCore trading
    pub core_view: Decimal,
    /// Portion available for HyperEVM contracts
    pub evm_view: Decimal,
}

impl UnifiedBalance {
    /// Invariant check: total == core_view + evm_view
    pub fn is_valid(&self) -> bool {
        self.total == self.core_view + self.evm_view
    }
}

/// Master balance sheet with views
#[derive(Debug, Clone, Default)]
pub struct UnifiedState {
    balances: HashMap<(AccountAddress, TokenIndex), UnifiedBalance>,
}

impl UnifiedState {
    /// Transfer from Core view to EVM view (not a bridge - just view adjustment)
    pub fn transfer_to_evm_view(
        &mut self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), &'static str> {
        let balance = self.balances.entry((user, token)).or_default();
        if balance.core_view < amount {
            return Err("Insufficient Core view balance");
        }
        balance.core_view -= amount;
        balance.evm_view += amount;
        // Note: balance.total unchanged!
        Ok(())
    }

    /// Transfer from EVM view to Core view
    pub fn transfer_to_core_view(
        &mut self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), &'static str> {
        let balance = self.balances.entry((user, token)).or_default();
        if balance.evm_view < amount {
            return Err("Insufficient EVM view balance");
        }
        balance.evm_view -= amount;
        balance.core_view += amount;
        // Note: balance.total unchanged!
        Ok(())
    }
}
```

#### Step 2: Refactor SpotEngineState

Modify `crates/engine/src/spot_engine.rs` to reference UnifiedState:

```rust
// Before: Separate balance HashMap
pub struct SpotEngineState {
    balances: HashMap<(AccountAddress, TokenIndex), SpotBalance>,
    // ...
}

// After: Reference to unified state
pub struct SpotEngineState {
    unified_state: Arc<RwLock<UnifiedState>>,
    reserved: HashMap<(AccountAddress, TokenIndex), Decimal>, // For open orders
    // ...
}

impl SpotEngineState {
    pub fn get_available_for_trading(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        let state = self.unified_state.read();
        let balance = state.get_balance(user, token);
        let reserved = self.reserved.get(&(user, token)).unwrap_or(&Decimal::ZERO);
        balance.core_view - reserved
    }
}
```

#### Step 3: Refactor EvmState

Modify `crates/evm/src/state.rs` to reference UnifiedState:

```rust
// Before: Separate account balances
pub struct EvmState {
    accounts: HashMap<Address, EvmAccount>,  // Contains balance
    // ...
}

// After: Balance comes from unified state
pub struct EvmState {
    unified_state: Arc<RwLock<UnifiedState>>,
    accounts: HashMap<Address, EvmAccountMeta>,  // Nonce, code_hash only
    storage: HashMap<Address, ContractStorage>,
    // ...
}

impl EvmState {
    pub fn get_balance(&self, address: &Address) -> U256 {
        // Convert address to AccountAddress
        // Look up evm_view in unified state
        let state = self.unified_state.read();
        let balance = state.get_balance(account_addr, TOKEN_ETH);
        balance.evm_view.to_u256()
    }
}
```

### Phase 1 Test Compatibility

**Phase 1 E2E tests remain valid** because:

1. **Spot Trading Tests**: Test core_view operations only
   - `SpotBalances` → reads from core_view
   - `SpotOrder` → reserves from core_view
   - No cross-layer operations tested

2. **EVM Tests**: Test evm_view operations only
   - `eth_getBalance` → reads from evm_view
   - `eth_sendRawTransaction` → debits evm_view for gas
   - No cross-layer operations tested

3. **Tests Implemented (Phase 2A)** ✅:
   - View transfer from Core to EVM - `spot_engine.rs:test_view_transfer()`
   - View transfer from EVM to Core - `spot_engine.rs:test_view_transfer()`
   - Total balance invariant verification - `unified_state.rs::tests`
   - Available balance considers reserved - `spot_engine.rs:test_available_balance_with_reserved()`

### Migration Strategy - COMPLETED ✅

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        MIGRATION PATH - ✅ COMPLETE                              │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   Old State (Phase 1)                    New State (Phase 2A - IMPLEMENTED)     │
│   ┌─────────────────────────────┐       ┌─────────────────────────────┐         │
│   │ SpotEngineState             │       │ UnifiedState                │         │
│   │   balances: HashMap         │  →    │   balances: HashMap         │         │
│   └─────────────────────────────┘       │     (user, token) →         │         │
│              REMOVED                     │       { total, core, evm } │         │
│   ┌─────────────────────────────┐       └─────────────────────────────┘         │
│   │ EvmState                    │                    ↑                          │
│   │   accounts: HashMap         │                    │                          │
│   │     (balance field)         │  ──────────────────┘                          │
│   └─────────────────────────────┘       Reference, not copy                     │
│              REFACTORED                                                          │
│                                                                                  │
│   Migration steps:                                                               │
│   ✅ 1. Create UnifiedState with existing balances (unified_state.rs)           │
│   ✅ 2. Point SpotEngine to unified_state.core_view (spot_engine.rs:187)        │
│   ✅ 3. Point EvmState to unified_state.evm_view (state.rs:134)                 │
│   ✅ 4. Add view transfer methods (spot_engine.rs:681-688)                      │
│   ✅ 5. Update tests to verify invariants (unit tests pass)                     │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```
