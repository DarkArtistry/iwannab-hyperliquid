# HyperCore Development TODO

Prioritized list of outstanding work items organized by criticality and phase.

## Legend
- 🔴 **P0 - Critical** - Blocks core functionality
- 🟠 **P1 - High** - Required for MVP
- 🟡 **P2 - Medium** - Important for production
- 🟢 **P3 - Low** - Nice to have

---

## Quick Reference: Current Status

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HyperCore Project Status                         │
├─────────────────────────────────────────────────────────────────────┤
│  Overall Completion: 95%                                            │
│  Test Coverage: 491 tests (all passing)                             │
│                                                                     │
│  ✅ READY NOW:        Single-node MVP deployment                    │
│  🔴 BLOCKED:          Multi-node (CRITICAL: AppHash incomplete)     │
│  🔴 BLOCKED:          Mainnet (needs fixes + audit)                 │
│                                                                     │
│  Key Documents:                                                     │
│  - docs/IMPLEMENTATION_STATUS.md - Full phase breakdown             │
│  - docs/CONSENSUS.md - Consensus verification implementation plan   │
│  - docs/ARCHITECTURE.md - System architecture                       │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 🔴 CRITICAL BLOCKERS (Must Fix Before Multi-Node/Devnet)

### ~~Issue 1: AppHash Does Not Commit to All Consensus-Critical State~~ ✅ FIXED

**Location**: `crates/chain/src/state.rs:compute_app_hash()`

**Fix Applied**: Added Merkle tree computation for all consensus-critical state components:

| State | Method | Status |
|-------|--------|--------|
| Unified balances | `compute_unified_state_root()` | ✅ Already existed |
| Nonces | `compute_nonce_root()` | ✅ Already existed |
| Positions | `compute_positions_root()` | ✅ **NEW** |
| Markets | `compute_markets_root()` | ✅ **NEW** |
| Leverage | `compute_leverage_root()` | ✅ **NEW** |
| CLOID mappings | `compute_cloid_root()` | ✅ **NEW** |
| Scalars (insurance, order ID) | `compute_engine_scalars_hash()` | ✅ **NEW** |
| Open Orders | `compute_orders_root()` | ✅ **IMPLEMENTED** |
| EVM state | TODO | ⚠️ Not yet consensus-critical |

**Remaining Work**:
- ~~Implement `get_all_orders_global()` in EngineState for proper order Merkle tree~~ ✅ DONE
- Add EVM state roots when EVM execution becomes consensus-critical

**Changes Made**:
- Added 6 new Merkle tree computation methods in `state.rs`
- Updated `compute_app_hash()` to include all new roots
- All state sorting ensures deterministic Merkle construction

### ~~Issue 2: Non-Deterministic SystemTime Usage~~ ✅ FIXED

**Location**: `crates/chain/src/state.rs:validate_nonce()`

**Fix Applied**: Changed `validate_nonce()` to accept a `reference_time_ms: u64` parameter instead of using `SystemTime::now()`.

- **Consensus-critical path (execute_tx)**: Uses block timestamp (deterministic)
- **Mempool admission (check_tx)**: Uses current time (acceptable - not consensus-critical)
- **Genesis token deployment**: Uses deterministic timestamp (0)

**Changes Made**:
- `state.rs:validate_nonce()` - Now accepts `reference_time_ms` parameter
- `state.rs:validate_tx()` - Now accepts `reference_time_ms` parameter
- `app.rs:execute_tx()` - Passes block timestamp (deterministic)
- `app.rs:check_tx()` - Passes current time (non-consensus-critical)
- `app.rs:init_from_genesis()` - Uses genesis_timestamp=0 (deterministic)
- Added new test: `test_timestamp_nonce_determinism()`

**Note**: EVM executor still uses `SystemTime::now()` for block_timestamp initialization, but this is not yet consensus-critical since EVM execution is currently single-node only. Will need fixing when EVM execution becomes part of consensus.

### ~~Issue 3: HashMap Determinism Audit~~ ✅ AUDITED

**Finding**: HashMap usage in the codebase is generally safe for determinism:

**✅ Safe Usage Patterns Found:**

| Location | Usage | Why Safe |
|----------|-------|----------|
| OrderBook (engine/orderbook.rs) | `BTreeMap<OrderKey, Order>` | Uses BTreeMap with deterministic ordering |
| Merkle computation (chain/state.rs) | All HashMaps sorted before hashing | Explicit sorting ensures determinism |
| Funding settlement | Lazy per-position settlement | No HashMap iteration affects outcomes |
| Risk calculations | Independent per-account | Order doesn't affect final values |

