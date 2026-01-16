# HyperCore Development TODO

Prioritized list of outstanding work items organized by criticality and phase.

## Legend
- 🔴 **P0 - Critical** - Blocks core functionality
- 🟠 **P1 - High** - Required for MVP
- 🟡 **P2 - Medium** - Important for production
- 🟢 **P3 - Low** - Nice to have

---

## Technical Debt Tracker

Items that work but should be refactored for maintainability, performance, or correctness.

### 🟡 TD-001: Duplicate EIP-712 Encoding Code

**Priority:** Medium | **Impact:** Maintenance burden

**Problem:** EIP-712 encoding functions are duplicated in two locations:
- `crates/gateway/src/eip712.rs` - Gateway's encoding (takes string addresses)
- `crates/chain/src/tx.rs` - Chain's encoding (takes AccountAddress type)

Both implementations produce identical hashes for chain ID 1337, but maintaining two copies is error-prone.

**Solution:** Move shared EIP-712 utilities to `crates/primitives/src/eip712.rs`:
```rust
// crates/primitives/src/eip712.rs
pub fn encode_string(s: &str) -> [u8; 32];
pub fn encode_uint8(v: u8) -> [u8; 32];
pub fn encode_uint64(v: u64) -> [u8; 32];
pub fn encode_bool(v: bool) -> [u8; 32];
pub fn encode_address(addr: &AccountAddress) -> [u8; 32];
pub fn encode_array(hashes: &[[u8; 32]]) -> [u8; 32];
pub fn compute_domain_separator(chain_id: u64) -> [u8; 32];
pub const DOMAIN_TYPE_HASH: [u8; 32];
```

**Files affected:**
- `crates/gateway/src/eip712.rs` - Refactor to use shared module
- `crates/chain/src/tx.rs` - Refactor to use shared module
- `crates/primitives/src/lib.rs` - Add eip712 module

---

### 🟡 TD-002: Hardcoded Chain ID in tx.rs

**Priority:** Medium | **Impact:** Multi-chain support blocked

**Problem:** `crates/chain/src/tx.rs:compute_domain_separator()` has chain ID 1337 hardcoded:
```rust
let chain_id: [u8; 32] = {
    let mut buf = [0u8; 32];
    buf[31] = 0x39; // 1337 hardcoded
    buf[30] = 0x05;
    buf
};
```

**Solution:** Make chain ID configurable:
```rust
fn compute_domain_separator(chain_id: u64) -> [u8; 32] {
    // ... use encode_uint64(chain_id) instead of hardcoded bytes
}
```

**Files affected:**
- `crates/chain/src/tx.rs` - Parameterize chain_id
- `crates/chain/src/app.rs` - Pass chain_id from config

---

### 🟢 TD-003: Unused Persistence Code

**Priority:** Low | **Impact:** Code cleanliness

**Problem:** `crates/persistence/src/rocksdb_backend.rs` has unused fields and methods:
- `cf_handles` field never used
- `cf_name()` method never called

**Solution:** Either remove unused code or implement the intended functionality.

---

### 🟢 TD-004: Decimal byte conversion returns Vec instead of [u8; 32]

**Priority:** Low | **Impact:** API consistency

**Problem:** `Decimal::to_be_bytes()` and `to_be_bytes_signed()` return `Vec<u8>` instead of `[u8; 32]`.

**Current:** Returns 16 bytes (i128) as Vec
**Expected:** Some callers might expect 32-byte array for EVM compatibility

**Solution:** Add `to_be_bytes_32()` method that pads to 32 bytes, keep existing methods for backward compatibility.

---

### 🟡 TD-005: Unused function parameters in liquidation.rs

**Priority:** Medium | **Impact:** Code clarity

**Problem:** `LiquidationEngine::process_liquidation()` has unused parameters:
- `account: AccountAddress` - never used in function body
- `timestamp: Timestamp` - never used in function body

**Solution:** Either:
1. Use these parameters (e.g., for logging, event emission)
2. Remove them from the function signature
3. Prefix with `_` to indicate intentionally unused

**Files affected:** `crates/engine/src/liquidation.rs:27-30`

---

### 🟡 TD-006: Missing liquidation integration with risk engine

**Priority:** Medium | **Impact:** Full liquidation flow

**Problem:** The liquidation engine calculates liquidation parameters but doesn't integrate with the risk engine's `is_liquidatable()` check in a unified flow.

**Current state:**
- `RiskEngine::is_liquidatable()` - Checks if position should be liquidated
- `LiquidationEngine::process_liquidation()` - Calculates liquidation params
- These are separate and not integrated

**Solution:** Create a unified `check_and_process_liquidation()` flow that:
1. Checks `is_liquidatable()` from risk engine
2. If true, calls `process_liquidation()` from liquidation engine
3. Handles the resulting position updates

---

### 🟢 TD-007: Test coverage gaps in concurrent order scenarios

**Priority:** Low | **Impact:** Edge case coverage

**Problem:** Tests don't cover concurrent order modification scenarios:
- Two users canceling the same order simultaneously
- Order fill racing with cancel
- Multiple partial fills on same order

