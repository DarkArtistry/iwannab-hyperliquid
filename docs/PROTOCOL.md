# Protocol Specification

## Overview

HyperCore is a perpetual futures exchange protocol with the following key properties:
- Central Limit Order Book (CLOB) matching
- Cross-margin (MVP) / Isolated margin (future)
- 8-hour funding intervals
- Partial liquidations with insurance fund backstop
- **Unified state model** with view-based balance separation (Phase 2A - COMPLETE)

## Implementation Status

| Feature | Status | Key Source File |
|---------|--------|-----------------|
| Perpetual Orders | ✅ Complete | `crates/engine/src/matching.rs` |
| Spot Trading | ✅ Complete | `crates/engine/src/spot_engine.rs` |
| Margin Calculation | ✅ Complete | `crates/engine/src/risk.rs` |
| Funding Rate | ✅ Complete | `crates/engine/src/funding.rs` |
| Unified State | ✅ **Complete** | `crates/primitives/src/unified_state.rs` |
| EVM Integration | ✅ Complete | `crates/evm/src/executor.rs` |
| Consensus (CometBFT) | ✅ **Complete** | `crates/chain/src/cometbft/` |
| Persistence (RocksDB) | ✅ **Complete** | `crates/persistence/` |
| State Save/Restore | ✅ **Complete** | `crates/persistence/src/persister.rs` |

---

## Transaction Flow

This section explains how transactions flow through the system from client to execution.

### Gateway Transaction Flow (HyperCore API)

```
Client                 Gateway                SpotEngine              UnifiedState
  │                      │                       │                        │
  │  POST /exchange      │                       │                        │
  │  {action: order}     │                       │                        │
  ├─────────────────────►│                       │                        │
  │                      │  verify signature     │                        │
  │                      │  (stub - Phase 3)     │                        │
  │                      │                       │                        │
  │                      │  spot_engine.place_order()                     │
  │                      ├──────────────────────►│                        │
  │                      │                       │  reserve_balance()     │
  │                      │                       ├───────────────────────►│
  │                      │                       │  (core_view -= amount) │
  │                      │                       │◄───────────────────────┤
  │                      │                       │                        │
  │                      │                       │  match_order()         │
  │                      │                       │  (orderbook matching)  │
  │                      │                       │                        │
  │                      │                       │  if fills: transfer()  │
  │                      │                       ├───────────────────────►│
  │                      │                       │◄───────────────────────┤
  │                      │                       │                        │
  │                      │  OrderResult          │                        │
  │                      │◄──────────────────────┤                        │
  │  {status: "ok"}      │                       │                        │
  │◄─────────────────────┤                       │                        │
```

**Key Source Files:**
- Request handling: `crates/gateway/src/handlers.rs:handle_exchange()`
- Order placement: `crates/engine/src/spot_engine.rs:place_order()`
- Balance reservation: `crates/engine/src/spot_engine.rs:SpotEngineState::reserve_balance()`
- Order matching: `crates/engine/src/spot_engine.rs:match_order()`

### EVM Transaction Flow (EVM RPC)

```
Client                 EVM RPC               EvmExecutor             UnifiedState
  │                      │                       │                        │
  │  eth_sendTransaction │                       │                        │
  │  (to: contract)      │                       │                        │
  ├─────────────────────►│                       │                        │
  │                      │  executor.execute()   │                        │
  │                      ├──────────────────────►│                        │
  │                      │                       │  get_evm_view()        │
  │                      │                       ├───────────────────────►│
  │                      │                       │  (check balance)       │
  │                      │                       │◄───────────────────────┤
  │                      │                       │                        │
  │                      │                       │  revm.transact()       │
  │                      │                       │  (execute bytecode)    │
  │                      │                       │                        │
  │                      │                       │  if precompile:        │
  │                      │                       │  read_from_spot_engine │
  │                      │                       │                        │
  │                      │                       │  debit_evm() for gas   │
  │                      │                       ├───────────────────────►│
  │                      │                       │◄───────────────────────┤
  │                      │                       │                        │
  │                      │  TransactionReceipt   │                        │
  │                      │◄──────────────────────┤                        │
  │  {hash: "0x..."}     │                       │                        │
  │◄─────────────────────┤                       │                        │
```

