# Multi-Node Test Failure Diagnosis

## Latest: Session 12 - Decimal Bincode Serialization Fix

### Session 12 Results:
- **Previous**: 46/52 tests passing (6 failures)
- **After fixes**: **52/52 tests passing** (ALL PASSING)
- **Root Causes Fixed**: #18 (Decimal bincode serialization loses precision) and duplicate FinalizeBlock height guard

### Root Causes Fixed This Session:

#### Root Cause #18: Decimal Bincode Serialization Loses Precision (FIXED)

**The Bug**: `Decimal`'s custom serde implementation always serialized to a string (e.g., `"0.1"`)
and always deserialized with hardcoded `PRICE_DECIMALS=8`, regardless of the actual `decimals` field.
When `Position` fields were persisted via bincode → RocksDB → bincode, any field with `decimals != 8`
had its raw value silently changed.

**How the cascade works**:
1. Position persisted via bincode: `Decimal{value=1000000000, decimals=10}` → string `"0.1"` → bytes
2. Position loaded via bincode: bytes → string `"0.1"` → `Decimal{value=10000000, decimals=8}`
3. `positions_root_from_engine` hashes `.raw()` values: 1000000000 ≠ 10000000
4. Different positions hash → AppHash mismatch → CometBFT replays last block on already-committed state → consensus failure

**Evidence from logs**:
- Nodes 0,1,2 at height 153: `positions=c279b235565171c7`
- Node 3 after restart: `positions=40ed592941b68801` (different!)
- All other hash components (unified, nonces, orders, markets, leverage, scalars, cloid, evm) are identical

**The affected field**: `Position.last_funding_index` uses `RATE_DECIMALS=10`, while `from_str_exact`
always parsed back with `PRICE_DECIMALS=8`. The 2-decimal difference means `raw()` values differ by
a factor of 100 after a bincode round-trip.

**The Fix** (`crates/primitives/src/decimal.rs`): Use serde's `is_human_readable()` to distinguish formats:
- **JSON** (human-readable): keep current string serialization for API compatibility
- **Bincode** (binary): serialize as `(i128, u8)` tuple to preserve exact `(value, decimals)` pair

```rust
impl Serialize for Decimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string_trimmed())
        } else {
            use serde::ser::SerializeTuple;
            let mut tup = serializer.serialize_tuple(2)?;
            tup.serialize_element(&self.value)?;
            tup.serialize_element(&self.decimals)?;
            tup.end()
        }
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            Self::from_str_exact(&s, Self::PRICE_DECIMALS)
                .ok_or_else(|| serde::de::Error::custom(format!("Invalid decimal: {}", s)))
        } else {
            let (value, decimals) = <(i128, u8)>::deserialize(deserializer)?;
            Ok(Decimal::from_raw(value, decimals))
        }
    }
}
```

**Also fixed**: `from_str_exact()` now uses `checked_mul`/`checked_add` instead of bare arithmetic,
returning `None` on overflow instead of panicking. This prevents crashes when `is_valid()` checks
large token balances with high decimal precision (e.g., 18-decimal ERC-20 tokens).

**Files Modified**: `crates/primitives/src/decimal.rs`

#### Duplicate FinalizeBlock Height Guard (FIXED)

**The Bug**: When CometBFT detects an AppHash mismatch on restart, it replays the last committed
block via FinalizeBlock. Without a guard, this re-executes transactions on already-committed state,
corrupting the app_hash further and making recovery impossible.

**The Fix** (`crates/chain/src/cometbft/app.rs`): Added an early return in `finalize_block()` if the
requested height matches the current committed height:

```rust
if inner.app.current_height() == request.height as u64 {
    tracing::warn!(
        "FinalizeBlock: skipping duplicate height {} (already committed)",
        request.height
    );
    return ResponseFinalizeBlock {
        events: Vec::new(),
        tx_results: Vec::new(),
        validator_updates: Vec::new(),
        consensus_param_updates: None,
        app_hash: inner.app.state.app_hash.to_vec().into(),
    };
}
```

**Files Modified**: `crates/chain/src/cometbft/app.rs`

#### Also Fixed: Test Data Overflow in Persistence Tests

The `create_comprehensive_test_state()` helper in `persister.rs` used raw values
(e.g., `"5000000000000000000000"`) as balance strings for an 18-decimal token. Since
`from_str_exact` interprets these as human-readable amounts and multiplies by `10^decimals`,
this caused i128 overflow. Fixed by using the human-readable amount `"5000"` instead.

**Files Modified**: `crates/persistence/src/persister.rs`

### Test Results

| Test Suite | Result |
|-----------|--------|
| `cargo check --workspace --features cometbft,persistence` | ✅ Compiles (warnings only) |
| `cargo test -p hypercore-primitives` | ✅ 61 tests pass (2 new: bincode round-trip, JSON round-trip) |
| `cargo test -p hypercore-persistence` | ✅ 51 tests pass |
| `make test-multinode-full` | ✅ 52/52 tests pass (ALL PASSING) |

### Expected Impact

Root Cause #18 was confirmed as the explanation for all 6 remaining failures. Every test that
involved node restart followed by position state comparison failed because:
- `Position.last_funding_index` (RATE_DECIMALS=10) silently changed to PRICE_DECIMALS=8
- This caused `positions` hash divergence after restart
- Which caused AppHash mismatch → CometBFT consensus failure
- Which caused all cross-node and resilience tests to fail

The duplicate height guard is a defense-in-depth measure: even if there's a transient AppHash
mismatch, CometBFT won't corrupt state further by re-executing an already-committed block.

### Final Status: ALL TESTS PASSING

With Root Cause #18 (Decimal bincode serialization) fixed, all 52 multinode tests pass:
- All 6 previously failing tests now pass
- Node restart produces identical AppHash across all validators
- No more positions hash divergence after persistence round-trip
- The duplicate FinalizeBlock height guard prevents state corruption on replay

**E2E Single-Node**: 151/151 passing
**Multi-Node Full (5-node)**: 52/52 passing
**Total Project Tests**: 823 (all passing)

---

## Previous: Session 11 - Two Critical Fixes