**Solution:** Add stress tests with concurrent operations using tokio tasks.

---

## Phase 1: EVM Integration ✅ COMPLETED (100%)

### Phase 1a: EVM JSON-RPC Server ✅ COMPLETED

**File created:** `crates/evm/src/rpc.rs`

```
[x] Create JSON-RPC server using jsonrpsee
[x] Implement all eth_* methods (chainId, blockNumber, gasPrice, etc.)
[x] Implement web3_clientVersion, net_version
[x] Upgrade revm to 19.0 with API compatibility fixes
[x] Update bytecode to solc 0.8.29 (PUSH0 opcode support)
[x] All 20 EVM unit tests passing
```

**File modified:** `crates/evm/src/executor.rs`

```
[x] Replace stub execute_tx() with real revm execution
[x] Create HyperEvmDb that wraps EvmState with Database trait
[x] Handle precompile calls directly before EVM execution
[x] Implement proper gas accounting via revm
[x] Handle transaction receipts and logs
[x] Implement state commitment via DatabaseCommit
```

**File modified:** `crates/node/src/main.rs`

```
[x] Create EvmRpcServer instance
[x] Start RPC server on port 8545 (configurable)
[x] Add --evm-rpc-addr CLI flag
[x] Wire up executor to RPC handlers
```

### Phase 1b: Token Standard Support ✅ COMPLETED

**File modified:** `scripts/e2e/runner.ts`

```
[x] Add ERC20 token deployment tests
[x] Add ERC20 metadata and transfer tests
[x] Add ERC721 NFT deployment tests
[x] Add ERC721 mint tests
[x] Add ERC1155 multi-token deployment tests
[x] Add ERC1155 mint tests
[x] All 8 token tests pass - verified with Docker rebuild!
```

### Phase 1c: HIP-1 Style Token Integration ✅ COMPLETED

**References:**
- [HIP-1: Native token standard](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperliquid-improvement-proposals-hips/hip-1-native-token-standard)
- [HyperEVM Architecture](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperevm)

**All Items Completed:**
```
[x] Add spot token primitives (crates/primitives/src/spot.rs)
    - SpotToken, SpotBalance, SpotMarketConfig
    - TokenIndex, system address derivation
[x] Implement SpotEngine (crates/engine/src/spot_engine.rs)
    - Token deployment with genesis allocations
    - Spot orderbook with price-time priority
    - Balance-based trading (no margin/leverage)
    - All 3 spot engine unit tests passing
[x] Create SpotToken.sol contract (contracts/src/SpotToken.sol)
    - HIP-1 style ERC20 with bridge support
    - SpotTokenFactory for deployment
[x] Add spot API endpoints to gateway
    - SpotMeta, SpotL2Book, SpotAllMids, SpotBalances, SpotOpenOrders
    - SpotOrder, SpotCancel, SpotCancelAll exchange actions
[x] Add spot precompiles (0x0806-0x0808)
    - SpotBalance: Get token balance for address
    - SpotMarket: Get spot market info
    - SpotOrderBook: Get L2 orderbook snapshot
[x] Wire up SpotEngine to node main.rs
    - GatewayServer.with_spot_engine() constructor
    - Default TEST-USDC spot market initialized
    - Test accounts credited with USDC and TEST balances
[x] Comprehensive E2E tests for spot trading (12 tests)
    - SpotMeta, SpotL2Book, SpotAllMids queries
    - SpotBalances with balance verification
    - SpotOpenOrders with/without market filter
    - SpotTokenInfo queries
    - Spot order placement and verification
    - Order cancellation and verification
```

---

## Phase 2: Unified State & Consensus Integration

### Phase 2A: Unified State Refactor ✅ COMPLETED (100%)