**Key Source Files:**
- RPC handling: `crates/evm/src/rpc.rs`
- Transaction execution: `crates/evm/src/executor.rs:EvmExecutor::execute()`
- Precompile reads: `crates/evm/src/precompiles.rs`
- EVM state: `crates/evm/src/state.rs`

### View Transfer Flow (Core ↔ EVM)

```
Client                 Gateway              UnifiedState
  │                      │                       │
  │  POST /exchange      │                       │
  │  {action: transfer   │                       │
  │   to_evm: true}      │                       │
  ├─────────────────────►│                       │
  │                      │  transfer_to_evm_view()                      │
  │                      ├──────────────────────►│
  │                      │                       │  core_view -= amount
  │                      │                       │  evm_view += amount
  │                      │                       │  (total unchanged!)
  │                      │                       │
  │                      │  Ok(())               │
  │                      │◄──────────────────────┤
  │  {status: "ok"}      │                       │
  │◄─────────────────────┤                       │
```

**Key Source File:** `crates/primitives/src/unified_state.rs:transfer_to_evm_view()` (line 203)

**Critical Invariant:** `total == core_view + evm_view` always holds.

---

## Markets

### Market Definition
```rust
struct Market {
    id: u8,                    // 0-255 markets supported
    symbol: String,            // e.g., "BTC-PERP"
    base_decimals: u8,         // Size precision (e.g., 3 for 0.001)
    quote_decimals: u8,        // Always 6 (USDC)
    tick_size: u64,            // Minimum price increment (scaled)
    lot_size: u64,             // Minimum size increment (scaled)
    max_leverage: u8,          // 1-100
    maintenance_margin: u64,   // Basis points (250 = 2.5%)
    initial_margin_factor: u64,// Basis points for initial margin buffer
    maker_fee: i64,            // Signed; negative = rebate
    taker_fee: u64,            // Always positive
    status: MarketStatus,      // Active, Paused, Settled
}
```

### MVP Markets
| ID | Symbol | Tick | Lot | Max Lev | Maint Margin |
|----|--------|------|-----|---------|--------------|
| 0 | BTC-PERP | 0.10 | 0.001 | 50x | 2.5% |
| 1 | ETH-PERP | 0.01 | 0.01 | 50x | 2.5% |

## Orders

### Order Types

**Limit Order**
- Rests on book at specified price
- Can be Good-Til-Canceled (GTC) or have time expiry

**Market Order**
- Executes immediately at best available price
- Unfilled portion is canceled (IOC behavior)

**Post-Only**
- Rejected if it would cross the spread
- Guarantees maker fee

**Immediate-or-Cancel (IOC)**
- Fills what it can immediately
- Remaining quantity is canceled

**Fill-or-Kill (FOK)**
- Must fill entirely or reject completely

**Reduce-Only**
- Can only decrease position size
- Automatically canceled if would increase position

### Order Lifecycle
```
             ┌──────────┐
             │ CREATED  │
             └────┬─────┘
                  │ validation
                  ▼
             ┌──────────┐
      ┌──────│  OPEN    │──────┐
      │      └────┬─────┘      │
      │           │            │
      ▼           ▼            ▼
┌──────────┐ ┌──────────┐ ┌──────────┐
│ CANCELED │ │ PARTIAL  │ │  FILLED  │
└──────────┘ └────┬─────┘ └──────────┘
                  │
                  ▼
             ┌──────────┐
             │  FILLED  │
             └──────────┘
```

### Order Validation
Before an order is accepted:
1. Market must be active
2. User must have sufficient free collateral
3. Order size >= lot_size
4. Price is multiple of tick_size
5. Post-only orders must not cross
6. Reduce-only orders must reduce existing position
7. Position + order size <= max position limit
8. Open orders count <= max orders per market (200)

### Matching Algorithm