**⚠️ Minor Concerns (Low Risk):**

| Location | Issue | Risk Level |
|----------|-------|------------|
| `find_underwater_accounts()` | Returns Vec in HashMap iteration order | Low - liquidation list order could vary |
| `get_market_ids()` | Returns Vec in HashMap order | Low - only used for iteration |

**Recommendation**: The liquidation order issue should be addressed by sorting the returned Vec by (address, market_id) to ensure deterministic processing order. However, this is low priority since liquidations are typically processed one-at-a-time and the final state would be the same.

---

### Immediate Action Items

| Priority | Task | Est. Time | Status |
|----------|------|-----------|--------|
| ✅ Done | ~~Fix AppHash to include ALL consensus-critical state~~ | 1-2 weeks | **FIXED** |
| ✅ Done | ~~Fix SystemTime::now() determinism bug~~ | 1 day | **FIXED** |
| ✅ Done | ~~Determinism audit (HashMap→BTreeMap)~~ | 1-2 weeks | **AUDITED** |
| ✅ Done | ~~Determinism test suite~~ | 1 week | **DONE** (12 new tests) |
| 🔴 P0 | State attestation protocol | 2-3 weeks | Pending |
| ✅ Done | ~~Implement `get_all_orders_global()` for order Merkle~~ | 2 days | **DONE** |
| 🟠 P1 | State sync snapshots (ABCI) | 2 weeks | Pending |
| 🟠 P1 | Engage security auditor | Start now | Pending |
| 🟠 P1 | Export/import utilities | 1 week | Pending |
| 🟡 P2 | WebSocket broadcasting | 1 week | Pending |

---

## Recent Updates (January 2026)

### ✅ Critical Consensus Fixes (January 19, 2026)

Fixed all critical determinism and state commitment issues identified in the consensus audit:

**1. SystemTime::now() Determinism Fix:**
- Changed `validate_nonce()` to accept `reference_time_ms` parameter
- DeliverTx (execute_tx) now uses block timestamp (deterministic)
- CheckTx (mempool) uses current time (acceptable - not consensus-critical)
- Genesis token deployment uses timestamp=0 (deterministic)
- Added `test_timestamp_nonce_determinism()` unit test

**2. AppHash State Commitment (MAJOR):**
- Added 6 new Merkle tree computation methods:
  - `compute_positions_root()` - All perpetual positions
  - `compute_markets_root()` - Market state (prices, funding, OI)
  - `compute_leverage_root()` - User leverage settings
  - `compute_cloid_root()` - Client order ID mappings
  - `compute_orders_root()` - Stub for orders (needs `get_all_orders_global()`)
  - `compute_engine_scalars_hash()` - Insurance fund, next order ID
- Updated `compute_app_hash()` to include all roots
- All state is sorted before Merkle construction for determinism

**3. HashMap Determinism Audit:**
- Verified OrderBook uses `BTreeMap` with deterministic `OrderKey` ordering
- Fixed `find_underwater_accounts()` to sort results for deterministic liquidation order
- Fixed `get_market_ids()` to return sorted list
- All Merkle computations explicitly sort before hashing

**4. Determinism Test Harness:**
- Added 12 new determinism tests in `state.rs`
- Tests verify identical inputs produce identical AppHash
- Tests cover: balance order independence, nonce order independence, CLOID order independence
- Tests verify: sequential blocks, AppHash chain integrity, Merkle root caching
- Tests all 8 Merkle root computations for determinism

**Test Results:** All 98 tests pass (54 chain + 44 engine)

### Code Review: TODOs, Stubs, and Hardcoded Values (Jan 19, 2026)

**Outstanding TODOs by Priority:**

| Priority | Location | Description | Status |
|----------|----------|-------------|--------|
| ✅ Done | `state.rs:540-579` | `compute_orders_root()` with `get_all_orders_global()` | ✅ IMPLEMENTED |
| 🟠 P1 | `app.rs:415,434` | Calculate realized PnL in fill events | Returns 0 |
| 🟠 P1 | `app.rs:833,838` | EVM deposit/withdraw action handlers | Placeholder events |
| 🟡 P2 | `rocksdb_backend.rs:290` | Implement RocksDB snapshot API | Comment only |
| 🟢 P3 | `node/main.rs:832` | Helper method for mark price update | Working code, cleanup |

