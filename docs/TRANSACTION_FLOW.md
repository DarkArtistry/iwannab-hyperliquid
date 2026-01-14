# Transaction Flow Guide

This document explains how transactions flow through HyperCore, from client submission to execution and state updates.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Spot Order Flow](#spot-order-flow)
3. [EVM Transaction Flow](#evm-transaction-flow)
4. [View Transfer Flow](#view-transfer-flow)
5. [Balance Reserve System](#balance-reserve-system)
6. [Key Source Files](#key-source-files)

---

## Architecture Overview

HyperCore uses a **unified state model** where all balances are stored in a single master balance sheet with separate views for HyperCore trading and HyperEVM operations.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         HyperCore Node (Single Process)                  │
│                                                                          │
│   ┌─────────────────────────────────────────────────────────────────┐   │
│   │                        Shared Unified State                      │   │
│   │                     Arc<RwLock<UnifiedState>>                   │   │
│   │                                                                  │   │
│   │  ┌─────────────────────────────────────────────────────────┐   │   │
│   │  │  User 0xf39F...2266:                                     │   │   │
│   │  │  ┌──────────────────────────────────────────────────┐   │   │   │
│   │  │  │ Token: USDC (index 0)                             │   │   │   │
│   │  │  │   total:     100,000.000000                       │   │   │   │
│   │  │  │   core_view:  80,000.000000 ← SpotEngine reads    │   │   │   │
│   │  │  │   evm_view:   20,000.000000 ← EvmExecutor reads   │   │   │   │
│   │  │  └──────────────────────────────────────────────────┘   │   │   │
│   │  └─────────────────────────────────────────────────────────┘   │   │
│   └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│              ┌─────────────────────┼─────────────────────┐              │
│              │                     │                     │              │
│              ▼                     ▼                     ▼              │
│   ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐      │
│   │  Gateway API    │   │   SpotEngine    │   │  EvmExecutor    │      │
│   │  Port 3000      │   │   Orderbooks    │   │  Port 8545      │      │
│   │  /info,/exchange│   │   Matching      │   │  eth_*, web3_*  │      │
│   └─────────────────┘   └─────────────────┘   └─────────────────┘      │
└─────────────────────────────────────────────────────────────────────────┘

Source: crates/node/src/main.rs:121-165
```

**Key Invariant:** `total == core_view + evm_view` always holds.

---

## Spot Order Flow

### 1. Client Submits Order

```typescript
// TypeScript SDK
const result = await client.exchange.placeOrder({
  market: 'TEST-USDC',
  side: 'buy',
  price: '10.50',
  size: '5.0',
  orderType: 'limit',
});
```

### 2. Gateway Receives Request

**File:** `crates/gateway/src/handlers.rs`

```rust
// Simplified flow
pub async fn handle_exchange(
    state: AppState,
    request: ExchangeRequest,
) -> Result<ExchangeResponse> {
    // 1. Parse action
    let action = parse_action(&request.action)?;

    // 2. Verify signature (stub in Phase 1)
    verify_signature(&request.signature, &action)?;

    // 3. Route to appropriate handler
    match action {
        Action::SpotOrder(order) => handle_spot_order(state, order).await,
        Action::SpotCancel(cancel) => handle_spot_cancel(state, cancel).await,
        // ...
    }
}
```

### 3. SpotEngine Processes Order

**File:** `crates/engine/src/spot_engine.rs`

```rust
pub fn place_order(&mut self, order: SpotOrder, ...) -> Result<SpotOrderResult> {
    // 1. Validate order
    self.validate_order(&order)?;

    // 2. Calculate reserve amount
    let reserve = if order.is_buy {
        order.size * order.price  // Buy: reserve quote (USDC)
    } else {
        order.size  // Sell: reserve base token
    };

    // 3. Reserve balance (moves from available to reserved)
    self.state.reserve_balance(order.account, reserve_token, reserve)?;

    // 4. Match against orderbook
    let fills = self.match_order(&order)?;

    // 5. Process fills (transfer tokens between accounts)
    for fill in &fills {
        self.process_fill(fill)?;
    }

    // 6. Handle remaining order
    if !order.is_filled() && order.should_rest() {
        // Add to orderbook, keep reserve
        self.add_to_book(order);
    } else {
        // Release unused reserve
        self.state.release_balance(order.account, reserve_token, remaining_reserve);
    }

    Ok(SpotOrderResult { fills, order_id, ... })
}
```

### 4. Balance Updates via UnifiedState

**File:** `crates/primitives/src/unified_state.rs`

```rust
// On trade fill - transfer between accounts
pub fn transfer_core(
    &mut self,
    from: AccountAddress,
    to: AccountAddress,
    token: TokenIndex,
    amount: Decimal,
) -> bool {
    // Check sender has enough in core_view
    let from_balance = self.balances.get(&(from, token))?;
    if from_balance.core_view < amount {
        return false;
    }

    // Debit sender's core_view
    balance.total -= amount;
    balance.core_view -= amount;

    // Credit receiver's core_view
    to_balance.total += amount;
    to_balance.core_view += amount;

    true
}
```

### 5. Response Returned to Client

```json
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "oid": 12345678
          }
        }
      ]
    }
  }
}
```

---

## EVM Transaction Flow

### 1. Client Sends EVM Transaction

```typescript
// Using viem/ethers
const tx = await walletClient.sendTransaction({
  to: contractAddress,
  data: encodedFunctionCall,
  value: 0n,
});
```

### 2. EVM RPC Server Receives Request

**File:** `crates/evm/src/rpc.rs`

```rust
async fn eth_send_transaction(&self, tx: TransactionRequest) -> Result<H256> {
    // 1. Build transaction
    let tx = self.build_transaction(tx)?;

    // 2. Execute via EvmExecutor
    let executor = self.executor.write().await;
    let result = executor.execute(tx)?;

    // 3. Return transaction hash
    Ok(result.tx_hash)
}
```

### 3. EvmExecutor Executes Transaction

**File:** `crates/evm/src/executor.rs`

```rust
pub fn execute(&mut self, tx: Transaction) -> Result<ExecutionResult> {
    // 1. Check EVM view balance for gas
    let evm_balance = self.unified_state.read()?.get_evm_view(tx.from, USDC)?;
    if evm_balance < tx.gas_limit * tx.gas_price {
        return Err(InsufficientBalance);
    }

    // 2. Build revm environment
    let env = self.build_env(&tx);

    // 3. Execute bytecode
    let result = self.revm.transact(env)?;

    // 4. Handle precompile calls (if any)
    // Precompiles read from SpotEngine state

    // 5. Debit gas from EVM view
    let gas_used = result.gas_used;
    self.unified_state.write()?.debit_evm(tx.from, USDC, gas_cost)?;

    Ok(ExecutionResult { ... })
}
```

### 4. Precompile Reads State

**File:** `crates/evm/src/precompiles.rs`

```rust
// Example: Get spot balance via precompile 0x0806
pub fn spot_balance(input: &[u8], engine: &SpotEngine) -> Vec<u8> {
    let (user, token_index) = decode_input(input);

    // Read from SpotEngine (which reads from unified state core_view)
    let balance = engine.get_balance(user, token_index);

    encode_output(balance.available, balance.reserved)
}
```

---

## View Transfer Flow

View transfers move funds between HyperCore trading and HyperEVM without actual token transfers.

### Core → EVM Transfer

**File:** `crates/primitives/src/unified_state.rs:203`

```rust
pub fn transfer_to_evm_view(
    &mut self,
    user: AccountAddress,
    token: TokenIndex,
    amount: Decimal,
) -> Result<(), ViewTransferError> {
    let balance = self.balances.get_mut(&(user, token))?;

    // Check core_view has enough
    if balance.core_view < amount {
        return Err(InsufficientCoreView);
    }

    // Adjust views (total unchanged!)
    balance.core_view = balance.core_view - amount;
    balance.evm_view = balance.evm_view + amount;

    // Verify invariant
    debug_assert!(balance.total == balance.core_view + balance.evm_view);

    Ok(())
}
```

### EVM → Core Transfer

**File:** `crates/primitives/src/unified_state.rs:234`

```rust
pub fn transfer_to_core_view(
    &mut self,
    user: AccountAddress,
    token: TokenIndex,
    amount: Decimal,
) -> Result<(), ViewTransferError> {
    let balance = self.balances.get_mut(&(user, token))?;

    // Check evm_view has enough
    if balance.evm_view < amount {
        return Err(InsufficientEvmView);
    }

    // Adjust views (total unchanged!)
    balance.evm_view = balance.evm_view - amount;
    balance.core_view = balance.core_view + amount;

    // Verify invariant
    debug_assert!(balance.total == balance.core_view + balance.evm_view);

    Ok(())
}
```

**Why This Matters:**
- No bridge contracts needed
- No wrapped tokens
- Atomic and instant
- Total balance always matches sum of views

---

## Balance Reserve System

When orders are placed, funds are reserved to prevent double-spending.

### Balance Structure

**File:** `crates/engine/src/spot_engine.rs`

```rust
pub struct SpotBalance {
    /// Amount available for new orders
    pub available: Decimal,
    /// Amount reserved for open orders
    pub reserved: Decimal,
}

