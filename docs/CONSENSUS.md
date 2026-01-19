# HyperCore Consensus Architecture

This document provides a comprehensive analysis of HyperCore's consensus mechanisms, state commitment verification, and implementation options for production-grade Byzantine Fault Tolerant (BFT) consensus.

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Current Architecture](#current-architecture)
3. [Consensus Libraries & Mechanisms](#consensus-libraries--mechanisms)
4. [The State Verification Problem](#the-state-verification-problem)
5. [Option A: Trust Determinism](#option-a-trust-determinism)
6. [Option B: ProcessProposal State Verification](#option-b-processproposal-state-verification)
7. [Option C: Post-Block Verification](#option-c-post-block-verification)
8. [Comparison Matrix](#comparison-matrix)
9. [Recommended Approach](#recommended-approach)
10. [Implementation Roadmap](#implementation-roadmap)

---

## Executive Summary

HyperCore implements a perpetual futures exchange with BFT consensus via CometBFT (formerly Tendermint). The system correctly computes Merkle state roots but **has a critical gap**: the AppHash does not commit to all consensus-critical state.

### Current State
- **Merkle Trees**: Properly implemented binary Merkle trees with keccak256 hashing
- **State Commitment**: ⚠️ **INCOMPLETE** - App hash includes only `unified_state_root + nonce_root`
- **Consensus**: CometBFT ABCI integration exists
- **Risk**: **Visible halt** if execution diverges (not silent fork - CometBFT validates AppHash)

### 🔴 Critical Issue: Incomplete AppHash

**The current AppHash computation is MISSING consensus-critical state:**

| State Component | Currently Committed? | Location |
|-----------------|---------------------|----------|
| Balances (core/evm views) | ✅ Yes | `unified_state_root` |
| Nonces | ✅ Yes | `nonce_root` |
| **Positions** | ❌ **NO** | `EngineState.positions` |
| **Open Orders** | ❌ **NO** | `EngineState.orders` + orderbooks |
| **Market State** | ❌ **NO** | `EngineState.markets` (mark/index price, funding) |
| **Leverage Settings** | ❌ **NO** | `EngineState.leverage` |
| **Insurance Fund** | ❌ **NO** | `EngineState.insurance_fund` |
| **Next Order ID** | ❌ **NO** | `EngineState.next_order_id` |
| **CLOID Mappings** | ❌ **NO** | `AppState.cloid_to_oid` |
| **EVM Accounts** | ❌ **NO** | `EvmState.accounts` (nonces, code_hash) |
| **EVM Storage** | ❌ **NO** | `EvmState.storage` |
| **EVM Code** | ❌ **NO** | `EvmState.code` |

**Impact**: Validators could diverge on positions, orders, or EVM state and the AppHash would not detect it, leading to **undetected state divergence** where queries return different results from different nodes.

### 🔴 Critical Issue: Determinism Bug

**File**: `crates/chain/src/state.rs:267-270`
```rust
let current_time_ms = std::time::SystemTime::now()  // ❌ NON-DETERMINISTIC!
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as u64;
```

This uses **wall clock time** for nonce validation. Different nodes have different clocks, causing potential divergence in transaction acceptance. **MUST use block timestamp instead.**

### Options Summary

| Option | Approach | Complexity | Security | Performance |
|--------|----------|------------|----------|-------------|
| **A** | Trust Determinism | Low | Medium | High |
| **B** | ProcessProposal Verification | High | High | Medium |
| **C** | Post-Block Verification | Medium | Medium-High | High |

---

## Current Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          HyperCore Node Architecture                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐     ┌─────────────────────────────────────────────────┐   │
│  │  CometBFT   │     │              HyperCore Application               │   │
│  │  (Consensus)│     │                                                  │   │
│  │             │     │  ┌─────────────────────────────────────────────┐│   │
│  │  - P2P      │◄────┤  │           ABCI Interface                    ││   │
│  │  - Mempool  │     │  │  - CheckTx (tx validation)                  ││   │
│  │  - Consensus│     │  │  - PrepareProposal (block building)         ││   │
│  │  - State    │     │  │  - ProcessProposal (block validation)       ││   │
│  │             │     │  │  - FinalizeBlock (execute + commit)         ││   │
│  └─────────────┘     │  └─────────────────────────────────────────────┘│   │
│        │             │                       │                          │   │
│        │             │  ┌────────────────────┴────────────────────┐    │   │
│        │             │  │                                         │    │   │
│        │             │  ▼                                         ▼    │   │
│        │             │  ┌─────────────┐              ┌─────────────┐   │   │
│        │             │  │ HyperCoreApp│              │   AppState  │   │   │
│        │             │  │             │◄────────────►│             │   │   │
│        │             │  │ - execute_tx│              │ - balances  │   │   │
│        │             │  │ - begin/end │              │ - nonces    │   │   │
│        │             │  │   _block    │              │ - positions │   │   │
│        │             │  └─────────────┘              │ - Merkle    │   │   │
│        │             │                               │   trees     │   │   │
│        │             │                               └─────────────┘   │   │
│        │             └─────────────────────────────────────────────────┘   │
│        │                                                                    │
│        ▼                                                                    │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         Block Storage                                │   │
│  │  Block N: [Header, Txs, app_hash, validator_signatures]             │   │
│  │  Block N+1: [Header, Txs, app_hash, validator_signatures]           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### State Commitment Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         State Commitment Pipeline                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. Transaction Execution                                                   │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  execute_tx(tx) → Modifies:                                      │    │
│     │    - unified_state (balances per token per address)              │    │
│     │    - nonces (per address)                                        │    │
│     │    - positions (per market per address)                          │    │
│     │    - orders (in orderbook)                                       │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                       │                                     │
│                                       ▼                                     │
│  2. End Block Processing                                                    │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  end_block() → Builds Merkle Trees:                              │    │
│     │                                                                  │    │
│     │    unified_state_tree = MerkleTree::from_entries(balances)       │    │
│     │    nonce_tree = MerkleTree::from_entries(nonces)                 │    │
│     │                                                                  │    │
│     │    Merkle Tree Structure:                                        │    │
│     │                    [root]                                        │    │
│     │                   /      \                                       │    │
│     │               [H01]      [H23]                                   │    │
│     │              /    \     /    \                                   │    │
│     │           [L0]  [L1] [L2]  [L3]                                  │    │
│     │                                                                  │    │
│     │    Leaf = keccak256(key || value)                                │    │
│     │    Node = keccak256(left || right)                               │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                       │                                     │
│                                       ▼                                     │
│  3. App Hash Computation                                                    │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  compute_app_hash() =                                            │    │
│     │    keccak256(                                                    │    │
│     │      height ||                     // 8 bytes                    │    │
│     │      timestamp ||                  // 8 bytes                    │    │
│     │      previous_app_hash ||          // 32 bytes                   │    │
│     │      unified_state_root ||         // 32 bytes (Merkle root)     │    │
│     │      nonce_root                    // 32 bytes (Merkle root)     │    │
│     │    )                                                             │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                       │                                     │
│                                       ▼                                     │
│  4. Block Commitment                                                        │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  Block Header includes:                                          │    │
│     │    - app_hash from ResponseFinalizeBlock                         │    │
│     │    - 2/3+ validator signatures on block                          │    │
│     │                                                                  │    │
│     │  ⚠️  Validators sign the BLOCK, not the STATE                    │    │
│     │      State agreement is ASSUMED via determinism                  │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Files

| File | Purpose |
|------|---------|
| `crates/chain/src/merkle.rs` | Binary Merkle tree implementation |
| `crates/chain/src/state.rs` | AppState with Merkle roots, proof generation |
| `crates/chain/src/app.rs` | HyperCoreApp with ABCI lifecycle |
| `crates/chain/src/cometbft/app.rs` | CometBFT ABCI interface |
| `crates/chain/src/block_producer.rs` | Standalone block production |

---

## Consensus Libraries & Mechanisms

### CometBFT (Tendermint Core)

**Library**: `tendermint-proto`, `tendermint-abci`
**Version**: CometBFT 0.38+ compatible

CometBFT is a Byzantine Fault Tolerant (BFT) consensus engine that:
- Handles peer-to-peer networking
- Manages mempool (transaction pool)
- Runs consensus algorithm (Tendermint BFT)
- Provides ABCI interface for application logic

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CometBFT Consensus Algorithm                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Tendermint BFT (PBFT-derived):                                            │
│                                                                             │
│  Round Structure:                                                           │
│  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐                     │
│  │ Propose │──►│Prevote  │──►│Precommit│──►│ Commit  │                     │
│  └─────────┘   └─────────┘   └─────────┘   └─────────┘                     │
│       │             │             │             │                           │
│       │             │             │             │                           │
│  Proposer      2/3+ votes    2/3+ votes    Block                           │
│  broadcasts    for block     for block    finalized                        │
│  block                                                                      │
│                                                                             │
│  Properties:                                                                │
│  - Safety: Never commits conflicting blocks (requires 2/3+ honest)         │
│  - Liveness: Always makes progress (requires 2/3+ online)                  │
│  - Instant finality: No forks once committed                               │
│                                                                             │
│  Fault Tolerance: f < n/3 (can tolerate up to 1/3 Byzantine validators)   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### ABCI 2.0 Interface

The Application Blockchain Interface (ABCI) separates consensus from application logic:

```rust
// crates/chain/src/cometbft/app.rs

trait Application {
    // Transaction validation (before mempool)
    fn check_tx(&self, request: RequestCheckTx) -> ResponseCheckTx;

    // Block proposal preparation (proposer only)
    fn prepare_proposal(&self, request: RequestPrepareProposal) -> ResponsePrepareProposal;

    // Block proposal validation (all validators)
    fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal;

    // Block execution and commitment
    fn finalize_block(&self, request: RequestFinalizeBlock) -> ResponseFinalizeBlock;

    // State persistence signal
    fn commit(&self) -> ResponseCommit;
}
```

### Current ABCI Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ABCI Message Flow                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         PROPOSER NODE                                 │  │
│  │                                                                       │  │
│  │  1. PrepareProposal(txs from mempool)                                │  │
│  │     └─► Returns ordered list of transactions                         │  │
│  │         (Current: just passes through, no reordering)                │  │
│  │                                                                       │  │
│  │  2. Broadcasts block proposal to network                             │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                              │                                              │
│                              ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                       ALL VALIDATORS                                  │  │
│  │                                                                       │  │
│  │  3. ProcessProposal(block)                                           │  │
│  │     └─► Validates transactions can be decoded                        │  │
│  │     └─► Runs check_tx on each transaction                            │  │
│  │     └─► Returns Accept/Reject                                        │  │
│  │     ❌ Does NOT execute block or verify state                        │  │
│  │                                                                       │  │
│  │  4. Vote on block (Prevote → Precommit)                              │  │
│  │                                                                       │  │
│  │  5. FinalizeBlock(block) - after 2/3+ precommits                     │  │
│  │     └─► begin_block(height, timestamp)                               │  │
│  │     └─► execute_tx(tx) for each transaction                          │  │
│  │     └─► end_block() - builds Merkle trees                            │  │
│  │     └─► Returns app_hash, tx_results, events                         │  │
│  │                                                                       │  │
│  │  6. Commit()                                                          │  │
│  │     └─► Persists state to disk                                       │  │
│  │     └─► app_hash included in next block header                       │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## The State Verification Problem

### The Gap

CometBFT consensus ensures all validators agree on:
- ✅ Block structure (header, transactions)
- ✅ Transaction ordering
- ✅ Block signatures

But it does **NOT** verify:
- ❌ That all validators computed the same state
- ❌ That the app_hash values match across validators
- ❌ That execution was deterministic

### Why This Matters

**Important Clarification**: CometBFT DOES validate AppHash in block headers. If validators compute different AppHash values, the network will **halt** (not silently fork) because validators will fail to agree on the next block.

However, there are two ways state divergence CAN go undetected:

1. **AppHash doesn't commit to all consensus-critical state** (OUR CURRENT BUG)
2. **Non-consensus state used for queries** (caches, indexes not in AppHash)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Undetected Divergence Scenario                           │
│                    (When AppHash is INCOMPLETE)                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Block N contains transaction: "Place order: Buy 1 BTC at $65,000"          │
│                                                                             │
│  Validator A (correct):           Validator B (buggy/different):           │
│  ┌─────────────────────────┐      ┌─────────────────────────┐              │
│  │ Order added to book     │      │ Order added to book     │              │
│  │ Order ID: 1001          │      │ Order ID: 1002  ← Bug!  │              │
│  │                         │      │                         │              │
│  │ Balances: same          │      │ Balances: same          │              │
│  │ Nonces: same            │      │ Nonces: same            │              │
│  │                         │      │                         │              │
│  │ app_hash: 0xAAA...      │      │ app_hash: 0xAAA...      │ ← SAME!      │
│  └─────────────────────────┘      └─────────────────────────┘              │
│                                                                             │
│  Problem: Orders are NOT in AppHash!                                        │
│  - Both validators compute identical app_hash                               │
│  - CometBFT sees agreement ✓                                                │
│  - Block commits successfully ✓                                             │
│  - But orderbooks have diverged                                             │
│                                                                             │
│  Result: UNDETECTED DIVERGENCE - queries return different orders            │
│          from different nodes. Matching may produce different fills.        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                    Detected Divergence Scenario                              │
│                    (When AppHash IS complete)                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  If AppHash commits to ALL state (balances, nonces, positions, orders...): │
│                                                                             │
│  Validator A:                     Validator B:                              │
│  ┌─────────────────────────┐      ┌─────────────────────────┐              │
│  │ app_hash: 0xAAA...      │      │ app_hash: 0xBBB...      │              │
│  └─────────────────────────┘      └─────────────────────────┘              │
│                                                                             │
│  CometBFT behavior:                                                         │
│  1. Block N+1 proposed with app_hash from Block N                          │
│  2. Validators compare app_hash in header vs their computed value          │
│  3. MISMATCH DETECTED → Validators reject block                            │
│  4. Network HALTS (no consensus on next block)                             │
│                                                                             │
│  Result: VISIBLE HALT - requires operator intervention                      │
│          Much better than silent divergence!                                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Sources of Non-Determinism

| Source | Example | Risk Level |
|--------|---------|------------|
| Floating point arithmetic | Price calculations | High |
| System time in execution | `SystemTime::now()` | High |
| Random number generation | Any `rand` usage | Critical |
| HashMap iteration order | Rust HashMap | Medium |
| Async race conditions | Concurrent state access | High |
| External API calls | Price oracles | Critical |
| Platform differences | f64 on different CPUs | Low |

---

## Option A: Trust Determinism

### Overview

The simplest approach: ensure all code is deterministic and trust that validators will compute the same state.

**Philosophy**: "If execution is deterministic, verification is unnecessary."

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Option A: Trust Determinism Architecture                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Determinism Guarantees                            │   │
│  │                                                                      │   │
│  │  Input Layer:                                                        │   │
│  │  ┌─────────────────────────────────────────────────────────────┐    │   │
│  │  │ - Same transactions                                          │    │   │
│  │  │ - Same order (from CometBFT)                                 │    │   │
│  │  │ - Same block metadata (height, timestamp from proposer)      │    │   │
│  │  └─────────────────────────────────────────────────────────────┘    │   │
│  │                              │                                       │   │
│  │                              ▼                                       │   │
│  │  Execution Layer:                                                    │   │
│  │  ┌─────────────────────────────────────────────────────────────┐    │   │
│  │  │ - Fixed-point arithmetic (Decimal type)                      │    │   │
│  │  │ - No floating point in state transitions                     │    │   │
│  │  │ - Sorted iteration (BTreeMap instead of HashMap)             │    │   │
│  │  │ - No system calls (time, random, network)                    │    │   │
│  │  │ - No async in state transitions                              │    │   │
│  │  └─────────────────────────────────────────────────────────────┘    │   │
│  │                              │                                       │   │
│  │                              ▼                                       │   │
│  │  Output Layer:                                                       │   │
│  │  ┌─────────────────────────────────────────────────────────────┐    │   │
│  │  │ - Same state changes                                         │    │   │
│  │  │ - Same Merkle roots                                          │    │   │
│  │  │ - Same app_hash                                              │    │   │
│  │  └─────────────────────────────────────────────────────────────┘    │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  Trust Model:                                                               │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                                                                      │   │
│  │  "All validators run the EXACT same code, producing EXACT same      │   │
│  │   state. If they don't, there's a bug that must be fixed."          │   │
│  │                                                                      │   │
│  │  Detection: Post-hoc via state queries, light client failures,      │   │
│  │             or explicit state comparison tools                       │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase A1: Audit for Non-Determinism (1-2 weeks)

**Goal**: Identify and eliminate all sources of non-determinism.

```bash
# 1. Audit all state-modifying code
grep -r "SystemTime\|Instant\|rand\|thread_rng\|HashMap" crates/

# 2. Check for floating point usage
grep -r "f32\|f64" crates/chain crates/engine crates/primitives

# 3. Verify iteration order
grep -r "HashMap\|HashSet" crates/chain crates/engine
```

**Tasks:**

| Task | File(s) | Action |
|------|---------|--------|
| Replace HashMap with BTreeMap | `crates/chain/src/state.rs` | Change iteration-sensitive maps |
| Audit Decimal arithmetic | `crates/primitives/src/decimal.rs` | Verify no precision loss |
| Remove system time in execution | `crates/chain/src/app.rs` | Use block timestamp only |
| Check async boundaries | `crates/chain/src/*.rs` | Ensure no races in state |

**Code Changes:**

```rust
// crates/chain/src/state.rs - BEFORE
use std::collections::HashMap;
pub struct AppState {
    nonces: HashMap<AccountAddress, u64>,  // Non-deterministic iteration!
}

// crates/chain/src/state.rs - AFTER
use std::collections::BTreeMap;
pub struct AppState {
    nonces: BTreeMap<AccountAddress, u64>,  // Deterministic iteration
}
```

#### Phase A2: Determinism Test Suite (1 week)

**Goal**: Create tests that verify deterministic execution.

```rust
// crates/chain/src/determinism_tests.rs

#[cfg(test)]
mod determinism_tests {
    /// Execute the same transactions twice and verify identical state
    #[test]
    fn test_execution_is_deterministic() {
        let genesis = create_test_genesis();
        let transactions = create_test_transactions();

        // First execution
        let mut app1 = HyperCoreApp::new();
        app1.init_from_genesis(&genesis);
        for tx in &transactions {
            app1.execute_tx(tx, 1000);
        }
        let hash1 = app1.commit();

        // Second execution (fresh app)
        let mut app2 = HyperCoreApp::new();
        app2.init_from_genesis(&genesis);
        for tx in &transactions {
            app2.execute_tx(tx, 1000);
        }
        let hash2 = app2.commit();

        assert_eq!(hash1, hash2, "Execution must be deterministic");
    }

    /// Run on multiple threads to detect race conditions
    #[test]
    fn test_parallel_execution_determinism() {
        let results: Vec<[u8; 32]> = (0..10)
            .into_par_iter()
            .map(|_| {
                let mut app = create_app_with_state();
                execute_standard_block(&mut app);
                app.commit()
            })
            .collect();

        assert!(results.windows(2).all(|w| w[0] == w[1]));
    }
}
```

#### Phase A3: CI/CD Determinism Checks (Ongoing)

```yaml
# .github/workflows/determinism.yml
name: Determinism Check

on: [push, pull_request]

jobs:
  determinism:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Check for HashMap in state code
        run: |
          if grep -r "HashMap" crates/chain/src/state.rs; then
            echo "ERROR: HashMap found in state.rs - use BTreeMap"
            exit 1
          fi

      - name: Check for floating point
        run: |
          if grep -r "f32\|f64" crates/chain crates/engine; then
            echo "WARNING: Floating point found - verify not used in consensus"
          fi

      - name: Run determinism tests
        run: cargo test determinism --release
```

### Pros & Cons

| Pros | Cons |
|------|------|
| No code changes to consensus | Silent forks possible |
| Best performance (no overhead) | Requires perfect discipline |
| Simplest implementation | Bug = catastrophic failure |
| Used by most Cosmos chains | No runtime detection |

### Risk Mitigation

1. **Code Review Policy**: All state-modifying code requires determinism review
2. **Static Analysis**: CI checks for known non-determinism patterns
3. **Fuzzing**: Property-based tests for determinism
4. **Monitoring**: Alert on state query discrepancies between nodes

---

## Option B: ProcessProposal State Verification

### Overview

Verify state agreement BEFORE committing by executing the block in ProcessProposal and comparing the resulting state hash.

**Philosophy**: "Don't commit to a block unless we know all validators agree on the resulting state."

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│              Option B: ProcessProposal State Verification                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Modified ABCI Flow:                                                        │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         PROPOSER                                      │  │
│  │                                                                       │  │
│  │  PrepareProposal:                                                     │  │
│  │  1. Select transactions from mempool                                  │  │
│  │  2. Execute transactions speculatively                                │  │
│  │  3. Compute expected_app_hash                                         │  │
│  │  4. Include expected_app_hash in proposal                             │  │
│  │                                                                       │  │
│  │  Block Proposal = {                                                   │  │
│  │    transactions: [...],                                               │  │
│  │    expected_app_hash: 0x123...,  // NEW: proposer's computed hash    │  │
│  │  }                                                                    │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                              │                                              │
│                              ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                       VALIDATORS                                      │  │
│  │                                                                       │  │
│  │  ProcessProposal:                                                     │  │
│  │  1. Execute all transactions (speculative, don't commit)             │  │
│  │  2. Compute local_app_hash                                           │  │
│  │  3. Compare: local_app_hash == proposal.expected_app_hash?           │  │
│  │                                                                       │  │
│  │     ┌─────────────────────────────────────────────────────┐          │  │
│  │     │  if local_hash == expected_hash:                    │          │  │
│  │     │      return Accept  // Vote for this block          │          │  │
│  │     │  else:                                              │          │  │
│  │     │      log("State mismatch! Local: {}, Expected: {}") │          │  │
│  │     │      return Reject  // Don't vote for this block    │          │  │
│  │     └─────────────────────────────────────────────────────┘          │  │
│  │                                                                       │  │
│  │  FinalizeBlock (only if 2/3+ Accept):                                │  │
│  │  1. Re-execute transactions (or use cached result)                   │  │
│  │  2. Commit state                                                      │  │
│  │  3. Return app_hash (should match expected)                          │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  Safety Guarantee:                                                          │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                                                                      │   │
│  │  A block is only committed if 2/3+ validators computed the SAME     │   │
│  │  app_hash as the proposer. State divergence = block rejection.      │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase B1: Protocol Extension (2-3 weeks)

**Goal**: Extend the block proposal to include expected state hash.

**Challenge**: CometBFT's standard block format doesn't include app_hash in the proposal.

**Solutions:**

1. **Encode in transaction list** (Hacky but works)
   ```rust
   // Add a "state commitment" pseudo-transaction at the end
   let state_tx = Transaction::StateCommitment {
       expected_hash: computed_hash
   };
   proposal.txs.push(serde_json::to_vec(&state_tx));
   ```

2. **Use proposal metadata** (If CometBFT version supports)
   ```rust
   ResponsePrepareProposal {
       txs: ordered_txs,
       // CometBFT 0.38+ may support custom metadata
   }
   ```

3. **Fork CometBFT** (Full control, high maintenance)

**Recommended: Option 1 (Pseudo-transaction)**

```rust
// crates/chain/src/tx.rs - Add new transaction type

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Order { ... },
    Cancel { ... },
    // ... existing types ...

    /// State commitment marker (proposer only)
    /// Must be last transaction in block
    #[serde(rename = "stateCommitment")]
    StateCommitment {
        expected_hash: [u8; 32],
        height: u64,
    },
}
```

#### Phase B2: PrepareProposal Implementation (1-2 weeks)

```rust
// crates/chain/src/cometbft/app.rs

fn prepare_proposal(&self, request: RequestPrepareProposal) -> ResponsePrepareProposal {
    let mut inner = self.inner.write();
    let timestamp = extract_timestamp(&request);

    // 1. Filter and order transactions
    let mut txs: Vec<Vec<u8>> = request.txs.into_iter()
        .filter(|tx_bytes| {
            // Validate transaction
            if let Ok(tx) = serde_json::from_slice::<Transaction>(tx_bytes) {
                inner.app.check_tx(&tx).is_ok()
            } else {
                false
            }
        })
        .take(inner.config.max_txs_per_block)
        .collect();

    // 2. Execute speculatively to compute expected hash
    let expected_hash = {
        // Clone state for speculative execution
        let mut speculative_app = inner.app.clone();
        speculative_app.begin_block(request.height as u64, timestamp);

        for tx_bytes in &txs {
            if let Ok(tx) = serde_json::from_slice::<Transaction>(tx_bytes) {
                let _ = speculative_app.execute_tx(&tx, timestamp);
            }
        }

        speculative_app.end_block();
        speculative_app.commit()
    };

    // 3. Add state commitment transaction
    let commitment_tx = Transaction {
        action: TransactionType::StateCommitment {
            expected_hash,
            height: request.height as u64,
        },
        nonce: 0,
        signature: Signature::zero(), // No signature needed
        hash: None,
    };
    txs.push(serde_json::to_vec(&commitment_tx).unwrap());

    ResponsePrepareProposal { txs }
}
```

#### Phase B3: ProcessProposal Verification (1-2 weeks)

```rust
// crates/chain/src/cometbft/app.rs

fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
    let inner = self.inner.read();
    let timestamp = extract_timestamp(&request);

    // 1. Extract state commitment (must be last tx)
    let (regular_txs, expected_hash) = match extract_state_commitment(&request.txs) {
        Ok((txs, hash)) => (txs, hash),
        Err(e) => {
            tracing::warn!("Invalid proposal: no state commitment - {}", e);
            return ResponseProcessProposal {
                status: ProposalStatus::Reject as i32,
            };
        }
    };

    // 2. Validate all regular transactions
    let mut parsed_txs = Vec::new();
    for tx_bytes in &regular_txs {
        let tx: Transaction = match serde_json::from_slice(tx_bytes) {
            Ok(tx) => tx,
            Err(_) => {
                return ResponseProcessProposal {
                    status: ProposalStatus::Reject as i32,
                };
            }
        };

        if inner.app.check_tx(&tx).is_err() {
            return ResponseProcessProposal {
                status: ProposalStatus::Reject as i32,
            };
        }
        parsed_txs.push(tx);
    }

    // 3. Execute speculatively and compute local hash
    let local_hash = {
        let mut speculative_app = inner.app.clone();
        speculative_app.begin_block(request.height as u64, timestamp);

        for tx in &parsed_txs {
            // Execute but don't fail proposal on tx failure
            // (failed txs still affect state via nonce increment)
            let _ = speculative_app.execute_tx(tx, timestamp);
        }

        speculative_app.end_block();
        speculative_app.commit()
    };

    // 4. Compare hashes
    if local_hash != expected_hash {
        tracing::error!(
            "STATE MISMATCH! Local: 0x{}, Expected: 0x{} at height {}",
            hex::encode(local_hash),
            hex::encode(expected_hash),
            request.height
        );

        // Emit alert metric
        metrics::counter!("consensus.state_mismatch").increment(1);

        return ResponseProcessProposal {
            status: ProposalStatus::Reject as i32,
        };
    }

    tracing::debug!("State verification passed for height {}", request.height);

    ResponseProcessProposal {
        status: ProposalStatus::Accept as i32,
    }
}

fn extract_state_commitment(txs: &[Vec<u8>]) -> Result<(Vec<Vec<u8>>, [u8; 32]), &'static str> {
    if txs.is_empty() {
        return Err("Empty transaction list");
    }

    // Last transaction should be state commitment
    let last_tx: Transaction = serde_json::from_slice(txs.last().unwrap())
        .map_err(|_| "Cannot decode last transaction")?;

    match last_tx.action {
        TransactionType::StateCommitment { expected_hash, .. } => {
            let regular_txs = txs[..txs.len()-1].to_vec();
            Ok((regular_txs, expected_hash))
        }
        _ => Err("Last transaction is not a state commitment"),
    }
}
```

#### Phase B4: Caching for Performance (1 week)

To avoid executing transactions twice (ProcessProposal + FinalizeBlock):

```rust
// crates/chain/src/cometbft/app.rs

struct CometBFTInner {
    app: HyperCoreApp,
    validators: ValidatorSet,
    chain_id: String,

    // Cache speculative execution result
    speculative_cache: Option<SpeculativeResult>,
}

struct SpeculativeResult {
    height: u64,
    app_hash: [u8; 32],
    tx_results: Vec<TxResult>,
    events: Vec<Event>,
}

fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
    // ... validation and speculative execution ...

    // Cache result for FinalizeBlock
    inner.speculative_cache = Some(SpeculativeResult {
        height: request.height as u64,
        app_hash: local_hash,
        tx_results,
        events,
    });

    ResponseProcessProposal { status: Accept }
}

fn finalize_block(&self, request: RequestFinalizeBlock) -> ResponseFinalizeBlock {
    let mut inner = self.inner.write();

    // Check if we have cached result from ProcessProposal
    if let Some(cached) = inner.speculative_cache.take() {
        if cached.height == request.height as u64 {
            // Use cached result, just commit
            inner.app.apply_cached_result(&cached);

            return ResponseFinalizeBlock {
                app_hash: cached.app_hash.to_vec().into(),
                tx_results: cached.tx_results,
                events: cached.events,
                ..Default::default()
            };
        }
    }

    // Fallback: re-execute (should be rare)
    // ... normal execution ...
}
```

### Pros & Cons

| Pros | Cons |
|------|------|
| Prevents silent forks | ~2x execution cost (mitigated by caching) |
| Immediate detection of divergence | Requires protocol modification |
| Strong safety guarantee | More complex implementation |
| Block rejected before commit | May reject valid blocks if one node is buggy |

### Risk Mitigation

1. **Caching**: Cache ProcessProposal results to avoid re-execution
2. **Timeout handling**: Ensure speculative execution has time limits
3. **Graceful degradation**: If ProcessProposal times out, accept (fallback to Option A)
4. **Testing**: Extensive testing of the caching mechanism

---

## Option C: Post-Block Verification

### Overview

Detect state divergence AFTER blocks are committed by having validators share and compare their computed state hashes.

**Philosophy**: "Commit optimistically, but verify immediately. Halt if divergence detected."

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                 Option C: Post-Block Verification                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Standard ABCI Flow (unchanged):                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │  PrepareProposal → ProcessProposal → FinalizeBlock → Commit          │  │
│  │        │                 │                │              │           │  │
│  │        │                 │                │              ▼           │  │
│  │        │                 │                │      ┌──────────────┐    │  │
│  │        │                 │                │      │ app_hash     │    │  │
│  │  (no changes)     (basic validation)   (execute) │ computed     │    │  │
│  │                                                  └──────────────┘    │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                       │                                     │
│                                       ▼                                     │
│  Post-Commit Verification Layer (NEW):                                      │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                                                                       │  │
│  │  After Commit:                                                        │  │
│  │  ┌─────────────────────────────────────────────────────────────┐     │  │
│  │  │  1. Validator broadcasts StateAttestation:                   │     │  │
│  │  │     {                                                        │     │  │
│  │  │       height: 1000,                                          │     │  │
│  │  │       app_hash: 0xABC...,                                    │     │  │
│  │  │       validator: 0x123...,                                   │     │  │
│  │  │       signature: 0xDEF...,                                   │     │  │
│  │  │     }                                                        │     │  │
│  │  └─────────────────────────────────────────────────────────────┘     │  │
│  │                              │                                        │  │
│  │                              ▼                                        │  │
│  │  ┌─────────────────────────────────────────────────────────────┐     │  │
│  │  │  2. Collect attestations from other validators               │     │  │
│  │  │                                                              │     │  │
│  │  │  Validator A: height=1000, hash=0xABC...                     │     │  │
│  │  │  Validator B: height=1000, hash=0xABC...                     │     │  │
│  │  │  Validator C: height=1000, hash=0xABC...                     │     │  │
│  │  │  Validator D: height=1000, hash=0xXYZ...  ← MISMATCH!        │     │  │
│  │  └─────────────────────────────────────────────────────────────┘     │  │
│  │                              │                                        │  │
│  │                              ▼                                        │  │
│  │  ┌─────────────────────────────────────────────────────────────┐     │  │
│  │  │  3. On mismatch detection:                                   │     │  │
│  │  │                                                              │     │  │
│  │  │     if local_hash != majority_hash:                          │     │  │
│  │  │         HALT("State divergence detected!")                   │     │  │
│  │  │         // Prevents further damage                           │     │  │
│  │  │         // Manual investigation required                     │     │  │
│  │  │                                                              │     │  │
│  │  │     else:                                                    │     │  │
│  │  │         log("Validator D has diverged, excluding...")        │     │  │
│  │  │         // Minority validator is buggy                       │     │  │
│  │  └─────────────────────────────────────────────────────────────┘     │  │
│  │                                                                       │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  Detection Window:                                                          │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                                                                      │   │
│  │  Block N committed → Attestations collected → Mismatch detected     │   │
│  │       │                      │                      │                │   │
│  │       │                      │                      ▼                │   │
│  │       │                      │              Block N+1 halted         │   │
│  │       │                      │              (max 1 block of damage)  │   │
│  │       ▼                      ▼                                       │   │
│  │    ~100ms              ~500ms-1s                                     │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase C1: State Attestation Protocol (2 weeks)

**Goal**: Define and implement the attestation message format and signing.

```rust
// crates/chain/src/attestation.rs

use serde::{Deserialize, Serialize};

/// State attestation broadcasted after each block commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateAttestation {
    /// Block height this attestation is for
    pub height: u64,

    /// The app_hash computed by this validator
    pub app_hash: [u8; 32],

    /// Validator's public key (for identification)
    pub validator_pubkey: Vec<u8>,

    /// Signature over (height || app_hash)
    pub signature: Vec<u8>,

    /// Timestamp when attestation was created
    pub timestamp: u64,
}

impl StateAttestation {
    /// Create and sign a new attestation
    pub fn new(
        height: u64,
        app_hash: [u8; 32],
        validator_key: &ed25519_dalek::SigningKey,
    ) -> Self {
        let pubkey = validator_key.verifying_key().to_bytes().to_vec();

        // Sign height || app_hash
        let mut message = Vec::with_capacity(40);
        message.extend_from_slice(&height.to_le_bytes());
        message.extend_from_slice(&app_hash);

        let signature = validator_key.sign(&message).to_bytes().to_vec();

        Self {
            height,
            app_hash,
            validator_pubkey: pubkey,
            signature,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    /// Verify the attestation signature
    pub fn verify(&self) -> bool {
        use ed25519_dalek::{Verifier, VerifyingKey, Signature};

        let pubkey = match VerifyingKey::from_bytes(
            self.validator_pubkey.as_slice().try_into().unwrap_or(&[0u8; 32])
        ) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let signature = match Signature::from_bytes(
            self.signature.as_slice().try_into().unwrap_or(&[0u8; 64])
        ) {
            Ok(sig) => sig,
            Err(_) => return false,
        };

        let mut message = Vec::with_capacity(40);
        message.extend_from_slice(&self.height.to_le_bytes());
        message.extend_from_slice(&self.app_hash);

        pubkey.verify(&message, &signature).is_ok()
    }
}
```

#### Phase C2: Attestation Collector (2 weeks)

```rust
// crates/chain/src/attestation_collector.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AttestationCollector {
    /// Attestations received per height
    attestations: Arc<RwLock<HashMap<u64, Vec<StateAttestation>>>>,

    /// Known validator public keys and their voting power
    validators: Arc<RwLock<HashMap<Vec<u8>, u64>>>,

    /// Our own validator key
    our_key: Option<ed25519_dalek::SigningKey>,

    /// Channel to signal divergence
    divergence_tx: tokio::sync::mpsc::Sender<DivergenceAlert>,

    /// Configuration
    config: AttestationConfig,
}

#[derive(Debug, Clone)]
pub struct AttestationConfig {
    /// How many blocks to keep attestations for
    pub retention_blocks: u64,

    /// Timeout waiting for attestations (ms)
    pub collection_timeout_ms: u64,

    /// Minimum voting power needed for quorum
    pub quorum_threshold: f64, // e.g., 0.67 for 2/3
}

#[derive(Debug)]
pub struct DivergenceAlert {
    pub height: u64,
    pub our_hash: [u8; 32],
    pub majority_hash: Option<[u8; 32]>,
    pub attestations: Vec<StateAttestation>,
}

impl AttestationCollector {
    /// Process a received attestation
    pub async fn process_attestation(&self, attestation: StateAttestation) {
        // 1. Verify signature
        if !attestation.verify() {
            tracing::warn!("Invalid attestation signature");
            return;
        }

        // 2. Verify validator is known
        let validators = self.validators.read().await;
        if !validators.contains_key(&attestation.validator_pubkey) {
            tracing::warn!("Attestation from unknown validator");
            return;
        }
        drop(validators);

        // 3. Store attestation
        let mut attestations = self.attestations.write().await;
        attestations
            .entry(attestation.height)
            .or_insert_with(Vec::new)
            .push(attestation.clone());

        // 4. Check for quorum and divergence
        self.check_divergence(attestation.height).await;
    }

    /// Check if we have enough attestations to detect divergence
    async fn check_divergence(&self, height: u64) {
        let attestations = self.attestations.read().await;
        let validators = self.validators.read().await;

        let height_attestations = match attestations.get(&height) {
            Some(a) => a,
            None => return,
        };

        // Calculate voting power by hash
        let mut hash_votes: HashMap<[u8; 32], u64> = HashMap::new();
        let mut total_voted: u64 = 0;

        for att in height_attestations {
            if let Some(&power) = validators.get(&att.validator_pubkey) {
                *hash_votes.entry(att.app_hash).or_insert(0) += power;
                total_voted += power;
            }
        }

        // Check if we have quorum
        let total_power: u64 = validators.values().sum();
        if (total_voted as f64 / total_power as f64) < self.config.quorum_threshold {
            return; // Not enough votes yet
        }

        // Find majority hash
        let majority_hash = hash_votes
            .iter()
            .max_by_key(|(_, &power)| power)
            .map(|(hash, _)| *hash);

        // Check if there's divergence
        if hash_votes.len() > 1 {
            // Multiple different hashes - DIVERGENCE!
            tracing::error!(
                "STATE DIVERGENCE DETECTED at height {}! {} different hashes",
                height,
                hash_votes.len()
            );

            // Get our hash
            let our_hash = height_attestations
                .iter()
                .find(|a| {
                    self.our_key.as_ref()
                        .map(|k| a.validator_pubkey == k.verifying_key().to_bytes().to_vec())
                        .unwrap_or(false)
                })
                .map(|a| a.app_hash);

            if let Some(our_hash) = our_hash {
                let alert = DivergenceAlert {
                    height,
                    our_hash,
                    majority_hash,
                    attestations: height_attestations.clone(),
                };

                let _ = self.divergence_tx.send(alert).await;
            }
        }
    }

    /// Broadcast our attestation after committing a block
    pub async fn broadcast_attestation(&self, height: u64, app_hash: [u8; 32]) {
        let Some(ref key) = self.our_key else {
            return; // Not a validator
        };

        let attestation = StateAttestation::new(height, app_hash, key);

        // Broadcast via P2P (implementation depends on network layer)
        // This could be:
        // 1. CometBFT's broadcast mechanism
        // 2. Separate gossip protocol
        // 3. Direct validator connections

        self.broadcast_to_peers(&attestation).await;
    }

    async fn broadcast_to_peers(&self, attestation: &StateAttestation) {
        // TODO: Implement based on network layer
        // For CometBFT, could use custom reactor or separate gossip
    }
}
```

#### Phase C3: Divergence Handler (1 week)

```rust
// crates/chain/src/divergence_handler.rs

use std::sync::atomic::{AtomicBool, Ordering};

pub struct DivergenceHandler {
    /// Flag to halt the node
    halted: AtomicBool,

    /// Alert receiver
    alert_rx: tokio::sync::mpsc::Receiver<DivergenceAlert>,

    /// Configuration
    config: DivergenceConfig,
}

#[derive(Debug, Clone)]
pub struct DivergenceConfig {
    /// Action to take on divergence
    pub action: DivergenceAction,

    /// External alerting endpoint
    pub alert_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DivergenceAction {
    /// Log and continue (dangerous, for testing only)
    LogOnly,

    /// Halt the node immediately
    Halt,

    /// Halt and attempt to sync with majority
    HaltAndResync,
}

impl DivergenceHandler {
    pub async fn run(mut self) {
        while let Some(alert) = self.alert_rx.recv().await {
            self.handle_alert(alert).await;
        }
    }

    async fn handle_alert(&self, alert: DivergenceAlert) {
        tracing::error!(
            "DIVERGENCE ALERT: height={}, our_hash={:?}, majority={:?}",
            alert.height,
            hex::encode(alert.our_hash),
            alert.majority_hash.map(|h| hex::encode(h))
        );

        // Send external alert
        if let Some(ref endpoint) = self.config.alert_endpoint {
            self.send_external_alert(endpoint, &alert).await;
        }

        // Determine if we're the minority
        let we_are_minority = alert.majority_hash
            .map(|mh| mh != alert.our_hash)
            .unwrap_or(false);

        match self.config.action {
            DivergenceAction::LogOnly => {
                tracing::warn!("Divergence logged but node continues (DANGEROUS)");
            }

            DivergenceAction::Halt => {
                tracing::error!("HALTING NODE due to state divergence");
                self.halted.store(true, Ordering::SeqCst);

                // Write divergence info to disk for debugging
                self.save_divergence_debug_info(&alert).await;

                // In production, this would trigger process exit
                std::process::exit(1);
            }

            DivergenceAction::HaltAndResync => {
                if we_are_minority {
                    tracing::error!("We are in minority - halting for resync");
                    self.halted.store(true, Ordering::SeqCst);

                    // Trigger state sync from majority
                    // This requires state sync implementation
                    self.trigger_state_sync(alert.height - 1).await;
                } else {
                    tracing::warn!("We are in majority - continuing but flagging minority validators");
                }
            }
        }
    }

    async fn send_external_alert(&self, endpoint: &str, alert: &DivergenceAlert) {
        let payload = serde_json::json!({
            "type": "state_divergence",
            "height": alert.height,
            "our_hash": hex::encode(alert.our_hash),
            "majority_hash": alert.majority_hash.map(|h| hex::encode(h)),
            "attestation_count": alert.attestations.len(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        if let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
        {
            let _ = client.post(endpoint)
                .json(&payload)
                .send()
                .await;
        }
    }

    async fn save_divergence_debug_info(&self, alert: &DivergenceAlert) {
        let debug_path = format!("/tmp/hypercore_divergence_{}.json", alert.height);

        let debug_info = serde_json::json!({
            "height": alert.height,
            "our_hash": hex::encode(alert.our_hash),
            "majority_hash": alert.majority_hash.map(|h| hex::encode(h)),
            "attestations": alert.attestations.iter().map(|a| {
                serde_json::json!({
                    "validator": hex::encode(&a.validator_pubkey),
                    "hash": hex::encode(a.app_hash),
                    "timestamp": a.timestamp,
                })
            }).collect::<Vec<_>>(),
        });

        if let Ok(json) = serde_json::to_string_pretty(&debug_info) {
            let _ = tokio::fs::write(&debug_path, json).await;
            tracing::error!("Divergence debug info saved to {}", debug_path);
        }
    }

    async fn trigger_state_sync(&self, target_height: u64) {
        // This would integrate with CometBFT state sync
        // For now, just halt
        tracing::error!("State sync from height {} required (not implemented)", target_height);
    }
}
```

#### Phase C4: P2P Integration (2 weeks)

Integrate attestation gossip with CometBFT or implement separate gossip layer.

**Option C4a: Use CometBFT's Reactor (Recommended)**

```rust
// crates/chain/src/cometbft/attestation_reactor.rs

use tendermint_proto::p2p::PacketMsg;

/// Custom reactor for attestation gossip
pub struct AttestationReactor {
    collector: Arc<AttestationCollector>,
    peers: Arc<RwLock<HashMap<PeerId, PeerState>>>,
}

impl AttestationReactor {
    /// Handle incoming P2P message
    pub async fn receive(&self, peer_id: PeerId, msg: PacketMsg) {
        if let Some(attestation) = self.decode_attestation(&msg) {
            self.collector.process_attestation(attestation).await;
        }
    }

    /// Broadcast to all peers
    pub async fn broadcast(&self, attestation: &StateAttestation) {
        let msg = self.encode_attestation(attestation);
        let peers = self.peers.read().await;

        for (peer_id, state) in peers.iter() {
            if state.supports_attestations {
                self.send_to_peer(*peer_id, msg.clone()).await;
            }
        }
    }
}
```

**Option C4b: Separate LibP2P Gossip**

```rust
// crates/chain/src/attestation_gossip.rs

use libp2p::{gossipsub, PeerId, Swarm};

pub struct AttestationGossip {
    swarm: Swarm<gossipsub::Behaviour>,
    topic: gossipsub::IdentTopic,
}

impl AttestationGossip {
    pub fn new(keypair: &libp2p::identity::Keypair) -> Self {
        let topic = gossipsub::IdentTopic::new("hypercore/attestations/1");

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_millis(500))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .expect("Valid config");

        let mut behaviour = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        ).expect("Gossipsub");

        behaviour.subscribe(&topic).expect("Subscribe");

        // ... setup swarm ...

        Self { swarm, topic }
    }

    pub async fn broadcast(&mut self, attestation: &StateAttestation) {
        let data = serde_json::to_vec(attestation).unwrap();
        self.swarm
            .behaviour_mut()
            .publish(self.topic.clone(), data)
            .expect("Publish");
    }
}
```

### Pros & Cons

| Pros | Cons |
|------|------|
| No modification to consensus protocol | 1 block of potential damage |
| Good performance (no double execution) | Requires additional P2P layer |
| Clear detection and alerting | More complex overall system |
| Can identify faulty validators | Doesn't prevent, only detects |

### Risk Mitigation

1. **Fast detection**: Optimize attestation gossip for <1 second latency
2. **Immediate halt**: Stop producing blocks on divergence
3. **Alerting**: External monitoring for divergence events
4. **State snapshots**: Keep recent snapshots for recovery

---

## Comparison Matrix

| Aspect | Option A | Option B | Option C |
|--------|----------|----------|----------|
| **Implementation Complexity** | Low | High | Medium |
| **Performance Impact** | None | Medium (cached) | Low |
| **Security Guarantee** | Medium | High | Medium-High |
| **Detection Speed** | Post-hoc | Pre-commit | 1 block delay |
| **Protocol Changes** | None | Moderate | Minor (P2P only) |
| **Failure Mode** | Silent fork | Block rejection | Node halt |
| **Recovery** | Manual | Automatic (try next proposer) | State sync |
| **Production Readiness** | Ready | 4-6 weeks | 3-4 weeks |

### Decision Matrix

| Criteria | Weight | Option A | Option B | Option C |
|----------|--------|----------|----------|----------|
| Security | 30% | 6/10 | 10/10 | 8/10 |
| Performance | 25% | 10/10 | 7/10 | 9/10 |
| Simplicity | 20% | 10/10 | 4/10 | 6/10 |
| Time to Implement | 15% | 10/10 | 4/10 | 6/10 |
| Maintainability | 10% | 8/10 | 5/10 | 7/10 |
| **Weighted Score** | 100% | **8.2** | **6.5** | **7.5** |

---

## Recommended Approach

### Phased Implementation

We recommend a **hybrid approach** that starts with Option A and adds Option C:

```
Phase 1 (Week 1-2): Option A - Trust Determinism
├── Audit codebase for non-determinism
├── Replace HashMap with BTreeMap in state
├── Add determinism test suite
└── CI/CD checks

Phase 2 (Week 3-6): Option C - Post-Block Verification
├── Implement attestation protocol
├── Add attestation collector
├── Implement divergence handler
└── P2P integration

Phase 3 (Future): Consider Option B
├── If silent forks occur in production
├── Or if regulatory requirements demand it
└── Implement ProcessProposal verification
```

### Rationale

1. **Option A is necessary regardless**: Any consensus system needs deterministic execution
2. **Option C adds safety net**: Detects issues that slip through Option A
3. **Option B deferred**: High complexity, marginal benefit over A+C
4. **Progressive hardening**: Start simple, add safety layers as needed

---

## Implementation Roadmap

### 🔴 P0: Fix AppHash Completeness (BEFORE DEVNET)

**This must be done BEFORE running distributed testnet/devnet.**

#### P0.1: Define Consensus-Critical State (1 day)

All state that affects:
- Transaction execution results
- Query responses
- Future transaction validity

**Full list for HyperCore:**

```rust
// crates/chain/src/state.rs - UPDATED compute_app_hash()

pub fn compute_app_hash(&self) -> [u8; 32] {
    let mut hasher = Keccak256::new();

    // Chain metadata
    hasher.update(&self.height.to_le_bytes());
    hasher.update(&self.timestamp.to_le_bytes());
    hasher.update(&self.app_hash);  // Previous hash

    // ===== EXISTING (keep) =====
    hasher.update(&self.compute_unified_state_root());  // Balances
    hasher.update(&self.compute_nonce_root());          // Nonces

    // ===== NEW: Add missing consensus-critical state =====

    // 1. Positions - critical for liquidation, funding, PnL
    hasher.update(&self.compute_positions_root());

    // 2. Orders - critical for matching, cancellation
    hasher.update(&self.compute_orders_root());

    // 3. Market state - mark price, funding indices, OI
    hasher.update(&self.compute_markets_root());

    // 4. Leverage settings - affects margin calculations
    hasher.update(&self.compute_leverage_root());

    // 5. Engine counters - next_order_id, insurance_fund
    hasher.update(&self.compute_engine_state_root());

    // 6. CLOID mappings - for CancelByCloid
    hasher.update(&self.compute_cloid_root());

    // 7. EVM state - accounts, storage, code
    hasher.update(&self.compute_evm_state_root());

    hasher.finalize().into()
}
```

#### P0.2: Implement Missing Merkle Trees (1-2 weeks)

```rust
// crates/chain/src/state.rs - New methods

/// Merkle root of all positions (account -> market -> position)
fn compute_positions_root(&self) -> [u8; 32] {
    // Get positions from perp engine
    let perp_engine = self.perp_engine.as_ref();
    // Build sorted entries and compute Merkle root
    // Key: account (20) || market_id (1)
    // Value: size (16) || entry_price (16) || cumulative_funding (16)
}

/// Merkle root of all open orders
fn compute_orders_root(&self) -> [u8; 32] {
    // Get orders from perp engine
    // Key: market_id (1) || order_id (8)
    // Value: owner (20) || price (16) || size (16) || side (1) || timestamp (8)
}

/// Merkle root of market state
fn compute_markets_root(&self) -> [u8; 32] {
    // Key: market_id (1)
    // Value: mark_price (16) || index_price (16) || funding_rate (16) ||
    //        open_interest (16) || next_funding_time (8)
}

/// Merkle root of EVM state
fn compute_evm_state_root(&self) -> [u8; 32] {
    // Must include:
    // - Account nonces (not in unified state)
    // - Contract code hashes
    // - Contract storage (all slots)

    // This is complex - consider using existing MPT libraries
    // or a simpler sorted-hash approach for MVP
}
```

#### P0.3: Fix SystemTime::now() Bug (1 day)

```rust
// crates/chain/src/state.rs - BEFORE (line 267-270)
let current_time_ms = std::time::SystemTime::now()  // ❌ BAD
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as u64;

// AFTER - Use block timestamp
pub fn validate_nonce(&self, address: &AccountAddress, nonce: u64, block_timestamp: u64) -> bool {
    const TIMESTAMP_THRESHOLD: u64 = 1_000_000_000_000;

    if nonce > TIMESTAMP_THRESHOLD {
        // Timestamp-based nonce validation
        let one_hour_ms: u64 = 3_600_000;
        let is_recent = nonce > block_timestamp.saturating_sub(one_hour_ms)
                     && nonce < block_timestamp.saturating_add(one_hour_ms);

        if !is_recent {
            return false;
        }

        let last_timestamp = self.last_timestamp_nonces.get(address).copied().unwrap_or(0);
        nonce > last_timestamp
    } else {
        // Sequential nonce validation
        let expected = self.get_nonce(address);
        nonce >= expected && nonce < expected + 100
    }
}
```

### 🔴 P0: Determinism Audit (Before Devnet)

#### P0.4: HashMap → BTreeMap Audit (2-3 days)

```bash
# Find all HashMap usage in state-modifying code
grep -rn "HashMap" crates/chain/src/ crates/engine/src/ | grep -v "test"

# Files to audit:
# - crates/chain/src/state.rs (nonces, block_hashes, block_metadata, cloid_to_oid)
# - crates/engine/src/state.rs (accounts, positions, leverage, markets, orders, etc.)
# - crates/evm/src/state.rs (accounts, storage, code)
```

**Change all iteration-sensitive HashMaps to BTreeMap:**

```rust
// BEFORE
pub struct AppState {
    nonces: HashMap<AccountAddress, u64>,
    // ...
}

// AFTER
pub struct AppState {
    nonces: BTreeMap<AccountAddress, u64>,
    // ...
}
```

### 🟡 P1: State Sync Snapshots (After Devnet Works)

For production multi-node, implement ABCI state sync:

```rust
// crates/chain/src/cometbft/app.rs

fn list_snapshots(&self, _req: RequestListSnapshots) -> ResponseListSnapshots {
    // Return available state snapshots at various heights
}

fn offer_snapshot(&self, req: RequestOfferSnapshot) -> ResponseOfferSnapshot {
    // Validate offered snapshot (check hash, format)
}

fn load_snapshot_chunk(&self, req: RequestLoadSnapshotChunk) -> ResponseLoadSnapshotChunk {
    // Return chunk of snapshot data
}

fn apply_snapshot_chunk(&self, req: RequestApplySnapshotChunk) -> ResponseApplySnapshotChunk {
    // Apply received chunk to local state
}
```

---

### Week 1-2: Determinism Audit (Option A)

```
Tasks:
□ Audit all HashMap usage in state-modifying code
□ Replace with BTreeMap where iteration order matters
□ Audit floating point usage
□ Remove system time from execution path
□ Create determinism test suite
□ Add CI checks for non-determinism patterns

Deliverables:
- PR: "feat(chain): ensure deterministic execution"
- Test suite: determinism_tests.rs
- CI workflow: determinism.yml
```

### Week 3-4: Attestation Protocol (Option C)

```
Tasks:
□ Implement StateAttestation struct
□ Implement signing/verification
□ Create AttestationCollector
□ Add storage for attestations
□ Implement quorum detection

Deliverables:
- crates/chain/src/attestation.rs
- crates/chain/src/attestation_collector.rs
- Unit tests for attestation
```

### Week 5-6: Divergence Handling (Option C)

```
Tasks:
□ Implement DivergenceHandler
□ Add halt mechanism
□ External alerting integration
□ Debug info capture
□ P2P gossip integration

Deliverables:
- crates/chain/src/divergence_handler.rs
- crates/chain/src/attestation_gossip.rs
- Integration tests
- Monitoring dashboard
```

### Week 7+: Testing & Hardening

```
Tasks:
□ End-to-end testing with multiple validators
□ Chaos testing (introduce non-determinism)
□ Performance benchmarking
□ Documentation
□ Runbooks for divergence scenarios

Deliverables:
- E2E test suite for consensus
- Performance benchmarks
- Operational runbooks
```

---

## Appendix A: Determinism Checklist

```
□ No HashMap/HashSet iteration in state-modifying code
□ No floating point (f32/f64) in state calculations
□ No SystemTime::now() in execution path
□ No random number generation
□ No external network calls during execution
□ No async races in state modification
□ Sorted iteration for all collections
□ Fixed-point arithmetic (Decimal type)
□ Deterministic serialization (sorted keys)
□ Platform-independent number encoding
```

## Appendix B: Monitoring Metrics

```rust
// Metrics to add for consensus monitoring

// Option A
counter!("consensus.blocks_produced").increment(1);
histogram!("consensus.block_time_ms").record(duration);

// Option C
counter!("consensus.attestations_sent").increment(1);
counter!("consensus.attestations_received").increment(1);
gauge!("consensus.attestation_quorum").set(quorum_percentage);
counter!("consensus.divergence_detected").increment(1);
histogram!("consensus.attestation_latency_ms").record(latency);
```

## Appendix C: Configuration

```toml
# config/consensus.toml

[determinism]
# Enable strict determinism checks (Option A)
strict_mode = true
# Fail on HashMap detection
fail_on_hashmap = true

[attestation]
# Enable post-block attestation (Option C)
enabled = true
# Retention period for attestations
retention_blocks = 100
# Timeout for collecting attestations
collection_timeout_ms = 2000
# Quorum threshold
quorum_threshold = 0.67

[divergence]
# Action on divergence: "log_only", "halt", "halt_and_resync"
action = "halt"
# External alerting endpoint
alert_endpoint = "https://alerts.example.com/divergence"
```

---

*Document Version: 1.0*
*Last Updated: January 2026*
*Authors: HyperCore Development Team*