**Reference:** [Inside Hyperliquid's Technical Architecture](https://www.blockhead.co/2025/06/05/inside-hyperliquids-technical-architecture/)

#### ✅ P0: Create UnifiedState Layer - COMPLETED

**File created:** `crates/primitives/src/unified_state.rs`

```
[x] Create UnifiedBalance struct with views:
    - total: Decimal (source of truth)
    - core_view: Decimal (available for trading)
    - evm_view: Decimal (available for EVM)
    - Invariant: total == core_view + evm_view
    - is_valid() method to verify invariant

[x] Create UnifiedState struct:
    - balances: HashMap<(AccountAddress, TokenIndex), UnifiedBalance>
    - Single state, two views

[x] Create SharedUnifiedState type: Arc<RwLock<UnifiedState>>
```

#### ✅ P0: Refactor SpotEngineState to use UnifiedState - COMPLETED

**File modified:** `crates/engine/src/spot_engine.rs`

```
[x] Replace SpotEngineState.balances with SharedUnifiedState reference (line 50)
[x] Implement get_core_view() for reading core balance (line 187)
[x] Implement reserve_balance() for order reservation
[x] Implement release_balance() for releasing reservations
[x] Update place_order() to use unified balance + reserved tracking
[x] All 8 spot engine unit tests passing
```

#### ✅ P0: Refactor EvmState to use UnifiedState - COMPLETED

**File modified:** `crates/evm/src/state.rs`

```
[x] Replace EvmState balance storage with SharedUnifiedState reference (line 53)
[x] Implement get_evm_view() for reading EVM balance (line 134)
[x] Implement debit_evm() for gas/transfers
[x] Implement credit_evm() for receives
[x] EVM nonce/storage remains in EvmState (not unified)
[x] All EVM unit tests passing
```

#### ✅ P0: Implement View Transfer (Not Bridging!) - COMPLETED

**File modified:** `crates/primitives/src/unified_state.rs`

```
[x] Implement UnifiedState.transfer_to_evm_view() (line 203):
    - Verify core_view >= amount
    - core_view -= amount
    - evm_view += amount
    - total unchanged!

[x] Implement UnifiedState.transfer_to_core_view() (line 234):
    - Verify evm_view >= amount
    - evm_view -= amount
    - core_view += amount
    - total unchanged!

[x] Both methods verify invariant with debug_assert!
```

#### ✅ P0: Shared Process Architecture - COMPLETED

**File modified:** `crates/node/src/main.rs`

```
[x] Create single SharedUnifiedState instance (line 121)
[x] SpotEngine uses Arc::clone(&unified_state) (line 129)
[x] EvmExecutor uses Arc::clone(&unified_state) (line 149)
[x] Gateway and EVM RPC run in same process, share state
[x] Docker compose updated to expose port 3000 from node
```

#### ✅ P1: Update E2E Tests for Unified State - COMPLETED

**File modified:** `scripts/e2e/runner.ts`

```
[x] Test: EVM balance reflects unified state evm_view
[x] Test: Reserved balance prevents over-transfer
[x] Test: View transfers work correctly
[x] 104+ E2E tests passing
```

---

### Phase 2B: Consensus Integration ✅ 100% COMPLETED

**Prerequisites (all from Phase 2A - DONE):**
- ✅ Single `UnifiedState` to commit
- ✅ Gateway and EVM RPC share same process
- ✅ View transfer methods ready

#### ✅ P0: ABCI Application - COMPLETED

**Files modified:**
- `crates/chain/src/app.rs` - HyperCoreApp with transaction execution
- `crates/chain/src/block_producer.rs` - Single-node block production
- `crates/gateway/src/handlers.rs` - Gateway → ABCI integration

```
[x] HyperCoreApp with full transaction execution
[x] BlockProducer for single-node consensus
[x] Gateway routes perp transactions through ABCI
[x] execute_perp_action_via_app() function
```

#### ✅ P1: Transaction Processing - COMPLETED

**File modified:** `crates/chain/src/app.rs`

```
[x] Implement Order action processing
[x] Implement Cancel action processing
[x] Implement CancelByCloid processing (with CLOID index)
[x] Implement CancelAll processing
[x] Implement UsdTransfer processing
[x] Implement Withdraw processing
[x] Implement UpdateLeverage processing
[x] Event generation for all transaction types
```

#### ✅ P1: State Commitment - COMPLETED

**File modified:** `crates/chain/src/state.rs`

```
[x] Implement compute_app_hash() - Merkle-style commitment
[x] Implement compute_unified_state_root() - Balances hash
[x] Implement compute_nonce_root() - Nonce state hash
[x] BlockMeta struct for block history
[x] CLOID to Order ID mapping
```

#### ✅ P1: Query Endpoints - COMPLETED

**File modified:** `crates/gateway/src/handlers.rs`

```
[x] OpenOrders - Returns real data from EngineState
[x] UserFills - Returns fill history
[x] UserFundingHistory - Returns funding payment history
[x] FundingHistory - Returns market funding rates
[x] RecentTrades - Returns trade feed
```

#### ✅ P1: Fill & Funding History - COMPLETED

**File modified:** `crates/engine/src/state.rs`

```
[x] BlockMetadata struct for block hash, timestamp, events
[x] record_fill() - Records fills for maker, taker, market
[x] record_funding_payment() - Records funding payments
[x] record_market_funding() - Records market funding rates
[x] get_user_fills(), get_recent_trades(), get_user_funding_history()
[x] get_all_user_orders() - All open orders across markets
```

#### ✅ P1: Funding Rate Application - COMPLETED

**File modified:** `crates/node/src/main.rs`

```
[x] funding_processor() - Calculates and applies funding
[x] Uses FundingEngine for rate calculations
[x] Records funding payments to history
[x] Updates position funding indices
```

#### ✅ P0: CometBFT Integration - COMPLETED

**Files created:**
- `crates/chain/src/cometbft/mod.rs` - Module definition
- `crates/chain/src/cometbft/app.rs` - CometBftApp implementing Application trait
- `crates/chain/src/cometbft/server.rs` - ABCI TCP server
- `crates/chain/src/cometbft/validators.rs` - Validator set management

**File modified:** `crates/node/src/main.rs`

```
[x] Add tendermint-abci crate (via cometbft feature)
[x] CometBftApp - Implements tendermint_abci::Application trait
[x] CometBftServer - TCP-based ABCI server
[x] ValidatorSet - Validator set tracking and supermajority calculation
[x] ConsensusMode enum (single-node | cometbft)
[x] --consensus-mode CLI flag
[x] --block-time-ms CLI flag
[x] Multi-node consensus support via CometBFT mode
```

**Usage:**
```bash
# Single-node mode (default, for development)
hypercore start --consensus-mode single-node

# Multi-node mode with CometBFT
cargo build --features cometbft
hypercore start --consensus-mode cometbft --abci-addr 0.0.0.0:26658
```

---

## Phase 3: Production Infrastructure ✅ MOSTLY COMPLETE

This phase focuses on production-readiness: proper genesis initialization, gas fee implementation, and state commitment hardening.

**Status:** Phase 3A, 3B, and 3D are complete. Only Phase 3C (State Hardening) remains.

### Phase 3A: Genesis State Initialization ✅ COMPLETED

**Goal:** Start the blockchain with initial state instead of using runtime `creditBalance()` calls.

**Reference:** [CometBFT Genesis Spec](https://docs.cometbft.com/v0.38/spec/core/genesis)

**Files modified:**

```
[x] crates/chain/src/app.rs - GenesisState struct
    - Extended to support multi-token balances with view allocation
    - Added GenesisBalance { address, token, amount, view }
    - Added BalanceView enum (Core, Evm)
    - Fixed Decimal parsing to use from_str_exact() with correct token decimals

[x] crates/node/src/main.rs - create_genesis() function
    - Populated genesis with initial balances for Alice, Bob, Charlie
    - Includes both USDC (token 0) and TEST (token 1) balances
    - Added initialize_genesis_balances() for single-node mode

[x] crates/chain/src/app.rs - init_from_genesis()
    - Completes market initialization
    - Initializes spot tokens from genesis
    - Credits balances to correct views (core vs evm)

[x] Removed runtime creditBalance() calls
    - Initial balances now come from genesis state
    - Works for both single-node and multi-node deployments
```

**GenesisState Structure (Implemented):**
```rust
pub struct GenesisState {
    pub chain_id: String,
    pub markets: Vec<GenesisMarket>,
    pub spot_tokens: Vec<GenesisSpotToken>,
    pub balances: Vec<GenesisBalance>,  // Multi-token, multi-view
}

pub struct GenesisBalance {
    pub address: AccountAddress,
    pub token: TokenIndex,
    pub amount: String,  // Decimal as string
    pub view: BalanceView,  // Core or EVM
}

#[derive(Default)]
pub enum BalanceView {
    #[default]
    Core,  // Available for trading
    Evm,   // Available for smart contracts
}
```

---

### Phase 3B: Gas Fee Implementation ✅ COMPLETED

**Goal:** Implement gas fees for HyperEVM while keeping HyperCore trading zero-gas (like Hyperliquid).

**Reference:** [Hyperliquid Fees](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees)

**Hyperliquid's Model:**
| Layer | Gas | Why |
|-------|-----|-----|
| HyperCore (trading) | **Zero** | Dedicated chain, spam prevented by nonces+signatures |
| HyperEVM (contracts) | **Native token** | General-purpose smart contracts need metering |

**Files modified:**

```
[x] crates/evm/src/executor.rs - Gas fee infrastructure
    - Added FEE_COLLECTOR_ADDRESS constant (0x...FEE1)
    - Added enforce_gas_fees flag (disabled by default for backward compatibility)
    - Added set_enforce_gas_fees() and is_gas_fees_enforced() methods
    - Added InsufficientGasBalance error variant
    - Added apply_gas_fee() helper method

[x] crates/evm/src/executor.rs - execute_tx_inner()
    - Pre-flight check: sender must have evm_view >= gas_limit * gas_price + value
    - Post-execution: gas_used * gas_price deducted from sender's evm_view
    - Gas fees credited to fee collector
    - Works for both successful and reverted transactions
    - Only auto-creates test accounts when gas fees NOT enforced (dev mode)

[x] HyperCore actions remain zero-gas
    - Order, Cancel, Transfer, ViewTransfer - all free
    - Only EVM transactions have gas fees (when enabled)
```

**Implementation (Completed):**
```rust
// 1. Pre-flight check (execute_tx_inner)
if self.enforce_gas_fees && commit_state {
    let max_cost = gas_limit * gas_price;
    let total_required = max_cost + value;
    let sender_balance = get_evm_view_balance(sender);
    if sender_balance < total_required {
        return Err(InsufficientGasBalance { required, available });
    }
}

// 2. Execute transaction (revm)
let result = evm.transact_preverified()?;

// 3. Apply gas fee
fn apply_gas_fee(&mut self, sender: Address, gas_used: u64, gas_price: u64) {
    let gas_fee = gas_used * gas_price;
    self.db.sub_balance(sender, gas_fee);    // Debit from sender
    self.db.add_balance(FEE_COLLECTOR, gas_fee);  // Credit to fee collector
}
```

**Usage:**
```rust
// Enable gas fees for production
executor.set_enforce_gas_fees(true);

// Check if enabled
if executor.is_gas_fees_enforced() { ... }
```

---

### Phase 3C: State Commitment Hardening 🟡 P2 - MEDIUM

**Goal:** Strengthen state commitment for production BFT consensus with proper Merkle proofs.

**Current Implementation (crates/chain/src/state.rs):**
```rust
// Simple hash-based commitment (adequate for single-node, needs upgrade for production)
pub fn compute_app_hash(&self) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(&self.height.to_le_bytes());
    hasher.update(&self.timestamp.to_le_bytes());
    hasher.update(&self.app_hash);  // Previous hash (chain link)
    hasher.update(&self.compute_unified_state_root());  // Sorted balance hash
    hasher.update(&self.compute_nonce_root());          // Sorted nonce hash
    hasher.finalize().into()
}
```

**What's Missing for Production:**
- ❌ Proper Merkle Patricia Trie (MPT) - needed for state proofs
- ❌ Light client verification - prove specific values without full state
- ❌ State sync snapshots - fast node bootstrap

**Implementation Options:**

1. **Use existing Merkle tree library (Recommended)**
   - [merkle-tree-rs](https://crates.io/crates/merkle-tree) or [jellyfish-merkle](https://github.com/penumbra-zone/jellyfish-merkle)
   - Provides proof generation and verification
   - Lower implementation effort

2. **Build custom MPT**
   - Match Ethereum's state trie exactly
   - More complex but full compatibility
   - Higher implementation effort

**Files to modify:**

```
[ ] crates/chain/src/state.rs - compute_app_hash()
    - Replace sorted hash with Merkle tree root
    - Include all state components:
      - UnifiedState balances
      - Engine positions and orders
      - EVM contract storage
      - Nonces

[ ] crates/chain/src/state.rs - State proofs (NEW)
    - Add compute_balance_proof(address, token) -> MerkleProof
    - Add verify_balance_proof(proof, root) -> bool
    - Enable light client verification

[ ] crates/chain/src/cometbft/app.rs - State sync (ABCI)
    - Implement ListSnapshots for available snapshots
    - Implement OfferSnapshot to accept snapshots
    - Implement LoadSnapshotChunk for chunk retrieval
    - Implement ApplySnapshotChunk for chunk application
```

**Target State Commitment Structure:**
```
app_hash = merkle_root([
    unified_state_root,    // Merkle root of all (address, token) -> balance
    engine_state_root,     // Merkle root of positions + orders
    evm_state_root,        // Ethereum-style state trie root
    nonce_root,            // Merkle root of all nonces
    block_metadata         // height, timestamp, prev_hash
])
```

**Priority:** This is MEDIUM priority because:
- Current implementation works correctly for single-node deployment
- Tests pass (122/122) with current approach
- Merkle proofs needed only for: multi-node sync, light clients, external verification

---

### ✅ P1: EIP-712 Signature Verification - 100% COMPLETE

**Files implemented:**

1. **Gateway EIP-712** (`crates/gateway/src/eip712.rs`):
   ```
   [x] Proper EIP-712 struct encoding matching TypeScript SDK
   [x] compute_typed_data_hash() - Full EIP-712 hash computation
   [x] compute_domain_separator() - Domain separator with chain ID
   [x] compute_action_struct_hash() - Per-action type hash encoding
   [x] All action types supported: Order, Cancel, CancelByCloid, CancelAll,
       UpdateLeverage, UsdTransfer, Withdraw, SpotOrder, SpotCancel,
       SpotCancelAll, ViewTransfer
   ```

2. **Chain EIP-712** (`crates/chain/src/tx.rs`):
   ```
   [x] Proper 32-byte padding for all EIP-712 values
   [x] encode_string() - keccak256 hash of string bytes
   [x] encode_uint8() / encode_uint64() - 32-byte big-endian padded
   [x] encode_bool() - 32-byte with 0/1 in last byte
   [x] encode_address() - 32-byte left-padded
   [x] encode_array() - keccak256 of concatenated hashes
   [x] TransactionType::compute_struct_hash() - All transaction types
   [x] OrderWire::compute_hash_eip712() - Order struct hash
   [x] CancelWire::compute_hash_eip712() - Cancel struct hash
   [x] CancelByCloidWire::compute_hash_eip712() - Cancel by CLOID hash
   ```

3. **SDK Signing** (`sdk/typescript/src/signing.ts`):
   ```
   [x] transformOrderForSigning() - Converts OrderTypeWire to TIF string
   [x] Message transformation to match EIP-712 type definitions
   [x] Lowercase TIF strings for consistency
   ```

4. **E2E Tests** (`scripts/e2e/lib/signing.ts`):
   ```
   [x] Production-like EIP-712 signing using viem
   [x] All test accounts use proper signatures
   ```

**Production Mode:**
- All signatures are cryptographically verified using EIP-712
- No bypass mechanisms available
- Hash computation matches exactly between TypeScript SDK and Rust backend
- USD transfers, orders, and all other actions work with proper signatures

**Current E2E Status:** ✅ 122/122 tests passing

### ✅ P2: Query Handlers - COMPLETED

**File modified:** `crates/gateway/src/handlers.rs`

```
[x] Implement UserFills query - Real data from fill history
[x] Implement FundingHistory query - Real data from market funding
[x] Implement UserFundingHistory query - Real data from user funding
[x] Implement RecentTrades query - Real data from trade feed
[ ] Implement CandleSnapshot query - Requires aggregation (typically indexer)
```

### 🟡 P2: WebSocket Broadcasting

**File to modify:** `crates/gateway/src/websocket.rs`

```
[ ] Implement trade broadcast on fills
[ ] Implement orderbook update broadcasts
[ ] Implement user-specific update broadcasts
[ ] Fix unsubscribe cleanup
```

---

## Phase 4: State Persistence ✅ 100% COMPLETE

### ✅ Phase 4A: Persistence Infrastructure - COMPLETE

**New crate created:** `crates/persistence/`

The persistence layer provides RocksDB-based state storage with:

```
[x] RocksDB backend with column families
[x] Column family schema for all state types:
    - Balances (unified state)
    - Positions, Orders, Accounts (perpetuals)
    - SpotTokens, SpotMarkets, SpotReserved (spot)
    - EvmAccounts, EvmStorage, EvmCode (EVM)
    - Nonces, BlockMeta, BlockHashes (chain)
    - Fills, FundingPayments, Trades (history)
[x] Binary key encoding for efficient storage
[x] Write batches for atomic operations
[x] WAL (Write-Ahead Log) for crash recovery
[x] State snapshot support
[x] 25 unit tests passing
```

**Key files:**
- `crates/persistence/src/lib.rs` - PersistenceBackend trait and WriteBatch
- `crates/persistence/src/column_families.rs` - 24 column families defined
- `crates/persistence/src/keys.rs` - Binary key encoding
- `crates/persistence/src/rocksdb_backend.rs` - RocksDB implementation
- `crates/persistence/src/state.rs` - PersistedState serialization types

**Node integration:**
- `crates/node/Cargo.toml` - Added `persistence` optional feature
- `crates/node/src/main.rs` - `--enable-persistence` and `--data-dir` CLI options
- Graceful shutdown with flush

**Usage:**
```bash
# Enable persistence (requires building with feature)
cargo build -p hypercore-node --features persistence

# Start with persistence enabled
hypercore start --enable-persistence --data-dir ./data/chain

# Without persistence (default, in-memory mode)
hypercore start
```

### ✅ Phase 4B: State Save/Restore - COMPLETE

```
[x] Save complete state to RocksDB on block commit
    - StatePersister for serializing/deserializing state
    - PostCommitHandler callback on BlockProducer
    - Automatic state extraction after each block
[x] Restore state from RocksDB on node startup
    - Check for persisted state before genesis init
    - Restore unified balances, positions, orders
    - Restore spot engine state (tokens, markets, orders)
    - Restore nonces, CLOID mappings, block metadata
[x] Schema migration support (version checking)
[x] State validation after restore
```

**New files:**
- `crates/persistence/src/persister.rs` - StatePersister with persist/load
- `crates/persistence/src/extractor.rs` - StateExtractor builder
- `crates/chain/src/persistence_integration.rs` - extract_state/restore_state

### 🟡 P2: Implement Export/Import

**File to modify:** `crates/node/src/main.rs`

```
[ ] Implement export command (state snapshot to file)
[ ] Implement import command (restore from snapshot)
```

### 🟡 P2: Enable State Sync

**File to modify:** `crates/chain/src/abci.rs`

```
[ ] Implement ListSnapshots
[ ] Implement OfferSnapshot
[ ] Implement LoadSnapshotChunk
[ ] Implement ApplySnapshotChunk
```

---

## Phase 5: Indexer Completion ✅ COMPLETED

### ✅ P2: Connect Indexer to Node - COMPLETED

**File modified:** `crates/node/src/main.rs`

```
[x] Start indexer service when enabled (--indexer flag + DATABASE_URL env)
[x] Pass engine events to indexer (via shared EngineState)
[x] Handle indexer errors gracefully (async spawn with error logging)
[x] Feature-flagged compilation (--features indexer)
```

### ✅ P2: Fix Indexer Engine Calls - COMPLETED

**File modified:** `crates/indexer/src/ingest.rs`

```
[x] BlockEvent types shared via hypercore-primitives
[x] Event deserialization from JSON
[x] Process all event types (Fill, OrderPlaced, OrderCanceled, etc.)
[x] Candle aggregation from fills
```

### ✅ P2: Block Events Emission - COMPLETED

**Files modified:**
- `crates/chain/src/state.rs` - Added pending_block_events accumulator
- `crates/chain/src/app.rs` - Emit FillEvent, OrderPlacedEvent during execution
- `crates/primitives/src/events.rs` - Shared BlockEvent types

```
[x] FillEvent emitted for both maker and taker on each fill
[x] OrderPlacedEvent emitted for resting orders
[x] FundingAppliedEvent emitted at end_block
[x] LiquidationEvent emitted at end_block
[x] Events stored to shared EngineState for indexer access
```

### ✅ P2: Candle Generation - COMPLETED

**File created:** `crates/indexer/src/candles.rs`

```
[x] CandleAggregator for OHLCV generation
[x] Support for 1m, 5m, 15m, 1h, 4h, 1d intervals
[x] Upsert candle on each taker fill
[x] Backfill from existing fills (for recovery)
[x] Unit tests for candle bound calculations
```

---

## Phase 6: Security Hardening

### 🟡 P2: Input Validation

```
[ ] Validate all user inputs
[ ] Add size limits
[ ] Sanitize addresses
[ ] Validate prices and sizes against market config
```

### 🟡 P2: Rate Limiting

```
[ ] Add rate limiting to gateway
[ ] Implement per-IP limits
[ ] Implement per-account limits
```

### 🟢 P3: Security Audit

```
[ ] Internal security review
[ ] External security audit
[ ] Bug bounty program
```

---

## Phase 7: Testing & Documentation

### 🟡 P2: Integration Tests

```
[x] Add comprehensive liquidation unit tests (15 new tests)
[x] Add risk calculation edge case tests (27 new tests)
[x] Add error handling unit tests
[x] Add E2E risk/margin tests (20+ new tests)
[x] Add E2E fill price validation tests
[x] Add E2E fee calculation tests
[ ] Add Rust integration tests for ABCI flow
[ ] Add EVM execution tests
[ ] Add consensus simulation tests
```

**Test Coverage Summary (January 2026):**

| Category | Unit Tests | E2E Tests | Coverage |
|----------|-----------|-----------|----------|
| Liquidation Engine | 23 tests | - | High |
| Risk Engine | 34 tests | - | High |
| Order Matching | 15 tests | 10+ tests | High |
| Position Management | 5 tests | 15+ tests | High |
| Fee Calculation | - | 5 tests | Medium |
| Margin Validation | 10 tests | 10+ tests | High |
| Error Handling | 10+ tests | 5+ tests | High |

### 🟢 P3: Documentation

```
[ ] API documentation with examples
[ ] Deployment guide
[ ] Operator manual
[ ] SDK tutorials
```

---

## Missing Engine Methods

Most methods are now implemented in `crates/engine/src/state.rs`:

```rust
// Block info - ✅ ALL IMPLEMENTED
[x] current_height() -> u64
[x] set_block_height(height)
[x] get_block_hash(height) -> Option<[u8; 32]>
[x] get_block_timestamp(height) -> Option<u64>
[x] get_block_tx_count(height) -> Option<u32>
[x] get_block_events(height) -> Vec<Event>
[x] store_block_metadata(height, metadata)

// Queries - ✅ ALL IMPLEMENTED
[x] get_all_user_orders(user) -> Vec<&Order>
[x] get_user_fills(user, limit) -> Vec<&Fill>
[x] get_market_funding_history(market, limit) -> Vec<(Timestamp, SignedAmount)>
[x] get_user_funding_history(user, limit) -> Vec<&FundingPayment>
[x] get_recent_trades(market, limit) -> Vec<&Fill>
[ ] get_candles(market, interval, start, end) -> Vec<Candle> (requires aggregation)

// Market info
[ ] get_market_id_by_name(name) -> Option<MarketId>
[ ] get_market_name(id) -> Option<String>
[ ] get_all_markets() -> Vec<&Market>
[ ] get_all_mid_prices() -> HashMap<String, Decimal>

// Funding
[ ] last_funding_time(market) -> u64
[ ] set_last_funding_time(market, time)
[ ] apply_funding_payment(user, market, amount)
[ ] should_apply_funding(market) -> bool
[ ] apply_funding(market)

// Liquidation
[ ] apply_liquidation(liquidation_result)

// State
[ ] compute_state_root() -> [u8; 32]
```

---

## Missing Primitives Methods

These methods are referenced but not implemented in `crates/primitives/`:

```rust
// Decimal
[ ] scale_to(decimals: u8) -> Self
[ ] to_be_bytes() -> [u8; 32]
[ ] to_be_bytes_signed() -> [u8; 32]
[ ] is_negative() -> bool

// Position
[ ] unrealized_pnl field
[ ] margin_used field
[ ] liquidation_price field
[ ] leverage field
[ ] is_long() method

// Order
[ ] is_resting_order() method

// MarketState struct (may be missing entirely)
```

---

## Quick Wins (Can be done independently)

1. ~~**Fix Docker healthcheck** - Currently checks 8545 which doesn't exist~~ ✅ EVM RPC server now exists

2. **Add missing Decimal methods** - Required by precompiles
   - Implement scale_to, to_be_bytes, etc.

3. **Add Position fields** - Required by precompiles
   - Add unrealized_pnl, margin_used, etc.

4. ~~**Update CLI help** - Document actual behavior~~ ✅ EVM is now functional

---

## Estimated Effort

| Phase | Focus | Dependencies | Status |
|-------|-------|--------------|--------|
| Phase 1a (EVM RPC) | JSON-RPC server | None | ✅ **COMPLETED** |
| Phase 1b (Token Standards) | ERC20/721/1155 | Phase 1a | ✅ **COMPLETED** |
| Phase 1c (HIP-1 Tokens) | Spot trading | Phase 1b | ✅ **COMPLETED** |
| **Phase 2A (Unified State)** | **Master balance sheet** | **Phase 1** | ✅ **COMPLETED** |
| **Phase 2B (Consensus)** | **ABCI, CometBFT, queries** | **Phase 2A** | ✅ **COMPLETED** |
| **Phase 3A (Genesis State)** | **Proper initialization** | **Phase 2B** | ✅ **COMPLETED** |
| **Phase 3B (Gas Fees)** | **EVM gas, zero-gas trading** | **Phase 2B** | ✅ **COMPLETED** |
| **Phase 3C (State Hardening)** | **Merkle proofs, snapshots** | **Phase 3A** | 🟡 **PLANNED** |
| **Phase 3D (EIP-712 Production)** | **Proper EIP-712 encoding** | **Phase 3A** | ✅ **COMPLETED** |
| **Phase 4A (Persistence Infra)** | **RocksDB, WAL, Column Families** | **Phase 3** | ✅ **COMPLETED** |
| **Phase 4B (State Save/Restore)** | **Auto-persist, restore on start** | **Phase 4A** | ✅ **COMPLETED** |
| **Phase 5 (Indexer)** | **Historical data, candles** | **Phase 4** | ✅ **COMPLETED** |
| Phase 6 (Security) | Hardening | Phase 1-5 | ⚪ Pending |
| Phase 7 (Testing) | Comprehensive testing | Phase 1-5 | 🔄 Ongoing |

**Phases 1-5 Complete!** Core functionality, consensus, persistence, and indexer fully implemented.

### Phase 3 Overview (Production Infrastructure)

Based on research into [Hyperliquid's architecture](https://hyperliquid.gitbook.io/hyperliquid-docs) and [CometBFT consensus](https://docs.cometbft.com/):

| Sub-Phase | Goal | Key Insight |
|-----------|------|-------------|
| **3A: Genesis** | Start chain with initial state | Currently using runtime creditBalance() - doesn't work for multi-node |
| **3B: Gas Fees** | Zero gas for trading, fees for EVM | Hyperliquid charges zero gas for HyperCore, HYPE for HyperEVM |
| **3C: State Commitment** | Production-grade Merkle proofs | Current hash is simple - need proper tree for light clients |

### Phase 2B Completion Summary

| Component | Implementation | Status |
|-----------|----------------|--------|
| HyperCoreApp | `crates/chain/src/app.rs` | ✅ |
| Transaction Execution | Order, Cancel, CancelByCloid, etc. | ✅ |
| BlockProducer | `crates/chain/src/block_producer.rs` | ✅ |
| Gateway → ABCI | `crates/gateway/src/handlers.rs:execute_perp_action_via_app()` | ✅ |
| State Commitment | `crates/chain/src/state.rs:compute_app_hash()` | ✅ |
| Query Endpoints | OpenOrders, UserFills, FundingHistory, etc. | ✅ |
| Fill/Trade History | `crates/engine/src/state.rs:record_fill()` | ✅ |
| Funding Application | `crates/node/src/main.rs:funding_processor()` | ✅ |
| **CometBFT ABCI Server** | `crates/chain/src/cometbft/server.rs` | ✅ |
| **CometBftApp (Application)** | `crates/chain/src/cometbft/app.rs` | ✅ |
| **Validator Set Management** | `crates/chain/src/cometbft/validators.rs` | ✅ |
| **Multi-node Consensus Mode** | `crates/node/src/main.rs:ConsensusMode` | ✅ |

### What's Next: Phase 3C (State Hardening) & Phase 6 (Security)

Phase 3D (EIP-712) and Phase 4 (Persistence) are now complete:

- ✅ RocksDB backend with 24 column families
- ✅ State save on block commit (PostCommitHandler)
- ✅ State restore on node startup
- ✅ Proper EIP-712 struct encoding in gateway (`eip712.rs`)
- ✅ Proper EIP-712 struct encoding in chain (`tx.rs`) with 32-byte padding
- ✅ SDK message transformation for EIP-712 signing
- ✅ All signatures cryptographically verified - no bypass mechanisms
- ✅ USD transfers work correctly with proper signature verification

**Remaining high-priority work:**

1. **Phase 3C: State Commitment Hardening**
   - Implement proper Merkle tree (not just sorted hash)
   - Add state proofs for light client verification
   - Enable ABCI state sync snapshots

2. **Phase 6: Security Hardening**
   - Add rate limiting middleware
   - Add input validation for orders/transfers
   - Add request size limits

**Usage (production mode):**
```bash
# With persistence
cargo build -p hypercore-node --features persistence
hypercore start --enable-persistence --data-dir ./data/chain
```

**Test Status (Updated January 2026):**
- 175+ Rust unit tests passing (86 engine tests + 89 other crates)
- 135 E2E integration tests passing (13 new risk/margin tests added)
- All tests run with production-like EIP-712 signature verification

**New Test Files Added:**
- `crates/engine/src/liquidation.rs` - 23 liquidation unit tests (15 new)
- `crates/engine/src/risk.rs` - 34 risk engine unit tests (27 new)
- `scripts/e2e/tests/risk.ts` - 13 E2E risk/margin tests (new)