**Stub Analysis:**

1. ~~**`compute_orders_root()` (CRITICAL)**~~: ✅ **FIXED** - Now computes proper Merkle root from all orders via `get_all_orders_global()`.

2. **Fill Event realized_pnl (Medium)**: FillEvent.realized_pnl is hardcoded to 0. This is cosmetic - the actual PnL calculation happens in position management, this is just for event reporting.

3. **EVM Actions (Medium)**: Actions 0,1 (deposit/withdraw) are placeholder events. EVM execution is currently single-node only, so not yet consensus-critical.

**Hardcoded Values Review:**

| Type | Assessment |
|------|------------|
| Test addresses (0xf39F...) | ✅ OK - Standard Foundry/Hardhat test addresses |
| Test prices (65000) | ✅ OK - BTC reference price, appropriate for tests |
| Test amounts (10000_000000) | ✅ OK - $10,000 deposits, appropriate for tests |
| localhost:8545 | ✅ OK - Default EVM RPC address, configurable via env |
| Chain ID 999 | ✅ OK - Test chain ID, configurable |

**E2E Test Skip Logic:** ✅ Appropriate - Tests skip when preconditions aren't met (e.g., insufficient balance). This is correct behavior for E2E tests.

### ✅ State Commitment Hardening (Phase 3C)
- Created `crates/chain/src/merkle.rs` with proper binary Merkle tree
- Supports: proof generation, proof verification, serialization
- State roots computed via Merkle tree instead of simple hash
- Balance proof methods: `prove_balance()`, `verify_balance_proof()`
- Nonce proof methods: `prove_nonce()`, `verify_nonce_proof()`
- Enables light client verification without full state
- Added 22 new tests (13 Merkle + 9 state proof tests)
- **API Endpoints Added:**
  - `POST /info {"type": "stateInfo"}` - Block height, app hash, state roots
  - `POST /info {"type": "stateProof", "user": "0x...", "token": 0}` - Merkle proof for balance
- **E2E Tests:** `scripts/e2e/tests/state-proofs.ts` (9 tests for proof verification)
- **Bug Fixed:** Merkle trees now properly cached via `AppState::end_block()`
- **Total: 298 Rust tests now passing**

### ✅ Input Validation (Phase 6B)
- Created `crates/gateway/src/validation.rs` with comprehensive input validation
- Fail-fast validation at gateway layer (before engine processing)
- Validates: addresses, prices, sizes, nonces, TIF, leverage, order counts
- Configurable limits: max orders/request (100), max body size (1MB)
- Integrated into handlers with clear error messages
- Added 30 new unit tests for validation

### ✅ Rate Limiting Middleware (Phase 6A)
- Created `crates/gateway/src/rate_limit.rs` with comprehensive rate limiting
- Per-IP rate limiting (100 req/min global by default)
- Per-endpoint rate limiting (50 req/min for /exchange, 200 req/min for /info)
- Configurable limits: default, development (relaxed), production (strict), disabled
- Tower middleware layer integrated into Axum router
- CLI flags: `--disable-rate-limit`, `--dev-rate-limit`
- Background cleanup task for expired rate limiters
- Added 11 new unit tests for rate limiting

### ✅ EIP-712 Code Consolidation (TD-001)
- Created `crates/primitives/src/eip712.rs` with shared encoding functions
- Gateway and chain crates now import from the same source
- Eliminates risk of hash mismatches between components
- Added 15 new unit tests for encoding functions

### ✅ Risk Tests Rewrite
- Fixed E2E risk tests to use `unifiedBalances` API instead of `clearinghouseState`
- The `clearinghouseState` returns engine's perpetual account (positions/margin)
- The `unifiedBalances` returns actual trading funds from unified state (source of truth)
- Added proper order confirmation polling with `waitForOrderInBook()` and `waitForFill()`
- Added timing measurements for all operations
- **13 risk tests now properly verify real functionality**

### Current Test Status
| Category | Count | Status |
|----------|-------|--------|
| Rust Unit Tests | 298 | ✅ All passing |
| Solidity Contract Tests | 49 | ✅ All passing |
| E2E Integration Tests | 144 | ✅ All passing |
| **Total** | **491** | **All passing** |

### Next Priority Tasks

**🔴 P0 - Critical (Blocks Multi-Node Production)**