```
function match(incoming_order):
    fills = []
    remaining = incoming_order.size

    while remaining > 0:
        # Get best opposing price level
        if incoming_order.side == BUY:
            best = orderbook.best_ask()
            crosses = best.price <= incoming_order.limit_price
        else:
            best = orderbook.best_bid()
            crosses = best.price >= incoming_order.limit_price

        if not crosses or best is None:
            break

        # Process orders at this price level (FIFO)
        for resting_order in best.orders:
            if remaining == 0:
                break

            # Self-trade prevention
            if resting_order.owner == incoming_order.owner:
                cancel(resting_order, reason=SELF_TRADE)
                continue

            fill_qty = min(remaining, resting_order.remaining)
            fill_price = resting_order.price  # Price improvement to taker

            fills.append(Fill{
                maker: resting_order.owner,
                taker: incoming_order.owner,
                price: fill_price,
                quantity: fill_qty,
                maker_order_id: resting_order.id,
                taker_order_id: incoming_order.id,
            })

            remaining -= fill_qty
            resting_order.remaining -= fill_qty

            if resting_order.remaining == 0:
                remove(resting_order)

    # Handle unfilled portion
    if remaining > 0:
        match incoming_order.type:
            case LIMIT:
                if incoming_order.post_only and len(fills) > 0:
                    revert("PostOnlyWouldCross")
                add_to_book(incoming_order, remaining)
            case MARKET, IOC:
                # Silently cancel unfilled portion
                pass
            case FOK:
                if len(fills) > 0:
                    revert("FOKPartialFill")

    return fills
```

## Positions

### Position State
```rust
struct Position {
    size: i128,              // Positive = long, negative = short
    entry_notional: u128,    // Cumulative entry value
    realized_pnl: i128,      // Settled P&L
    last_funding_index: i128,// For funding calculation
}
```

### Entry Price Calculation
```
entry_price = entry_notional / abs(size)
```

### Unrealized PnL
```
if size > 0:  # Long
    unrealized_pnl = size * (mark_price - entry_price)
else:  # Short
    unrealized_pnl = abs(size) * (entry_price - mark_price)
```

### Position Updates on Fill
```
function update_position(position, fill):
    old_size = position.size
    fill_size = fill.quantity * (fill.is_buy ? 1 : -1)
    new_size = old_size + fill_size

    if sign(old_size) == sign(new_size):
        # Increasing position
        position.entry_notional += fill.quantity * fill.price
        position.size = new_size
    elif abs(fill_size) <= abs(old_size):
        # Reducing position (partial close)
        close_ratio = fill.quantity / abs(old_size)
        closed_notional = position.entry_notional * close_ratio
        realized = fill.quantity * fill.price - closed_notional
        if old_size < 0:
            realized = -realized
        position.realized_pnl += realized
        position.entry_notional *= (1 - close_ratio)
        position.size = new_size
    else:
        # Flipping position (close + open opposite)
        # First: close entire old position
        close_pnl = calculate_close_pnl(position, fill.price)
        position.realized_pnl += close_pnl

        # Then: open new position with remaining
        remaining = abs(fill_size) - abs(old_size)
        position.size = remaining * sign(fill_size)
        position.entry_notional = remaining * fill.price
```

## Margin System

### Cross-Margin Model
All positions share the same collateral pool.

**Account Equity**
```
equity = balance + sum(unrealized_pnl for all positions)
```

**Initial Margin Required**
```
initial_margin = sum(
    abs(position.size) * mark_price / leverage
    for all positions
)
```

**Maintenance Margin Required**
```
maintenance_margin = sum(
    abs(position.size) * mark_price * maintenance_margin_rate
    for all positions
)
```

**Free Collateral**
```
free_collateral = equity - initial_margin
```

### Order Margin Reservation
When placing an order:
```
order_margin = order.size * order.price / leverage

if order.reduce_only:
    order_margin = 0  # No additional margin needed

if free_collateral < order_margin:
    reject("InsufficientMargin")
```

## Funding

### Funding Rate Calculation
Every 8 hours:
```
# Premium Index (TWAP of premium over funding period)
premium = (best_bid + best_ask) / 2 - index_price
premium_index = ewma(premium / index_price, span=5min)

# Clamp to ±0.05% per 8 hours
funding_rate = clamp(premium_index, -0.0005, 0.0005)
```

### Funding Settlement
```
# Global accumulator update
funding_accumulator += funding_rate * mark_price

# Per-position settlement (lazy, on position access)
function settle_funding(position, market):
    delta = market.funding_accumulator - position.last_funding_index
    payment = position.size * delta

    account.balance -= payment  # Longs pay when rate > 0
    position.last_funding_index = market.funding_accumulator
```