impl SpotBalance {
    pub fn total(&self) -> Decimal {
        self.available + self.reserved
    }
}
```

### Reserve Lifecycle

```
1. Order Placed:
   available: 1000 → 950
   reserved:     0 →  50
   (Reserved 50 USDC for buy order at $10 × 5 units)

2. Order Partially Fills (2 units):
   available: 950 → 950 (no change)
   reserved:   50 →  30 (released 20 for filled portion)
   + Received: 2 TEST tokens

3. Order Cancelled (remaining 3 units):
   available: 950 → 980
   reserved:   30 →   0 (released remaining reserve)
```

### Key Functions

```rust
// Reserve balance for order
pub fn reserve_balance(&mut self, user, token, amount) -> Result<()> {
    let balance = self.balances.get_mut(&(user, token))?;
    if balance.available < amount {
        return Err(InsufficientBalance);
    }
    balance.available -= amount;
    balance.reserved += amount;
    Ok(())
}

// Release reserve (on fill or cancel)
pub fn release_balance(&mut self, user, token, amount) {
    let balance = self.balances.get_mut(&(user, token))?;
    balance.reserved -= amount;
    balance.available += amount;
}
```

---

## Key Source Files

| Component | File | Key Functions |
|-----------|------|---------------|
| **Unified State** | `crates/primitives/src/unified_state.rs` | `UnifiedState`, `UnifiedBalance`, `transfer_to_evm_view()` |
| **Node Entry** | `crates/node/src/main.rs` | `main()`, creates shared unified state (line 121) |
| **Spot Engine** | `crates/engine/src/spot_engine.rs` | `place_order()`, `match_order()`, `SpotEngineState` |
| **Gateway Handlers** | `crates/gateway/src/handlers.rs` | `handle_exchange()`, `handle_spot_order()` |
| **EVM Executor** | `crates/evm/src/executor.rs` | `EvmExecutor::execute()`, `with_unified_state()` |
| **EVM RPC** | `crates/evm/src/rpc.rs` | `eth_sendTransaction()`, `eth_call()` |
| **Precompiles** | `crates/evm/src/precompiles.rs` | `spot_balance()`, `get_position()` |
| **Matching** | `crates/engine/src/matching.rs` | Order matching logic |
| **Risk Engine** | `crates/engine/src/risk.rs` | Margin calculations |

---

## Sequence Diagram Summary

```
┌────────┐     ┌─────────┐     ┌───────────┐     ┌──────────────┐
│ Client │     │ Gateway │     │SpotEngine │     │UnifiedState  │
└───┬────┘     └────┬────┘     └─────┬─────┘     └──────┬───────┘
    │               │                │                   │
    │ POST /exchange│                │                   │
    │──────────────►│                │                   │
    │               │ place_order()  │                   │
    │               │───────────────►│                   │
    │               │                │ reserve_balance() │
    │               │                │──────────────────►│
    │               │                │                   │ core_view -= amt
    │               │                │◄──────────────────│
    │               │                │                   │
    │               │                │ match_order()     │
    │               │                │ (find matching orders)
    │               │                │                   │
    │               │                │ process_fills()   │
    │               │                │──────────────────►│
    │               │                │                   │ transfer between accounts
    │               │                │◄──────────────────│
    │               │                │                   │
    │               │ OrderResult    │                   │
    │               │◄───────────────│                   │
    │ {status: ok}  │                │                   │
    │◄──────────────│                │                   │
```

---

## Related Documentation

- [ARCHITECTURE.md](./ARCHITECTURE.md) - System design and component overview
- [PROTOCOL.md](./PROTOCOL.md) - Protocol specification
- [IMPLEMENTATION_STATUS.md](./IMPLEMENTATION_STATUS.md) - What's implemented vs stubbed
- [API.md](./API.md) - API reference