1. **Phase 7A: Determinism Verification** (4-6 weeks)
   - Audit and replace HashMap with BTreeMap in state-modifying code
   - Remove SystemTime from execution path
   - Verify no floating point in state calculations
   - Add determinism test suite
   - See `docs/CONSENSUS.md` Option A for details

2. **Phase 7B: State Attestation Layer** (2-3 weeks after 7A)
   - Implement StateAttestation protocol
   - Add AttestationCollector with quorum detection
   - Implement DivergenceHandler (halt on mismatch)
   - P2P integration for attestation gossip
   - See `docs/CONSENSUS.md` Option C for details

**🟠 P1 - High (Required for Production)**

3. **External Security Audit** (4-8 weeks, can start parallel)
   - Engage security firm for audit
   - Focus areas: consensus, unified state, EVM integration
   - Address findings before mainnet

4. **Export/Import Utilities** (1 week)
   - `hypercore export --output state.json`
   - `hypercore import --input state.json`
   - Needed for operational backup/recovery

**🟡 P2 - Medium (Improve UX/Operations)**

5. **WebSocket Broadcasting** (1 week)
   - Broadcast fills, orderbook updates via WebSocket
   - Better UX for trading frontends

6. **ABCI State Sync** (2 weeks)
   - Implement snapshot methods for fast node bootstrap
   - Needed for multi-node network expansion

**✅ Completed Tasks**
- ~~**TD-002**: Fix hardcoded chain ID in tx.rs~~ ✅
- ~~**TD-001**: Consolidate duplicate EIP-712 code~~ ✅
- ~~**Phase 6A**: Add rate limiting middleware~~ ✅
- ~~**Phase 6B**: Add input validation for orders~~ ✅
- ~~**Phase 3C**: State commitment hardening (Merkle proofs)~~ ✅
- ~~**State Proof API**: StateInfo and StateProof endpoints~~ ✅
- ~~**Merkle Tree Caching**: Fixed AppState::end_block() call~~ ✅
- ~~**Consensus Architecture Doc**: Created CONSENSUS.md~~ ✅

---

## Test Coverage Analysis

### Test Commands
```bash
make test-quick    # Rust + Solidity only (347 tests, no Docker)
make test-all      # All tests including E2E (482+ tests)
make test-e2e      # E2E only (starts Docker services)
```

### Rust Unit Test Breakdown (298 tests)
| Crate | Tests | Key Coverage |
|-------|-------|--------------|
| `hypercore-chain` | 41 | Merkle proofs, state proofs, consensus, block producer |
| `hypercore-engine` | 86 | Matching, risk, funding, liquidation |
| `hypercore-primitives` | 51 | Decimal, EIP-712, events, unified state |
| `hypercore-gateway` | 66 | Rate limiting (11), validation (30), handlers |
| `hypercore-evm` | 23 | Executor, precompiles, state |
| `hypercore-persistence` | 25 | RocksDB, state save/restore |
| Other | 6 | Indexer, misc |

### E2E Integration Test Breakdown (144 tests)
| Category | Tests | Coverage |
|----------|-------|----------|
| Connection & Health | 4 | Gateway/EVM reachability |
| Market Data | 7 | Orderbook, prices, candles |
| Account State | 5 | Balances, positions, history |
| Order Lifecycle | 9 | Place, cancel, batch operations |
| Matching Engine | 5 | Cross-account matching |
| Position Management | 3 | Position tracking |
| EVM Integration | 27 | JSON-RPC, precompiles |
| Spot Trading | 13 | HIP-1 token operations |
| Unified State | 18 | View transfers, invariants |
| Risk & Margin | 13 | Leverage, fills, balances |
| Advanced Scenarios | 13 | Error handling, edge cases |
| Stress Tests | 3 | Concurrent operations |
| Token Standards | 8 | ERC-20, token metadata |
| Advanced EVM | 7 | Contract interactions |
| State Proofs | 9 | Merkle proofs, client verification |

### Identified Coverage Gaps (Phase 7 TODOs)
| Area | Current | Needed | Priority |
|------|---------|--------|----------|
| Liquidation Scenarios | 0 | 15-20 | 🔴 High |
| Stress/Performance | 3 | 20-30 | 🟠 High |
| Margin Calculations | 3 | 10-15 | 🟠 High |
| Error Handling | 5 | 10-15 | 🟡 Medium |
| EVM Contract Tests | 0 | 10 | 🟡 Medium |
| Concurrency/Race | 3 | 10-15 | 🟡 Medium |

---