### Index Price
Aggregated from multiple oracle sources:
- Primary: Pyth Network price feed
- Fallback: Chainlink aggregator
- Validation: Reject prices deviating >5% from median

## Liquidation

### Liquidation Trigger
```
account_health = equity / maintenance_margin

if account_health < 1.0:
    trigger_liquidation(account)
```

### Liquidation Process
```
function liquidate(account):
    # Sort positions by notional value (largest first)
    positions = sorted(account.positions, by=notional, desc=true)

    for position in positions:
        if calculate_health(account) >= 1.0:
            break  # Account healthy

        # Partial liquidation: 25% of position
        liq_size = min(position.size * 0.25, position.size)

        # Liquidation price (includes spread penalty)
        if position.size > 0:  # Long
            liq_price = mark_price * 0.995  # 0.5% below mark
        else:  # Short
            liq_price = mark_price * 1.005  # 0.5% above mark

        # Place liquidation order
        fills = execute_liquidation_order(position, liq_size, liq_price)

        # Collect liquidation fee
        fee = sum(fill.value * 0.005 for fill in fills)
        insurance_fund += fee

        # Check for bankruptcy
        if calculate_equity(account) < 0:
            shortfall = -calculate_equity(account)
            if insurance_fund >= shortfall:
                insurance_fund -= shortfall
                account.balance = 0
            else:
                trigger_adl(position.market, opposite_side(position))
```

### Auto-Deleveraging (ADL)
When insurance fund cannot cover bankruptcy:
```
function trigger_adl(market, side):
    # Find profitable positions on opposite side
    # Ranked by profit ratio: unrealized_pnl / position_value
    profitable = get_profitable_positions(market, opposite(side))
    profitable = sorted(profitable, by=profit_ratio, desc=true)

    bankrupt_position = get_bankrupt_position()
    remaining = abs(bankrupt_position.size)

    for counter_position in profitable:
        if remaining == 0:
            break

        delever_size = min(remaining, abs(counter_position.size))

        # Force close at bankruptcy price
        execute_adl(bankrupt_position, counter_position, delever_size)

        remaining -= delever_size
```

## Fees

### Fee Structure
| Type | Rate | Recipient |
|------|------|-----------|
| Maker | 0.02% (2 bps) | Protocol |
| Taker | 0.05% (5 bps) | Protocol |
| Liquidation | 0.50% (50 bps) | Insurance Fund |

### Fee Calculation
```
maker_fee = fill_value * maker_fee_rate
taker_fee = fill_value * taker_fee_rate

# Fee deducted from realized PnL
maker_account.balance -= maker_fee  # Can be negative (rebate)
taker_account.balance -= taker_fee
```

## Nonce Management

Each account has a nonce for replay protection:
```
struct NonceState {
    next_nonce: u64,  # Next expected nonce
}

function validate_nonce(account, provided_nonce):
    # Allow timestamp-based nonces (within 1 hour)
    current_time_ms = current_timestamp_ms()
    if provided_nonce > current_time_ms - 3600000:
        if provided_nonce > account.last_timestamp_nonce:
            account.last_timestamp_nonce = provided_nonce
            return true

    # Or sequential nonces
    if provided_nonce == account.next_nonce:
        account.next_nonce += 1
        return true

    return false
```

## State Commitments