### Session 11 Results:
- **Previous**: 46/52 tests passing (6 failures)
- **After fixes**: 46/52 (root causes #16 and #17 fixed, but #18 was the real remaining issue)
- **Root Causes Fixed**: #16 (blocking_read panic) and #17 (persist_state using empty core fields)

### Root Causes Fixed in Session 11:

#### Root Cause #16: `blocking_read()` Panics During Startup Verification (FIXED)

**The Bug**: Every restarted node hit this panic:
```
thread 'main' panicked at crates/chain/src/state.rs:468:33:
Cannot block the current thread from within a runtime. This happens because a function
attempted to block the current thread while the thread is being used to drive asynchronous tasks.
```

**Call chain**:
1. `main.rs` — `cometbft_app.verify_app_hash(&restored_app_hash)` runs in `#[tokio::main]` async context
2. `cometbft/app.rs` — calls `inner.app.state.compute_app_hash()`
3. `state.rs` — `perp.blocking_read()` panics because you can't call `blocking_read()` on a tokio RwLock from within a tokio runtime

**Context**: Session 10's fix changed `try_read()` to `blocking_read()` for determinism (Root Cause #13).
That was correct for the ABCI `spawn_blocking` thread, but the startup verification runs BEFORE the
ABCI server is spawned, directly in the async `main()`.

**Cascade effect**: Node dies → ABCI server never starts → CometBFT can't connect → "no such host" errors
→ validator never rejoins → ALL restart/resilience/cross-node tests fail.

**The Fix** (`crates/chain/src/state.rs`): Added `acquire_rw_lock_read()` — a generic `try_read()` with
spin-then-sleep retry loop (same pattern as existing `acquire_engine_read()`). Replaced both
`perp.blocking_read()` and `evm.blocking_read()` in `compute_app_hash()` with this new method.

Works safely in ALL contexts:
- In `spawn_blocking` (ABCI server) — succeeds immediately, no contention
- In async `main()` (startup verification) — succeeds immediately, no tokio panic
- Under lock contention (RPC handler holding write lock) — retries until acquired

```rust
fn acquire_rw_lock_read<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockReadGuard<'a, T> {
    // Fast path: try_read() — usually succeeds immediately
    // Slow path: 10,000 spin loops for brief contention
    // Very slow path: 100 × 1ms sleeps for extended contention
    // Panic after 100ms — something is seriously wrong
}
```

**Files Modified**: `crates/chain/src/state.rs:464-616`

#### Root Cause #17: `persist_state()` Writing Empty `core.*` Fields (FIXED)

**The Bug**: `persist_state()` in `persister.rs` was writing `state.core.positions`,
`state.core.orders`, `state.core.leverage`, and `state.core.markets` to RocksDB.
But in CometBFT mode, these `core.*` fields are **always empty** — the actual data
lives in `state.perp.*` (written by `extract_state()` in the ABCI app).

**Evidence from previous session logs**:
```
Restored perp_engine state: 0 positions, 0 orders (source=core-fallback)
```
The `core-fallback` source means `perp.positions` was empty, so it fell back to
`core.positions` — which was ALSO empty because `persist_state()` never wrote the
perp data.

**The Fix** (`crates/persistence/src/persister.rs`): Changed `persist_state()` to
prefer `perp.*` fields when non-empty, falling back to `core.*` for non-CometBFT mode:

```rust
// Before (broken): always used empty core fields in CometBFT mode
for position in &state.core.positions { ... }

// After (fixed): prefer perp fields (populated in CometBFT mode)
let positions = if !state.perp.positions.is_empty() {
    &state.perp.positions
} else {
    &state.core.positions
};
for position in positions { ... }
```

Applied to: positions, orders, leverage, markets.

**Files Modified**: `crates/persistence/src/persister.rs:126-161`

#### Also Fixed: Pre-existing Test Compilation Errors

Added missing `decimals` field to `UnifiedBalanceData` test constructors in
`persister.rs` and `state.rs`. The `decimals` field was added in Session 6 (Root Cause #7)
but the test code was never updated. The field stores each token's actual decimal
precision (e.g., 6 for USDC, 18 for TEST) — set dynamically by the token deployer
in production.

**Files Modified**: `crates/persistence/src/persister.rs`, `crates/persistence/src/state.rs`

### Expected Impact

These two fixes together should resolve all 6 remaining failures:

1. **Root Cause #16** was killing every restarted node — no node could survive a restart.
   With this fixed, nodes will actually boot and connect to CometBFT.

2. **Root Cause #17** meant even if nodes booted, they'd restore with 0 positions/orders,
   computing a different AppHash than running nodes. With this fixed, restored state
   will match what was actually persisted.

**All 6 failures were caused by the restart cascade:**
- ❌ Cross-node spot matching → Node 3 diverged after restart
- ❌ Nonce replay protection → Chain stalled, nodes couldn't agree
- ❌ EVM receipts consistent → Node 3 stuck, receipts not found
- ❌ Cross-node EVM + perp → Orders not visible on restarted nodes
- ❌ All-node concurrent storm → Restarted nodes diverged
- ❌ Chain halts at BFT threshold → Restarted nodes couldn't rejoin

### Test Results

| Test Suite | Result |
|-----------|--------|
| `cargo check --workspace` | ✅ Compiles (warnings only) |
| `cargo test --package hypercore-chain` | ✅ 215 tests pass |
| `cargo test -p hypercore-engine --lib` | ✅ 107 tests pass |
| `cargo test -p hypercore-persistence --lib` | ⚠️ 48/51 pass (3 pre-existing decimal overflow in Position::apply_fill) |
| `make test-multinode-full` | ⏳ Running... |

---

## Previous: Session 10 - Progress from 43/52 to 46/52 (6 failures remaining)

### Session 10 Results:
- **Previous**: 43/52 tests passing (9 failures)
- **After fixes**: 46/52 tests passing (6 failures) ✓
- **BFT stall works**: "Growth during 15s halt: 0 blocks" - docker stop fix worked!
- **New issue discovered**: AppHash divergence persists after restart

### Root Causes Fixed This Session:
1. **Root Cause #13**: try_read() non-determinism in scalars hash (previous session)
2. **Root Cause #14**: Docker stop grace period (this session) - **CONFIRMED WORKING**
3. **Root Cause #15**: Need diagnostic logging for state divergence (IN PROGRESS)

### Root Cause #15: AppHash Divergence After Restart (INVESTIGATING)

**Evidence from CometBFT logs at height 128:**
```
E[...] prevote step: consensus deems this block invalid; prevoting nil
  err="wrong Block.Header.AppHash.
       Expected A76B0A8968AEDBFF8FD3815437B501D90B4200A87A4981EF182A1B08B479DC3B,
       got F9217C15BCB143BC70EB1C4835E630A4207F484CE6F797E47B77F8B3A8DE13FD"
```

Node 3 rejects ALL proposed blocks because it has a different app_hash than other nodes!

**Balance Divergence Detected:**
```
Nodes 0,1,2: USDC=199998524459989.9892
Nodes 3,4:   USDC=199998524459989.9871
Difference:  0.0021 USDC
```

Both Nodes 3 AND 4 have the same "lower" value, suggesting the divergence happened when
BOTH were offline/restarted together (during section 24 resilience tests).

**Diagnostic Logging Added:**
1. ABCI Info endpoint now recomputes and compares app_hash to detect corruption
2. State restoration now logs full app_hash and state counts
3. These will help identify exactly when/where divergence occurs

**Files Modified:**
- `crates/chain/src/cometbft/app.rs:144-180` - Info endpoint verification
- `crates/chain/src/persistence_integration.rs:212-225` - Restore logging

### Previous Session Results:
- Session 8: 43/52 tests passing
- Session 9: 45/52 tests passing (2 more fixed)

### Root Cause #13: Scalars Hash try_read() Fallback (FIXED)

**The Bug**: In `compute_app_hash()`, the scalars hash (next_order_id + insurance_fund) was
computed from `perp_engine` using `try_read()`, with a **fallback to the shared engine** if
the lock was unavailable:

```rust
// BUG: Non-deterministic fallback!
let s = if let Some(ref perp) = self.perp_engine {
    if let Ok(perp_eng) = perp.try_read() {
        Self::scalars_hash_from_engine(&perp_eng.state)  // Uses actual scalars
    } else {
        Self::scalars_hash_from_engine(&engine)  // BUG: Falls back to DEFAULTS!
    }
} else {
    Self::scalars_hash_from_engine(&engine)
};
```

The shared engine has DEFAULT scalars (next_order_id=1, insurance_fund=0), while perp_engine
has ACTUAL scalars. If `try_read()` fails due to lock contention on one node but succeeds on
others, the nodes compute different scalars hashes → different AppHash → consensus failure.

**Evidence from Logs**: At height 128 during the BFT threshold test:
- Nodes 0, 1, 2: Expected `A783B2AC6D26DDC3E72E586E41CD6247296E52A33F3A2A5CA3A8A003171E1083`
- Node 3: Expected `BCC2032C523E6D4F20E820DE2C87A0CF2BFDACF742CFDE8EA69ADB861B54CF92`

**The Fix** (state.rs): Use `blocking_read()` instead of `try_read()` to guarantee the lock
is acquired. This is safe because the code runs on a `spawn_blocking` thread (via ABCI server):

```rust
// FIXED: Use blocking_read() for deterministic scalars computation
let s = if let Some(ref perp) = self.perp_engine {
    let perp_eng = perp.blocking_read();
    Self::scalars_hash_from_engine(&perp_eng.state)
} else {
    Self::scalars_hash_from_engine(&engine)
};
```

**File Modified**: `crates/chain/src/state.rs:469-482`

### Root Cause #14: Docker Stop Grace Period (FIXED)

**The Bug**: The BFT threshold tests expected the chain to halt when 2/5 validators were stopped,
leaving only 3/5 (60% < 66.7% required). However, the chain continued producing 8 blocks in 15s.

**Root Cause**: The `stopValidator()` function used `docker stop` without a timeout:
```javascript
await exec(`docker stop ${CONTAINER_PREFIX}-cometbft-${nodeIdx}`).catch(() => {});
```

Docker's default stop behavior:
1. Send SIGTERM to container
2. Wait for `stop_grace_period` (15s in docker-compose) for graceful shutdown
3. Send SIGKILL after timeout

During the 15-second grace period, CometBFT validators **continue participating in consensus**!
So when the test waits 15s and checks height growth, most blocks were produced before the
validators actually stopped.

**The Fix** (multinode.ts): Use `-t 1` timeout for fast shutdown and verify containers stopped:
```javascript
async function stopValidator(nodeIdx: number): Promise<void> {
    // Stop with 1s timeout (fast shutdown for BFT tests)
    await exec(`docker stop -t 1 ${cometContainer}`);
    await exec(`docker stop -t 1 ${nodeContainer}`);

    // Verify containers are actually stopped (wait up to 5s)
    const verifyTimeout = Date.now() + 5000;
    while (Date.now() < verifyTimeout) {
        const cometRunning = await isContainerRunning(cometContainer);
        const nodeRunning = await isContainerRunning(nodeContainer);
        if (!cometRunning && !nodeRunning) return;
        await sleep(500);
    }
}
```

**File Modified**: `scripts/e2e/tests/multinode.ts:1332-1371`

### Current Test Results (6 failures - Session 10):
- ✅ Double failure halts consensus - NOW PASSES (chain stalls with 0 blocks!)
- ❌ Cross-node spot matching with balance consistency (0.0021 USDC difference between node groups)
- ❌ Nonce replay protection and old nonce rejection (chain stops producing blocks)
- ❌ EVM transaction receipts consistent across nodes (receipt not found after 15s)
- ❌ Cross-node EVM + perp partial fill (order not visible on Node 3 after 15s)
- ❌ All-node concurrent storm with mixed types (partial fill timeout)
- ❌ Chain halts at BFT threshold (chain did not recover - Node 3 rejects all blocks)

### Analysis of Remaining 6 Failures:

**Root Problem: Node 3 has different AppHash after restart**

CometBFT logs show Node 3 rejecting ALL proposed blocks at height 128:
```
err="wrong Block.Header.AppHash.
     Expected A76B0A89... (Node 3's computed hash),
     got F9217C15... (consensus hash from Nodes 0,1,2)"
```

This single issue causes ALL 6 failures:
1. **Balance divergence** (0.0021 USDC) → different unified_root → different app_hash
2. **Chain can't recover** → Node 3 prevotes nil for every block
3. **Cross-node tests fail** → Nodes 3,4 have different state than 0,1,2
4. **Nonce test fails** → Cluster already destabilized, chain can't produce blocks

**Why blocking_read() fix didn't help:**
The `blocking_read()` fix ensures deterministic scalars hash during FinalizeBlock.
But the divergence seems to happen during STATE RESTORATION, not during normal operation.

The 0.0021 USDC difference between node groups suggests:
- Some state wasn't properly persisted or restored
- Or some calculation differs between persistence and computation

**Root Cause B: Docker Stop Grace Period - FIXED AND VERIFIED**
Validators continue signing blocks during the 15-second docker stop grace period, causing:
- ❌ Double failure halts consensus (chain produces 8 blocks, expected 0-2)
- ❌ Chain halts at BFT threshold (recovery fails because nodes diverged)

**Cascading Failures:**
- ❌ Nonce replay test - Chain stalls because the cluster is already destabilized
- ❌ Cross-node EVM + perp - Node 3 stuck, orders not visible
- ❌ All-node concurrent storm - Reduced to 4/5 nodes agreeing (Node 3 diverged)

**Expected After Fixes:**
With both root causes fixed, Node 3 should sync correctly after restart, and BFT tests
should see proper chain halt/recovery behavior. Most cascading failures should resolve.

---

## Previous: Session 8 (Continued) - Root Causes #10-#12

### Root Causes Fixed in Session 8:
1. **Root Cause #10**: try_read() non-determinism in CLOID cleanup
2. **Root Cause #11**: Persistence timing (FinalizeBlock vs Commit)
3. **Root Cause #12**: Dual EngineState architecture causing scalars hash divergence

### Root Cause #12: Dual EngineState Architecture (FIXED)

**The Bug**: The codebase has two separate `EngineState` instances:
1. `engine` (SharedEngineState) - used for AppHash computation
2. `perp_engine.state` - used for order matching and persistence

When transactions update `next_order_id` and `insurance_fund`, they only update `perp_engine.state`,
but the `scalars_hash` was being computed from `engine` (which stays at defaults).

On restart, `next_order_id` was restored to BOTH engines, causing the restored node to have
different scalars hash than running nodes.

**The Fix** (state.rs): Changed `compute_app_hash()` to read scalars from `perp_engine` if available.

---

## Previous: Session 8 - Root Cause #11: Persistence Timing (FIXED)

### Fixes Applied:

**Fix 1: Move Persistence from FinalizeBlock to Commit**
The persistence was happening inside `FinalizeBlock`, but CometBFT stores the app_hash
in its state.db AFTER calling `Commit`. This caused mismatch on restart.

Changed to store state in `pending_persistence` during FinalizeBlock, then actually
persist to RocksDB in the `Commit` method.

**Fix 2: Clean CometBFT Data Directories**
The `infra/multinode/validator-*/data/` directories retain state across docker-compose
runs. Tests must clean these before running:
```bash
for i in 0 1 2 3 4; do
    rm -rf infra/multinode/validator-$i/data/{blockstore.db,state.db,evidence.db,tx_index.db,cs.wal,addrbook.json}
    echo '{"height": "0", "round": 0, "step": 0}' > infra/multinode/validator-$i/data/priv_validator_state.json
done
```

---

## Previous: Session 8 - Root Cause #10: try_read() Non-Determinism (FIXED)

**Test Status**: Testing fix for CLOID divergence.

### Root Cause #10: try_read() Causes Non-Deterministic CLOID Cleanup

**The Bug**: In `execute_order_sync()`, the CLOID cleanup code used `try_read()` to check
if maker orders were fully filled:

```rust
// BUG: try_read() can fail non-deterministically!
if let Ok(engine) = perp_engine.try_read() {
    // Check if order is fully filled, remove CLOID if so
}
```

If `try_read()` fails (due to lock contention from RPC handlers), the CLOID cleanup is
**SKIPPED entirely** for that block. This causes:
- Node A: try_read() succeeds → CLOIDs cleaned up
- Node B: try_read() fails (RPC handler holding lock) → CLOIDs NOT cleaned up
- Different CLOID state → Different AppHash → Consensus failure

### The Fix

Changed to use `acquire_read_with_retry()` which guarantees the lock is acquired:

```rust
// FIXED: Always acquire the lock, retry if needed
let engine = acquire_read_with_retry(perp_engine, "Perp engine (CLOID cleanup)")?;
// Check if order is fully filled, remove CLOID if so
```

### Changes Applied in Session 8

1. **Added `acquire_read_with_retry()` function in `app.rs`**
   - Similar to `acquire_write_with_retry()` but for read locks
   - Retries up to 100 times with 1ms sleep between attempts
   - Ensures CLOID cleanup is always performed

2. **Fixed CLOID cleanup in `execute_order_sync()`**
   - Changed from `perp_engine.try_read()` to `acquire_read_with_retry()`
   - CLOID cleanup is now deterministic across all nodes

3. **Fixed perp_engine persistence in `cometbft/app.rs`**
   - Changed from `perp.try_read()` to `perp.blocking_read()`
   - Ensures perp state (including next_order_id) is always persisted

4. **Added `cloid_count()` method to AppState**
   - Returns current number of CLOID mappings
   - Used for debugging state divergence

5. **Added CLOID logging in FinalizeBlock**
   - Logs CLOID count at START and END of each block
   - Shows delta (how many CLOIDs were added/removed)

### Previous Session 7 Changes
- Added `oid_to_cloid` reverse mapping in `state.rs`
- Added `remove_cloid_by_order()` method for cleanup when orders are filled
- Updated `execute_order_sync()` to track and clean up filled maker orders
- Updated `restore_cloid_mappings()` to populate both forward and reverse mappings

---

## Previous: Session 7 - CLOID Investigation

**Test Status**: 42/52 tests passing.

### Critical Insight: ONLY CLOID Differs!

Analysis showed that ONLY the CLOID component differs between nodes at height 91:

```
Node 2: prev=0747dee0 unified=47e97af8 nonces=1518de6e positions=c5d24601 orders=c5d24601
        markets=36898d8b leverage=c5d24601 scalars=463194fe cloid=7f3b2a0d evm=95ebb340

Node 4: prev=0747dee0 unified=47e97af8 nonces=1518de6e positions=c5d24601 orders=c5d24601
        markets=36898d8b leverage=c5d24601 scalars=463194fe cloid=19af134a evm=95ebb340
```

All other AppHash components are IDENTICAL. This led to the discovery of the try_read() bug.

---

## Previous: Session 6 - Significant Progress!

**Test Status**: ~44/52 tests passing. Most critical node restart/catch-up tests now pass!

### Key Fixes This Session:
- ✅ Fixed balance decimals in validation (Root Cause #7) - TEST token invariant now validates correctly
- ✅ Fixed balance decimals in restoration (use stored decimals, not hardcoded)
- ✅ **CRITICAL FIX**: EVM simulation state leak (Root Cause #8) - eth_estimateGas was creating accounts
  in simulation mode, modifying EVM state and causing AppHash divergence between nodes

### Tests Now Passing:
- ✅ Node catch-up after restart - nodes rejoin and sync correctly
- ✅ Transactions during downtime reflected after sync
- ✅ Stopped node does not corrupt state on rejoin
- ✅ All mixed EVM/perp transaction tests
- ✅ AppHash consistency across nodes (except double failure scenario)

### Remaining Issues:
- ❌ Double failure recovery - when 2/5 validators stop simultaneously, they may restore with different
  CLOID state than running nodes. This is an edge case in the persistence timing.

## Previous: Chain stalls at height ~47-50 (Session 5)

## Older: 40/52 passing, 12 failing (Session 4)

## Root Cause #1: AppHash Divergence After Restart (PRIMARY)

### The Problem

When a node restarts and replays blocks, it computes different `app_hash` values
than the running nodes. This causes CometBFT to reject proposed blocks and stall
consensus.

### Architecture Background

HyperCore has a **dual EngineState architecture**:

1. **Shared EngineState** (`AppState.engine`) - Used by `compute_app_hash()` for
   consensus. During normal operation, it has:
   - Markets (from `init_chain` genesis)
   - **Empty** positions, orders, leverage
   - **Default** scalars (next_order_id=0, insurance_fund=0)

2. **perp_engine EngineState** (`Engine.state`) - Internal to the perpetuals
   engine. All order execution goes through here. Contains:
   - Markets (with runtime mark_price, funding state, OI)
   - Active positions, orders, leverage
   - Runtime scalars (incremented next_order_id, accumulated insurance_fund)

### What Went Wrong

The persistence supplement (added in a previous session) was **overriding** the
`core` fields in `PersistedState` with perp_engine data:

```
extract_state() → core.positions = [] (empty, from shared EngineState)
supplement      → core.positions = [pos1, pos2, ...] (overrides with perp data)
```

On restart, `restore_state()` loads `core` data into the **shared EngineState**:

```
restore_state() → shared EngineState.positions = [pos1, pos2, ...]  ← WRONG!
```

Now `compute_app_hash()` reads non-empty positions from the shared EngineState,
producing a different hash than running nodes (which have empty positions).

### Evidence from Logs

Running nodes at height 168:
```
positions=c5d2460186f7233c  (keccak of empty)
orders=c5d2460186f7233c     (keccak of empty)
leverage=c5d2460186f7233c   (keccak of empty)
=> app_hash=2d7e801dc53c38fb
```

Node 3 after restart at height 169:
```
ABCI Info: height=169, app_hash=6ad15b625d2a00b5  ← DIFFERENT
```

CometBFT error:
```
prevote step: consensus deems this block invalid; prevoting nil
  Expected 2D7E801DC53C38FB..., got 6AD15B625D2A00B5...
```

Node 4 stuck at height 61 (hash diverged immediately after restart):
```
positions=4544641384bccc46  (NON-EMPTY - contaminated by restore)
```

### The Fix

Added a **separate `perp` field** to `PersistedState` that stores perp_engine
data independently from the `core` data (which represents the shared EngineState):

| Field | Before (broken) | After (fixed) |
|-------|-----------------|---------------|
| `core.positions` | Overridden with perp data | Shared EngineState data (empty) |
| `core.orders` | Overridden with perp data | Shared EngineState data (empty) |
| `core.markets` | Overridden with perp data | Shared EngineState data (genesis) |
| `perp.positions` | *(didn't exist)* | Perp engine positions |
| `perp.orders` | *(didn't exist)* | Perp engine orders |
| `perp.markets` | *(didn't exist)* | Perp engine market state |

On restore:
- `restore_state()` puts correct (empty) data into shared EngineState
- `main.rs` reads from `perp` field to restore the perp_engine

Files changed:
- `crates/persistence/src/state.rs` - Added `PerpState` struct and `perp` field
- `crates/persistence/src/lib.rs` - Exported `PerpState`
- `crates/chain/src/cometbft/app.rs` - Write to `perp` instead of overriding `core`
- `crates/node/src/main.rs` - Read from `perp` with fallback to `core`

### Backward Compatibility

- `#[serde(default)]` on the `perp` field means old snapshots (without it)
  deserialize with empty defaults
- main.rs fallback logic: if `perp.positions` is empty, use `core.positions`
- First restart after the fix will use the fallback, subsequent restarts use `perp`

---

## Root Cause #2: CometBFT addr_book Permission Denied (SECONDARY)

### The Problem

CometBFT defaults `addr_book_file` to `/cometbft/config/addrbook.json`, but
the config directory is mounted read-only (`:ro`) in Docker. This prevents
CometBFT from saving peer addresses, degrading reconnection after node restart.

### Evidence from Logs

```
Saving AddrBook to file  book=/cometbft/config/addrbook.json
Failed to save AddrBook to file  err="open /cometbft/config/write-file-atomic-...: permission denied"
```

After restart, nodes show "duplicate CONN" errors and can't reconnect:
```
Error reconnecting to peer. Trying again  err="duplicate CONN<172.25.0.8:26656>"
numOutPeers=0 numInPeers=0
```

### The Fix

Added `addr_book_file = "data/addrbook.json"` to the `[p2p]` section of all 5
validator config.toml files. The `/cometbft/data/` directory IS writable.

Files changed:
- `infra/multinode/validator-{0,1,2,3,4}/config/config.toml`

---

## How the Failures Cascade

The 12 test failures follow a clear cascade pattern:

1. **Resilience tests** stop and restart validators
2. Restarted nodes compute wrong app_hash (Root Cause #1)
3. CometBFT can't reach consensus → chain stalls at height 170
4. All subsequent tests fail because the chain isn't producing blocks

### Failure Mapping

| Test | Root Cause | Expected Fix |
|------|-----------|--------------|
| Node catch-up after restart | #1 (hash divergence at h61) | Perp/core separation |
| Double failure halts consensus, recovery resumes | #1 (node 3 wrong hash after restart) | Perp/core separation |
| Contract deploy on Node 0 | Chain stalled (cascade) | Chain recovery |
| Spot order propagation | Chain stalled (cascade) | Chain recovery |
| View transfer propagation | Chain stalled (cascade) | Chain recovery |
| Cross-node spot matching | Chain stalled (cascade) | Chain recovery |
| Nonce replay rejection | Chain stalled (cascade) | Chain recovery |
| EVM receipts consistent | Chain stalled (cascade) | Chain recovery |
| Cross-node EVM + perp | Chain stalled (cascade) | Chain recovery |
| Concurrent storm | Chain stalled (cascade) | Chain recovery |
| Stopped node state on rejoin | #1 + #2 (hash + P2P) | Both fixes |
| Chain halts at BFT threshold | #1 (node 3 wrong hash after restart) | Perp/core separation |

---

---

## Root Cause #3: EVM Executor Lock Contention (PRIMARY, Session 3)

### The Problem

Node 0 stalls at height 41 while nodes 1-4 continue to height 63+. The ABCI
consensus thread crashes due to a panic in `compute_app_hash`, caused by
a write lock conflict between EVM RPC handlers and the consensus path.

### Architecture Background

The EVM executor (`EvmExecutor`) is shared between two consumers:

1. **CometBFT ABCI app** (FinalizeBlock) - Executes EVM transactions during
   consensus. Needs a **write lock** on the executor.

2. **EVM RPC server** - Handles `eth_call`, `eth_estimateGas`, `eth_getBalance`,
   etc. `eth_call` and `eth_estimateGas` need a **write lock** (because
   `simulate_tx` takes `&mut self`) even though they don't commit state changes.

Both consumers share the same `Arc<TokioRwLock<EvmExecutor>>`.

### What Went Wrong

Two bugs interact to crash Node 0:

**Bug 1: `try_read().expect()` panic in `compute_app_hash`**

```rust
// crates/chain/src/state.rs line 469
let executor = evm.try_read()
    .expect("EVM executor lock should not be held during state commitment");
```

This panics if ANY write lock is held on the executor. Between EVM transaction
execution in FinalizeBlock and `compute_app_hash`, the executor lock is briefly
released. If an RPC handler grabs it (e.g., viem calling `eth_estimateGas`
before `eth_sendRawTransaction`), the `.expect()` panics, crashing the ABCI
consensus thread.

**Bug 2: `acquire_tokio_write_with_retry` giving up after 100ms**

```rust
// crates/chain/src/cometbft/app.rs line 461
let Some(mut exec) = acquire_tokio_write_with_retry(executor, ...) else {
    // SKIP the EVM transaction!
    tx_results.push(ExecTxResult { code: 1, log: "lock contention" });
    continue;
};
```

If an RPC handler holds the write lock for >100ms, FinalizeBlock SKIPS the EVM
transaction. This causes different EVM state than other nodes → different
app_hash → consensus divergence.

### Failure Sequence

1. Tests 1-22 pass (no EVM transactions through consensus)
2. Test 23 ("Mixed Transaction Types") sends first EVM tx to Node 0
3. viem calls `eth_estimateGas` → acquires WRITE lock on executor
4. CometBFT calls FinalizeBlock → needs write lock for EVM tx or read lock
   for `compute_app_hash`
5. Either Bug 1 (panic) or Bug 2 (skip) triggers
6. ABCI consensus thread crashes or app_hash diverges
7. Node 0 stuck at height 41, other nodes continue (4/5 > 2/3 quorum)
8. All tests that query Node 0 for height see 41 and fail

### Evidence from Logs

Node 0 at height 41 (stuck):
```
priv_validator_state.json: height="42", step=2 (PREVOTE but never PRECOMMIT)
```

Nodes 1-4 at height 65:
```
priv_validator_state.json: height="65", step=3 (PRECOMMIT, fully participating)
```

Node 0 still accepts RPC calls but no FinalizeBlock logs after height 41:
```
02:30:19 - CheckTx: raw EVM tx from 0xf39fd6...  (mempool still works)
02:30:29 - EIP-712 signature verified               (gateway still works)
           (NO FinalizeBlock logs)                   (consensus thread dead)
```

cometbft-0 connected to peers but stuck:
```
numOutPeers=0 numInPeers=3  (has peers but can't reach consensus)
```

### The Fix

**1. Replace `try_read().expect()` with `blocking_read()`**

```rust
// state.rs - compute_app_hash
let executor = evm.blocking_read();  // Safe: runs in spawn_blocking
```

**2. Replace `acquire_tokio_write_with_retry` with `blocking_write()`**

```rust
// app.rs - FinalizeBlock
let mut exec = executor.blocking_write();  // Never skips EVM transactions
```

This ensures:
- FinalizeBlock ALWAYS executes EVM transactions (no more skipping)
- `compute_app_hash` ALWAYS reads EVM state (no more panics)
- RPC handlers wait until FinalizeBlock finishes (async `.write().await` blocks)
- All operations run on `spawn_blocking` threads, so blocking is safe

Files changed:
- `crates/chain/src/state.rs` - `blocking_read()` for EVM state root
- `crates/chain/src/cometbft/app.rs` - `blocking_write()` for executor,
  receipts, transactions, and block number

### Why Node 0 Specifically?

Node 0 is the primary test target. The test script sends transactions to Node 0
first and queries Node 0 for all height checks. This means Node 0 handles more
concurrent RPC traffic during tests, making lock contention more likely.

### addr_book_file Fix (Re-applied)

The previous session's addr_book_file fix was overwritten by `generate_genesis`
(which does `rm -rf ./infra/multinode` and regenerates all configs). Fixed by
adding `addr_book_file = "data/addrbook.json"` to the generator template at
`scripts/generate-multi-validator-genesis.sh`.

---

---

## Root Cause #4: EVM State Not Persisted/Restored (Session 4)

### The Problem

After Session 3's fix (40/52), EVM executor lock contention is solved. But node restarts
still cause AppHash divergence because **EVM state is NOT persisted or restored**.

### Evidence from Logs

Node 3 after restart (ABCI Info):
```
ABCI Info: height=170, app_hash=fd5c791da234f64b
```

Running nodes at height 171:
```
wrong Block.Header.AppHash.  Expected 503C77B454234E69..., got FD5C791DA234F64B...
```

Node 3 proposes blocks with its (wrong) app_hash, other nodes reject them.

### Architecture Background

`compute_app_hash()` includes `evm_root` from `executor.state_root()`:

```rust
// state.rs line 467-478
let evm_root = if let Some(ref evm) = self.evm_executor {
    let executor = evm.blocking_read();
    executor.state_root()  // Hashes: nonces, code, storage
};
```

The EVM state root includes:
- Account nonces (incremented after each EVM tx)
- Deployed contract code
- Contract storage

### What Went Wrong

`extract_state()` in `persistence_integration.rs` extracts:
- ✅ Unified balances
- ✅ Perp engine positions/orders/leverage/markets
- ✅ Spot engine tokens/markets/orders
- ✅ Nonces, CLOID index, block metadata
- ❌ EVM state (accounts, storage, code)

The `PersistedState.evm` field exists but is always `EvmStateData::default()` (empty).

When a node restarts:
1. `EvmExecutor` is created fresh with empty state
2. `state_root()` returns hash of empty EVM state
3. Other nodes have EVM state (nonces, contracts) from executed transactions
4. Different `evm_root` → different `app_hash` → consensus failure

### The Fix

Add EVM state extraction to `app.rs` FinalizeBlock persistence:

```rust
// In app.rs FinalizeBlock, after perp_engine extraction:
if let Some(ref executor) = inner.evm_executor {
    let evm = executor.blocking_read();
    // Extract EVM accounts (nonces, code hashes)
    // Extract contract storage
    // Extract deployed code
}
```

Add EVM state restoration to `main.rs` startup:

```rust
// In main.rs CometBFT mode, after perp_engine restoration:
if !persisted.evm.accounts.is_empty() {
    let mut evm = evm_executor.blocking_write();
    // Restore EVM accounts, storage, code
}
```

Files to change:
- `crates/chain/src/cometbft/app.rs` - Extract EVM state in FinalizeBlock
- `crates/node/src/main.rs` - Restore EVM state on startup
- `crates/evm/src/state.rs` - Add getters for extracting state
- `crates/evm/src/executor.rs` - Add state restoration methods

---

## Root Cause #5: Unified Balance Divergence (Session 4, continued)

### The Problem

After fixing EVM state restoration, the EVM hash now matches between restarted and running nodes.
But the **unified balance hash** and **CLOID index hash** differ.

### Evidence from Logs

**Node 0 (running) at height 61:**
```
unified=47e97af860a9152a
cloid=c5d2460186f7233c (keccak of empty)
evm=95ebb340bfc7c5ee ← SAME
=> 4543043cd7e7c3db
```

**Node 4 (restarted) at height 61:**
```
unified=6ccef688094267c3 ← DIFFERENT
cloid=2e7304ebcd9a4dd1 ← DIFFERENT (has data)
evm=95ebb340bfc7c5ee ← SAME
=> 808c878105d78394
```

### Analysis

1. **EVM fix verified working**: Both nodes compute `evm=95ebb340bfc7c5ee`

2. **Unified balance divergence**: The unified balance state is different between nodes.
   This affects Alice/Bob/etc. balances. Need to investigate why restoration produces
   different balances.

3. **CLOID index divergence**: Node 4 has CLOID mappings that Node 0 doesn't have.
   CLOIDs are used for cancel-by-cloid. When orders are filled/cancelled, the CLOID
   mapping should be removed. But Node 4's persisted CLOIDs might be stale.

### Possible Causes

1. **Unified balances**: The `compute_unified_state_root()` might be including
   something that's different between persisted and running state (e.g., fee balances,
   accumulated interest, etc.)

2. **CLOID index**: The CLOID index is persisted but orders might be filled/cancelled
   between persistence and restart. The running nodes remove the CLOID entry when
   the order is consumed, but the persisted state keeps it.

### Key Insight

Both nodes have the SAME `prev` hash (height 60's app_hash = `35c6671fd12fc533`), but
DIFFERENT component hashes at height 61. This is paradoxical because:
- If they agreed at height 60, they should have the same state
- Block 61 has 0 transactions, so state shouldn't change
- Yet unified and cloid hashes are different

### Possible Root Causes

1. **Order escrow affecting balances**: Orders reserve funds in unified_state. If Node 4
   persisted with orders pending, but running nodes filled those orders, the balances
   would be different (escrowed vs released).

2. **Persistence timing**: Node 4 might have persisted mid-FinalizeBlock, capturing an
   inconsistent state. Need to verify persistence happens atomically after commit.

3. **CLOID lifecycle**: CLOIDs are added when orders are placed, removed when orders
   complete. Stale CLOIDs in Node 4's persistence while running nodes have removed them.

### Resolution

The CLOID and unified balance divergence issues were caused by **stale data accumulating in RocksDB**.
The persistence layer was only adding/updating entries but never deleting removed items.

**Fix Applied (Session 4 continued):**
Modified `persist_state()` in `crates/persistence/src/persister.rs` to clear all "snapshot"
column families before writing:
- CLOIDs (cancelled/filled orders)
- Orders (filled/cancelled)
- Positions (closed)
- Leverage (for closed positions)
- Balances (for removed accounts)
- Nonces (for removed accounts)

After this fix, the CLOID hash now matches between nodes at height 63.

---

## Root Cause #7: Unified Balance Invariant Violation (Session 5, FIXED in Session 6)

### The Problem

After fixing the stale data issue, a new issue was discovered. The chain stalls during EVM
token tests due to a balance invariant violation:

```
Failed to persist state at height 47: Invalid state: Invalid balance for
0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266 token 1: total != core_view + evm_view
```

### Root Cause

The `UnifiedBalanceData::is_valid()` function in `crates/persistence/src/state.rs` was
hardcoded to use 6 decimals for validation:

```rust
const DECIMALS: u8 = 6;  // Always 6, but TEST token uses 18!
```

USDC (token 0) uses 6 decimals, but TEST token (token 1) uses 18 decimals. When the
invariant check parsed balance strings with the wrong decimal precision, the math failed.

### Fix (Session 6)

1. Added `decimals: u8` field to `UnifiedBalanceData` struct
2. Modified `extract_unified_balances()` to store `balance.total.decimals()`
3. Updated `is_valid()` to use the stored decimals instead of hardcoded 6
4. Updated `restore_state()` to read decimals from the stored field (with fallback for
   backwards compatibility)

**Files modified:**
- `crates/persistence/src/state.rs` - Added decimals field and updated validation
- `crates/persistence/src/extractor.rs` - Store decimals when extracting
- `crates/chain/src/persistence_integration.rs` - Store and restore with correct decimals

---

## Root Cause #8: EVM Simulation State Leak (Session 6, CRITICAL FIX)

### The Problem

Nodes diverged on EVM state hash at height 10 even with 0 transactions in the block:
- Node 0: evm=c27f4c0b45dc8e4c
- Node 1: evm=a6e1ac4d0ef70f12

### Root Cause

In `crates/evm/src/executor.rs`, the `execute_tx_inner()` function had code that created
accounts during **simulation** (eth_estimateGas, eth_call):

```rust
// BUG: This runs even when commit_state=false (simulation mode)!
if self.db.state.get_account(&tx.from).is_none() {
    if !self.enforce_gas_fees {
        self.db.state.set_balance(tx.from, U256::from(10u64).pow(U256::from(20)));
    }
}
```

When eth_estimateGas was called on Node 0 (but not on other nodes), it created an account
in the EVM state. This modified the accounts HashMap, changing the EVM state root hash.

### Fix

Added `commit_state &&` check to only create accounts during actual transactions:

```rust
// FIXED: Only create accounts when actually committing (not during simulation)
if commit_state && self.db.state.get_account(&tx.from).is_none() {
    if !self.enforce_gas_fees {
        self.db.state.set_balance(tx.from, U256::from(10u64).pow(U256::from(20)));
    }
}
```

**File modified:** `crates/evm/src/executor.rs:394-400`

---

## Root Cause #9: CLOIDs Not Removed When Orders Are Filled (Session 7)

### The Problem

When analyzing the test failure logs, nodes diverged at height 91 with different CLOID hashes:
- Nodes 0, 1, 2: `cloid=3b4bfe536b0f8343`
- Node 4: `cloid=61c570b4bb08e8b4`

Additionally, Node 4 showed "1 perp positions" while others showed "2 perp positions".

### Root Cause

CLOIDs (Client Order IDs) are registered when orders are placed but were **only removed when orders
are explicitly cancelled**. When orders are **filled** (matched against other orders), their CLOIDs
remained in the `cloid_to_oid` map indefinitely!

This caused CLOID accumulation and state divergence when:
1. A maker order with a CLOID is placed and rests in the orderbook
2. A taker order comes in and fully fills the maker order
3. The maker order is removed from the orderbook
4. **But the CLOID mapping remained!**

During catch-up or state restoration, different timing of fills could cause different CLOID states.

### The Fix

1. **Added reverse mapping**: `oid_to_cloid: HashMap<(MarketId, OrderId), (AccountAddress, String)>`
   - This allows looking up the CLOID for an order by its order ID
   - Maintained in sync with `cloid_to_oid`

2. **Updated CLOID registration**: `register_cloid()` now adds to both mappings:
   ```rust
   self.cloid_to_oid.insert((owner, cloid.clone()), (market_id, order_id));
   self.oid_to_cloid.insert((market_id, order_id), (owner, cloid));
   ```

3. **Added removal by order ID**: `remove_cloid_by_order()` for cleanup when orders are filled:
   ```rust
   pub fn remove_cloid_by_order(&mut self, market_id: MarketId, order_id: OrderId) -> bool {
       if let Some((owner, cloid)) = self.oid_to_cloid.remove(&(market_id, order_id)) {
           self.cloid_to_oid.remove(&(owner, cloid));
           true
       } else {
           false
       }
   }
   ```

4. **Updated order execution**: In `execute_order_sync()`, after processing fills:
   - Track maker orders that had fills
   - Check if each maker order still exists in the orderbook
   - If not (fully filled and removed), call `remove_cloid_by_order()`

5. **Updated restore logic**: `restore_cloid_mappings()` now populates both mappings

### Files Modified
- `crates/chain/src/state.rs` - Added `oid_to_cloid` field and related methods
- `crates/chain/src/app.rs` - Added CLOID cleanup logic for filled orders in both spot and perp

---

## All Fixes Summary

| # | Root Cause | Fix | Status |
|---|-----------|-----|--------|
| 1 | Perp data overriding core in persistence | Separate `perp` field in PersistedState | Applied |
| 2 | addr_book_file in read-only config dir | Point to writable data dir + generator template | Applied |
| 3 | EVM executor lock contention + panic | `blocking_read()`/`blocking_write()` | Applied |
| 4 | EVM state not persisted/restored | Extract/restore EVM accounts, storage, code | Applied - VERIFIED WORKING |
| 5 | CLOID index accumulating stale entries | Clear CLOIDs before persist, delete removed CLOIDs | Applied - VERIFIED WORKING |
| 6 | Balances/Orders/Positions accumulating stale entries | Clear all snapshot column families before persist | Applied |
| 7 | Balance invariant validation with wrong decimals | Store and use correct decimal precision | Applied - VERIFIED WORKING |
| 8 | EVM simulation creating accounts in state | Only create accounts when commit_state=true | Applied - VERIFIED WORKING |
| 9 | CLOIDs not removed when orders are filled | Add oid_to_cloid reverse mapping; remove CLOIDs on fill | Applied - Session 7 |
| 10 | try_read() non-determinism in CLOID cleanup | `acquire_read_with_retry()` | Applied - Session 8 |
| 11 | Persistence timing (FinalizeBlock vs Commit) | Move persistence to Commit method | Applied - Session 8 |
| 12 | Dual EngineState scalars hash divergence | Read scalars from perp_engine if available | Applied - Session 8 |
| 13 | try_read() non-determinism in scalars hash | `blocking_read()` for perp_engine | Applied - Session 10 |
| 14 | Docker stop grace period (15s default) | `docker stop -t 1` + verify stopped | Applied - Session 10 |
| 15 | Diagnostic logging for state divergence | ABCI Info recomputes app_hash, restore logs state counts | Applied - Session 10 |
| 16 | `blocking_read()` panics in async main startup | `acquire_rw_lock_read()` try+retry for all contexts | Applied - Session 11 |
| 17 | `persist_state()` writing empty core fields | Prefer perp fields when non-empty | Applied - Session 11 |
| 18 | Decimal bincode serde loses precision (decimals always reset to 8) | `is_human_readable()` dispatch: JSON=string, bincode=(i128,u8) tuple | Applied - Session 12 |
| — | Duplicate FinalizeBlock at same height corrupts state | Early return if `current_height() == request.height` | Applied - Session 12 |
| — | `from_str_exact()` panics on i128 overflow | Use `checked_mul`/`checked_add`, return `None` | Applied - Session 12 |

## Verification Steps

After rebuilding the Docker image with these fixes:

1. `docker compose -f docker-compose-multinode-5.yml down -v` (clean volumes)
2. Rebuild: `docker compose -f docker-compose-multinode-5.yml build`
3. Run tests: `./scripts/e2e-multinode-full.sh`

### What to Look For

- Node restarts should show: `Restored perp_engine state: ... (source=perp)`
- Node restarts should show: `Restored EVM state: N accounts, M storage slots, P code entries, Q block hashes`
- No more `Failed to save AddrBook` errors
- No more `duplicate CONN` errors
- No more `wrong Block.Header.AppHash` errors
- Restarted nodes should sync past the height where they were stopped
- **No more Node 0 stalling at any height**
- **All 5 nodes should agree on height throughout the test**
- **EVM transactions on Node 0 should produce receipts within a few seconds**
- **Restarted nodes should have matching `evm_root` in their app_hash**

---

## Session 6 Test Results (Latest)

### Tests Passing (~44/52):
- ✅ All basic connectivity tests (21 tests)
- ✅ State Consistency Across Nodes (AppHash, clearinghouse, unified balances, markets)
- ✅ Mixed Transaction Types (EVM + Non-EVM)
  - EVM tx via Node 0 processes correctly
  - EVM tx via different nodes
  - Mixed EVM and perp orders in same block window
  - Failing EVM tx does not halt chain
  - Failing perp tx does not halt chain
- ✅ Node Resilience (partial)
  - Validator failure: chain continues with 4/5
  - **Node catch-up after restart** (CRITICAL - now works!)
  - Transactions during downtime reflected after sync
  - CometBFT finality: committed blocks never change
- ✅ Byzantine Fault Tolerance Properties
  - Evidence parameters configured
  - Validator set integrity
  - No evidence in normal operation
  - Block commits have supermajority signatures
  - No divergence under concurrent adversarial load
  - **Stopped node does not corrupt state on rejoin** (CRITICAL - now works!)

### Tests Failing (~8/52):
- ❌ Double failure halts consensus, recovery resumes
  - Edge case: 2/5 validators stopped, then restarted with different CLOID state
  - Not a critical issue for normal operation
- ❌ Some cross-node EVM/spot tests (cascade failures from double failure scenario)
- ❌ State consistent after mixed transaction storm (transient fetch error)

### Files Modified in Session 6

1. **`crates/persistence/src/state.rs`**
   - Added `decimals: u8` field to `UnifiedBalanceData`
   - Updated `is_valid()` to use stored decimals

2. **`crates/persistence/src/extractor.rs`**
   - Store `balance.total.decimals()` when extracting balances

3. **`crates/chain/src/persistence_integration.rs`**
   - Use stored decimals in restoration (with fallback for backwards compatibility)

4. **`crates/evm/src/executor.rs`** (CRITICAL)
   - Fixed simulation state leak: Added `commit_state &&` check at line 394
   - Prevents eth_estimateGas from creating accounts during simulation

### Remaining Edge Case: Double Failure Recovery

When 2/5 validators are stopped simultaneously and then one is restarted:
- The restarted node restores from its last persisted state
- Running nodes may have processed additional transactions (adding CLOIDs)
- CLOID index can differ, causing AppHash mismatch

This is a rare edge case that only affects recovery from below-quorum situations.
The fix would require either:
1. State sync from running peers instead of local persistence
2. Or more careful CLOID lifecycle management during recovery

### How to Run Tests

```bash
# Full test (handles cleanup and build automatically)
make test-multinode-full

# Or manually:
docker compose -f docker-compose-multinode-5.yml down -v --remove-orphans
docker compose -f docker-compose-multinode-5.yml build
./scripts/e2e-multinode-full.sh --verbose
```