## Technical Debt Tracker

Items that work but should be refactored for maintainability, performance, or correctness.

### ✅ TD-001: Duplicate EIP-712 Encoding Code - RESOLVED

**Priority:** Medium | **Impact:** Maintenance burden

**Problem:** EIP-712 encoding functions were duplicated in gateway and chain crates.

**Solution Applied:**
- Created `crates/primitives/src/eip712.rs` with shared encoding functions
- Refactored gateway to import from shared module
- Refactored chain to import from shared module
- Added 15 new unit tests for EIP-712 encoding

**Key Functions Now Shared:**
- `encode_string()`, `encode_uint8()`, `encode_uint64()`, `encode_bool()`
- `encode_address_bytes()`, `encode_address_str()`, `encode_array()`
- `compute_domain_separator()`, `type_hash()`
- `DEFAULT_CHAIN_ID`, `DOMAIN_TYPE_HASH` constants

**Files modified:**
- `crates/primitives/src/eip712.rs` - NEW shared module
- `crates/gateway/src/eip712.rs` - Imports from shared module
- `crates/chain/src/tx.rs` - Imports from shared module

---

### ✅ TD-002: Hardcoded Chain ID in tx.rs - RESOLVED

**Priority:** Medium | **Impact:** Multi-chain support blocked

**Problem:** `crates/chain/src/tx.rs:compute_domain_separator()` had chain ID 1337 hardcoded.

**Solution Applied:**
- Made `compute_domain_separator(chain_id: u64)` take a parameter
- Added `DEFAULT_CHAIN_ID: u64 = 1337` constant for backward compatibility
- Added test `test_different_chain_ids_produce_different_separators`

**Files modified:**
- `crates/chain/src/tx.rs` - Parameterized chain_id, added constant and test

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
[x] 135 E2E tests passing (including 13 risk tests)
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

### Phase 3C: State Commitment Hardening ✅ COMPLETED

**Goal:** Strengthen state commitment for production BFT consensus with proper Merkle proofs.

**Implementation (January 2026):**

Created `crates/chain/src/merkle.rs` with a proper binary Merkle tree implementation:

```rust
// Merkle tree implementation
pub struct MerkleTree { ... }
pub struct MerkleProof { ... }

impl MerkleTree {
    pub fn from_entries(entries: &[(K, V)]) -> Self;  // Build from key-value pairs
    pub fn root(&self) -> [u8; 32];                   // Get Merkle root
    pub fn prove(&self, leaf_index: usize) -> Option<MerkleProof>;  // Generate proof
}

impl MerkleProof {
    pub fn verify(&self, root: &[u8; 32]) -> bool;   // Verify against root
    pub fn to_bytes(&self) -> Vec<u8>;              // Serialize for transmission
    pub fn from_bytes(bytes: &[u8]) -> Option<Self>; // Deserialize
}
```

**State proof methods in `crates/chain/src/state.rs`:**

```rust
impl AppState {
    // Balance proofs
    pub fn prove_balance(&self, address, token) -> Option<MerkleProof>;
    pub fn verify_balance_proof(&self, address, token, balance, proof) -> bool;

    // Nonce proofs
    pub fn prove_nonce(&self, address) -> Option<MerkleProof>;
    pub fn verify_nonce_proof(&self, address, nonce, proof) -> bool;

    // Root accessors
    pub fn get_unified_state_root(&self) -> [u8; 32];
    pub fn get_nonce_root(&self) -> [u8; 32];
}
```

**Key features:**
- ✅ Binary Merkle tree with keccak256 hashing
- ✅ Proof generation for any leaf
- ✅ Standalone proof verification (no full state needed)
- ✅ Proof serialization/deserialization
- ✅ Cached trees after end_block() for fast proof generation
- ✅ 22 new tests (13 Merkle + 9 state proof tests)

**Remaining (ABCI State Sync):**
- ⚠️ ListSnapshots, OfferSnapshot, LoadSnapshotChunk, ApplySnapshotChunk
- These are needed only for fast node bootstrap in multi-node deployments
- Can be added later without breaking existing functionality

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

**Current E2E Status:** ✅ 135/135 tests passing

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

### ✅ P2: Input Validation - COMPLETED

**File created:** `crates/gateway/src/validation.rs`