Each block commits to a deterministic hash of ALL state:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         STATE COMMITMENT STRUCTURE                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   app_hash = Keccak256(                                                         │
│       block_height,                    // Current block number                   │
│       timestamp,                       // Block timestamp (ms)                   │
│       prev_app_hash,                   // Chain link to previous block          │
│       unified_state_root,              // All balances (total, core, evm)       │
│       nonce_root,                      // All account nonces                    │
│   )                                                                             │
│                                                                                  │
│   unified_state_root = Keccak256(                                               │
│       // For each (address, token) pair, sorted deterministically:              │
│       for (addr, token, balance) in balances.sorted():                          │
│           hash(addr || token || balance.total || balance.core || balance.evm)   │
│   )                                                                             │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### How Consensus Uses State Commitment

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    CONSENSUS VERIFICATION VIA APP_HASH                           │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   Block N Proposed by Leader                                                    │
│   ─────────────────────────                                                     │
│                                                                                  │
│   Leader:                                                                        │
│   1. Executes all transactions                                                   │
│   2. Computes app_hash_N                                                         │
│   3. Broadcasts block + app_hash_N                                              │
│                                                                                  │
│   Each Validator:                                                                │
│   1. Receives block from leader                                                  │
│   2. Executes SAME transactions (deterministic!)                                │
│   3. Computes OWN app_hash_N                                                    │
│   4. Compares: own_hash == leader_hash ?                                        │
│      - YES → Vote to commit                                                     │
│      - NO  → Reject block (state divergence detected!)                          │
│                                                                                  │
│   Consensus:                                                                     │
│   - 2/3+ validators vote to commit                                              │
│   - Block finalized with VERIFIED app_hash                                      │
│   - All nodes guaranteed to have SAME state                                     │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### State Commitment Guarantees

| Property | How It's Enforced |
|----------|-------------------|
| **Deterministic** | Same txs → Same state → Same hash (no randomness) |
| **Complete** | All state included (balances, nonces, positions) |
| **Tamper-evident** | Any change produces different hash |
| **Chain-linked** | Each block includes prev_app_hash |

### Future: Merkle Proofs (Phase 3C)

Current implementation uses simple sorted hash. Production requires:
```
// Target: Merkle tree for state proofs
proof = compute_balance_proof(address, token)
// Allows light clients to verify balances without full state
verified = verify_balance_proof(proof, app_hash)
```

This allows:
- Light client verification
- State sync for new nodes
- Fraud proofs (future)

---

## Unified State Architecture (Phase 2A - IMPLEMENTED ✅)

### How It Works: Unified State Model