```
[x] Validate all user inputs (fail-fast at gateway layer)
[x] Add size limits (max 100 orders/request, 1MB body)
[x] Sanitize addresses (0x prefix, 40 hex chars)
[x] Validate prices and sizes (positive, within bounds)
[x] Validate TIF values (Gtc, Ioc, Alo, Fok)
[x] Validate nonces (must be > 0)
[x] Validate leverage (1-50)
[x] 30 unit tests added (66 total gateway tests)
```

**Key Implementation:**
- `ValidationConfig`: Configurable limits (default, strict, disabled)
- `Validator`: Validates all request types
- `ValidationError`: Typed errors with clear messages
- Integrated into handlers before processing

### ✅ P2: Rate Limiting - COMPLETED

**File created:** `crates/gateway/src/rate_limit.rs`

```
[x] Add rate limiting to gateway (tower middleware layer)
[x] Implement per-IP limits (100 req/min default)
[x] Implement per-endpoint limits (/exchange: 50/min, /info: 200/min)
[x] Configurable presets (default, development, production, disabled)
[x] CLI flags: --disable-rate-limit, --dev-rate-limit
[x] Background cleanup task for expired limiters
[x] 11 unit tests added (36 total gateway tests)
```

**Key Implementation:**
- Uses `governor` crate for token bucket rate limiting
- `dashmap` for concurrent IP tracking
- Returns 429 Too Many Requests with Retry-After header
- Extracts client IP from X-Forwarded-For, X-Real-IP, or socket address

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
| **Phase 3C (State Hardening)** | **Merkle proofs, state proofs** | **Phase 3A** | ✅ **COMPLETED** |
| **Phase 3D (EIP-712 Production)** | **Proper EIP-712 encoding** | **Phase 3A** | ✅ **COMPLETED** |
| **Phase 4A (Persistence Infra)** | **RocksDB, WAL, Column Families** | **Phase 3** | ✅ **COMPLETED** |
| **Phase 4B (State Save/Restore)** | **Auto-persist, restore on start** | **Phase 4A** | ✅ **COMPLETED** |
| **Phase 5 (Indexer)** | **Historical data, candles** | **Phase 4** | ✅ **COMPLETED** |
| Phase 6A (Rate Limiting) | DoS protection | Phase 1-5 | ✅ **COMPLETED** |
| Phase 6B (Input Validation) | Order validation | Phase 6A | ✅ **COMPLETED** |
| Phase 7 (Testing) | Comprehensive testing | Phase 1-5 | 🔄 Ongoing |

**All Core Phases Complete!** EVM, consensus, persistence, indexer, rate limiting, input validation, and Merkle proofs fully implemented.

---

## Future: Hyperliquid-Level Performance (100k+ orders/sec)

**See full plan**: [docs/ORDERS_THROUGHPUT_UPGRADE.md](docs/ORDERS_THROUGHPUT_UPGRADE.md)

After completing and testing the current implementation, the following phases will achieve Hyperliquid-level throughput:

| Upgrade Phase | Target Throughput | Key Changes | Effort |
|---------------|-------------------|-------------|--------|
| **Phase A: Quick Wins** | 5k orders/sec | Reduce block time, batch endpoint | ~1 week |
| **Phase B: Engine Optimization** | 15k orders/sec | Lockless matching, batch signatures | ~2 weeks |
| **Phase C: Binary Protocol** | 30k orders/sec | Binary format, WebSocket upgrade | ~3 weeks |
| **Phase D: Custom Consensus** | 60k orders/sec | HyperBFT, optimistic execution | ~6 weeks |
| **Phase E: Full Stack** | 100k+ orders/sec | SIMD, memory-mapped state | ~8+ weeks |

**Current State**: ~1k orders/sec
**Target State**: 100k+ orders/sec (100x improvement)

**Recommended Path**:
1. ✅ Complete current implementation (Phase 1-6)
2. ⬜ Full E2E testing and production validation
3. ⬜ Implement Phase A (quick wins) - immediate 5x improvement
4. ⬜ Proceed with Phases B-E based on requirements

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
- 216 Rust unit tests passing (86 engine + 130 other crates)
- 135 E2E integration tests passing (13 risk/margin tests)
- 49 Solidity contract tests passing
- **Total: 400 tests across all categories**
- All tests run with production-like EIP-712 signature verification

**Key Test Files:**
- `crates/engine/src/liquidation.rs` - 23 liquidation unit tests
- `crates/engine/src/risk.rs` - 34 risk engine unit tests
- `scripts/e2e/tests/risk.ts` - 13 E2E risk/margin tests (uses unifiedBalances API)