Both HyperCore and HyperEVM share the SAME underlying state through a **Master Balance Sheet**. This matches Hyperliquid's architecture.

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    HYPERCORE UNIFIED STATE MODEL (IMPLEMENTED ✅)                │
│                    Source: crates/primitives/src/unified_state.rs               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│                           HyperBFT Consensus                                    │
│                         (Single consensus layer)                                │
│                                    │                                             │
│                    ┌───────────────┴───────────────┐                            │
│                    │                               │                             │
│              HyperCore                        HyperEVM                           │
│           (Trading Engine)                     (EVM)                             │
│                    │                               │                             │
│                    └───────────────┬───────────────┘                            │
│                                    │                                             │
│                    ┌───────────────┴───────────────┐                            │
│                    │   MASTER BALANCE SHEET        │                            │
│                    │   (Single Source of Truth)    │                            │
│                    │                               │                            │
│                    │   User 0xf39F:                │                            │
│                    │   ┌─────────────────────────┐ │                            │
│                    │   │ Total USDC: 100,000    │ │                            │
│                    │   │ ├─ Core view: 80,000   │ │                            │
│                    │   │ └─ EVM view:  20,000   │ │                            │
│                    │   └─────────────────────────┘ │                            │
│                    │                               │                            │
│                    │   "Moving" between layers:    │                            │
│                    │   Just adjusts the VIEWS      │                            │
│                    │   NOT an actual transfer!     │                            │
│                    │                               │                            │
│                    └───────────────────────────────┘                            │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**Source:** [Inside Hyperliquid's Technical Architecture](https://www.blockhead.co/2025/06/05/inside-hyperliquids-technical-architecture/)

### Key Principles

1. **No Real Bridge**: When tokens "move" between HyperCore and HyperEVM, the system updates views in the master ledger - no actual transfer occurs

2. **No Wrapped Tokens**: The same token exists on both layers simultaneously with different "views" of the balance

3. **Atomic Updates**: View changes happen within the same consensus round - no risk of funds getting stuck

4. **System Addresses**: Each token has a system address (0x20 + token_index) that represents the EVM view

### System Address Formula

```
System Address = 0x20 || 00...00 || token_index (big-endian, 19 bytes padding)

Token Index 0 (USDC):  0x2000000000000000000000000000000000000000
Token Index 1 (TEST):  0x2000000000000000000000000000000000000001
Token Index 1385:      0x2000000000000000000000000000000000000569
```

### How "Transfers" Actually Work

**HyperCore → HyperEVM (No actual bridge!):**
```
Before: User has 100,000 USDC (Core view: 100,000, EVM view: 0)
Action: User requests 20,000 USDC to EVM
After:  User has 100,000 USDC (Core view: 80,000, EVM view: 20,000)

The TOTAL never changes - only the VIEWS change!
```

**HyperEVM → HyperCore:**
```
Before: User has 100,000 USDC (Core view: 80,000, EVM view: 20,000)
Action: User sends 20,000 USDC to Core from EVM
After:  User has 100,000 USDC (Core view: 100,000, EVM view: 0)
```

---

### Our Current Implementation (Phase 2A) - NOW MATCHES HYPERLIQUID ✅

**✅ Our implementation now uses the unified state model:**

```
User: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266

┌─────────────────────────────────────────────────────────────────────────────────┐
│  CURRENT: UNIFIED STATE MODEL (Phase 2A - IMPLEMENTED ✅)                        │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│                     ┌─────────────────────────────────┐                         │
│                     │  SharedUnifiedState             │                         │
│                     │  Arc<RwLock<UnifiedState>>      │                         │
│                     │  (crates/primitives/unified_state.rs)                     │
│                     ├─────────────────────────────────┤                         │
│                     │  UnifiedBalance {               │                         │
│                     │    total: 100,000 USDC          │                         │
│                     │    core_view: 80,000 ◄── SpotEngine reads                 │
│                     │    evm_view: 20,000  ◄── EvmState reads                   │
│                     │  }                              │                         │
│                     └─────────────────────────────────┘                         │
│                                    │                                             │
│              ┌─────────────────────┼─────────────────────┐                      │
│              │                     │                     │                      │
│              ▼                     ▼                     ▼                      │
│     ┌──────────────┐      ┌──────────────┐      ┌──────────────┐               │
│     │ SpotEngine   │      │   Gateway    │      │ EvmExecutor  │               │
│     │ (trading)    │      │   (:3000)    │      │  (:8545)     │               │
│     └──────────────┘      └──────────────┘      └──────────────┘               │
│                                                                                  │
│  ✅ SHARED STATE - All components use the same Arc<RwLock<UnifiedState>>        │
│  ✅ View transfers work (transfer_to_evm_view, transfer_to_core_view)           │
│  ✅ Reserved balance tracking for resting orders                                │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Phase 2A Implementation Summary ✅

| Component | Status | Source File |
|-----------|--------|-------------|
| UnifiedState struct | ✅ Complete | `crates/primitives/src/unified_state.rs` |
| UnifiedBalance { total, core_view, evm_view } | ✅ Complete | `unified_state.rs:37` |
| SpotEngine uses core_view | ✅ Complete | `spot_engine.rs:187` |
| EvmState uses evm_view | ✅ Complete | `state.rs:134` |
| View transfer methods | ✅ Complete | `unified_state.rs:203-259` |
| Shared process (Gateway + EVM RPC) | ✅ Complete | `node/main.rs:121-165` |
| Reserved balance tracking | ✅ Complete | `spot_engine.rs` |

### Token Index System (HIP-1)

HyperCore uses a token index system for spot tokens:

| Index | Symbol | Purpose |
|-------|--------|---------|
| 0 | USDC | Native quote token, used for all trading |
| 1 | TEST | First deployed token (example) |
| 2-255 | User tokens | Deployed via token deployment action |

**Market ID Assignment:**
- Perpetual markets: IDs 0-127 (BTC-PERP=0, ETH-PERP=1, ...)
- Spot markets: IDs 128-255 (TEST-USDC=128, ...)

### State Storage Locations

| State | Rust Type | File Location |
|-------|-----------|---------------|
| Spot balances | `HashMap<(AccountAddress, TokenIndex), SpotBalance>` | `crates/engine/src/spot_engine.rs` |
| Perp accounts | `HashMap<AccountAddress, Account>` | `crates/engine/src/state.rs` |
| Positions | `HashMap<(AccountAddress, MarketId), Position>` | `crates/engine/src/state.rs` |
| Orderbooks | `HashMap<MarketId, OrderBook>` | `crates/engine/src/state.rs` |
| EVM accounts | `HashMap<Address, EvmAccount>` | `crates/evm/src/state.rs` |
| EVM storage | `HashMap<Address, ContractStorage>` | `crates/evm/src/state.rs` |
| Contract code | `HashMap<B256, Vec<u8>>` | `crates/evm/src/state.rs` |

### Cross-Layer Communication

**Reading HyperCore state from EVM (Precompiles):**

EVM contracts can read HyperCore state via precompiles:

| Address | Name | Input | Output |
|---------|------|-------|--------|
| 0x0800 | PositionReader | `(address, market_id)` | Position struct |
| 0x0801 | AccountReader | `(address)` | Account balance, margin |
| 0x0802 | MarketReader | `(market_id)` | Market config |
| 0x0806 | SpotBalanceReader | `(address, token_index)` | SpotBalance struct |
| 0x0807 | SpotMarketReader | `(market_id)` | SpotMarket config |
| 0x0808 | SpotOrderBookReader | `(market_id, depth)` | L2 orderbook |

Example Solidity usage:
```solidity
// Read user's USDC balance on HyperCore
(bool success, bytes memory data) = address(0x0806).staticcall(
    abi.encodePacked(userAddress, uint8(0))  // token index 0 = USDC
);
(uint256 total, uint256 reserved, uint256 available) = abi.decode(data, (uint256, uint256, uint256));
```

**Writing to HyperCore from EVM (CoreWriter):**

EVM contracts can queue actions via CoreWriter precompile (0x0820):

```solidity
// Place order on HyperCore from EVM contract
ICoreWriter(0x0820).placeOrder(
    marketId,
    isBuy,
    price,
    size,
    orderType
);

// Transfer USDC from EVM to HyperCore for trading
ICoreWriter(0x0820).transferToCore(
    usdcAmount
);
```

**Important:** CoreWriter actions are queued and executed in the NEXT block. This prevents MEV attacks where an EVM transaction could read state, place an order, and read the result atomically.

### View Transfers (NOT Bridging!)

**Important:** HyperCore does NOT use bridging. View transfers are simple view adjustments:

**Core → EVM (Make funds available for DeFi):**
```
1. User submits viewTransfer action via Gateway API
2. UnifiedState adjusts views atomically:
   - core_view -= amount
   - evm_view += amount
   - total unchanged!
3. Response confirms new view balances

Source: crates/primitives/src/unified_state.rs:transfer_to_evm_view()
```

**EVM → Core (Make funds available for trading):**
```
1. User submits viewTransfer action via Gateway API
2. UnifiedState adjusts views atomically:
   - evm_view -= amount
   - core_view += amount
   - total unchanged!
3. Response confirms new view balances

Source: crates/primitives/src/unified_state.rs:transfer_to_core_view()
```

**Why This is Better Than Bridging:**
- No wrapped tokens needed
- No bridge contract security risks
- Atomic and instant (same transaction)
- Total balance always equals sum of views

### Consistency Guarantees

| Property | Mechanism |
|----------|-----------|
| **Atomic within layer** | Single-threaded execution |
| **Cross-layer ordering** | CometBFT orders all txs |
| **No MEV on state reads** | CoreWriter deferred execution |
| **Deterministic** | Same txs → same state on all nodes |

### Current Implementation Status

| Component | Status | Phase | Notes |
|-----------|--------|-------|-------|
| **Unified State Model** | ✅ **Complete** | **Phase 2A** | `crates/primitives/src/unified_state.rs` |
| **View Transfers** | ✅ **Complete** | **Phase 2A** | `transfer_to_evm_view()`, `transfer_to_core_view()` |
| **Shared Process** | ✅ **Complete** | **Phase 2A** | Gateway + EVM RPC in same process |
| Precompile reads | ✅ Complete | Phase 1 | 0x0800-0x0808 |
| CoreWriter queue | ⚠️ Partial | Phase 2B | Structure exists, needs consensus |
| CometBFT integration | ✅ **Complete** | **Phase 2B** | ABCI app, server, validators |

---

## Unified State Implementation (Phase 2A - COMPLETE ✅)

This section documents the implemented unified state model.

### Transaction Types for View Transfers

View transfers are implemented as follows:

```rust
/// Transaction for moving balance between views
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ViewTransferAction {
    /// Move tokens from Core view to EVM view
    TransferToEvm {
        token: TokenIndex,
        amount: Decimal,
    },
    /// Move tokens from EVM view to Core view
    TransferToCore {
        token: TokenIndex,
        amount: Decimal,
    },
}
```

### API Endpoints for View Transfers

View transfer endpoints (implemented):

```json
// POST /exchange
{
    "action": {
        "type": "viewTransfer",
        "direction": "toEvm",  // or "toCore"
        "token": 0,
        "amount": "1000.0"
    },
    "nonce": 1234567890,
    "signature": {...}
}

// Response
{
    "status": "ok",
    "response": {
        "type": "viewTransfer",
        "newCoreView": "9000.0",
        "newEvmView": "1000.0",
        "totalUnchanged": "10000.0"
    }
}
```

### Gateway Query: Unified Balances

Balance queries now show both views:

```json
// GET /info with type=unifiedBalances
{
    "type": "unifiedBalances",
    "user": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
}

// Response
{
    "balances": [
        {
            "token": 0,
            "symbol": "USDC",
            "total": "10000.0",
            "coreView": "8000.0",
            "evmView": "2000.0"
        },
        {
            "token": 1,
            "symbol": "TEST",
            "total": "5000.0",
            "coreView": "5000.0",
            "evmView": "0.0"
        }
    ]
}
```

### EVM Integration

The EVM layer reads from unified state:

```solidity
// Precompile at 0x0809: UnifiedBalanceReader
interface IUnifiedBalance {
    struct Balance {
        uint256 total;
        uint256 coreView;
        uint256 evmView;
    }

    function getUnifiedBalance(
        address user,
        uint8 tokenIndex
    ) external view returns (Balance memory);
}

// Usage in contract
IUnifiedBalance reader = IUnifiedBalance(0x0809);
IUnifiedBalance.Balance memory bal = reader.getUnifiedBalance(msg.sender, 0);
// bal.evmView is what's available for EVM operations
```

### State Commitment (Phase 2B - Future)

The unified state will be included in block commitments once CometBFT is integrated:

```rust
pub fn compute_block_commitment(&self) -> [u8; 32] {
    let mut hasher = Keccak256::new();

    // Include unified state in commitment
    hasher.update(&self.unified_state.compute_root());

    // Include EVM state (storage, code - not balances)
    hasher.update(&self.evm_state.compute_root());

    // Include orderbook state
    hasher.update(&self.engine_state.compute_root());

    hasher.finalize().into()
}
```

---

## Test Coverage (Phase 2A)

### E2E Tests - All Passing ✅

The E2E tests (104+ tests) verify the unified state model works correctly:

| Test Category | Tests | State Tested | Status |
|---------------|-------|--------------|--------|
| **Spot Trading** | 12+ | core_view | ✅ Passing |
| **EVM Execution** | 24+ | evm_view | ✅ Passing |
| **Token Standards** | 8+ | evm_view | ✅ Passing |
| **Precompile Reads** | 6+ | Both layers | ✅ Passing |
| **Unified State** | 4+ | View transfers | ✅ Passing |
| **Reserved Balances** | 2+ | Order reserves | ✅ Passing |

### Key Unified State Tests

```typescript
// scripts/e2e/runner.ts - Implemented tests

// Test: EVM balance reflects unified state evm_view
// Verifies: eth_getBalance returns the evm_view from UnifiedState

// Test: Reserved balance prevents over-transfer
// Verifies: Resting orders reserve balance, preventing double-spend

// Test: View transfer from Core to EVM
// Verifies: core_view decreases, evm_view increases, total unchanged

// Test: View transfer from EVM to Core
// Verifies: evm_view decreases, core_view increases, total unchanged
```

### Unit Tests

```bash
# Run unit tests for unified state
cargo test -p hypercore-primitives -- unified_state

# Run unit tests for spot engine (includes reserve tests)
cargo test -p hypercore-engine -- spot_engine

# All tests: 8 spot_engine tests passing
```

See [ARCHITECTURE.md](./ARCHITECTURE.md) for detailed implementation diagrams.
