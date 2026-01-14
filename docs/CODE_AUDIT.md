# HyperCore Code Audit Checklist

This document provides a comprehensive audit of all crates in the HyperCore monorepo, identifying implementation status, issues, and areas needing work before production.

## Legend
- [x] Complete and functional
- [~] Partial implementation / needs work
- [ ] Stub / not implemented
- [!] Critical issue needs fixing

---

## 1. Primitives Crate (`crates/primitives/`)

### Types (`types.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `AccountAddress` type | [x] | Wrapper around `[u8; 20]` |
| `Signature` struct | [~] | Has r/s/v but `v` is `u32` - should be `u8` for EIP-712 |
| `Timestamp` type alias | [x] | `u64` milliseconds |
| `BlockHeight` type alias | [x] | `u64` |
| `MarketId` type alias | [x] | `u8` (limits to 256 markets) |
| `OrderId` type alias | [x] | `u64` |

### Decimal (`decimal.rs`)
| Item | Status | Notes |
|------|--------|-------|
| Core struct | [~] | Needs verification of field names |
| `from_str()` | [~] | Referenced but may not exist |
| `to_string()` | [~] | Referenced but may not exist |
| `scale_to(decimals)` | [ ] | **MISSING** - Referenced in precompiles.rs |
| `to_be_bytes()` | [ ] | **MISSING** - Referenced in precompiles.rs |
| `to_be_bytes_signed()` | [ ] | **MISSING** - Referenced in precompiles.rs |
| `is_negative()` | [ ] | **MISSING** - Referenced in precompiles.rs |
| `is_zero()` | [~] | Referenced but may not exist |
| `ZERO` constant | [~] | Referenced but may not exist |
| Arithmetic ops (+, -, *, /) | [~] | Needs implementation |

### Order (`order.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `Order` struct | [x] | Has id, owner, market_id, side, price, size, filled_size, timestamp |
| `OrderRequest` struct | [~] | Needs `owner` field added |
| `OrderSide` enum | [x] | Buy/Sell |
| `OrderType` enum | [x] | Limit, Market, StopLimit |
| `TimeInForce` enum | [x] | GTC, IOC, FOK, ALO |
| `is_filled()` method | [~] | Referenced but needs verification |
| `is_resting_order()` method | [ ] | **MISSING** - Referenced in app.rs |

### Position (`position.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `Position` struct | [~] | Has size, entry_price, but needs more fields |
| `unrealized_pnl` field | [ ] | **MISSING** - Referenced in precompiles |
| `margin_used` field | [ ] | **MISSING** - Referenced in precompiles |
| `liquidation_price` field | [ ] | **MISSING** - Referenced in precompiles |
| `leverage` field | [ ] | **MISSING** - Referenced in precompiles |
| `is_long()` method | [ ] | **MISSING** - Referenced in precompiles |

### Market (`market.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `MarketConfig` struct | [~] | Needs all fields verified |
| `MarketState` struct | [ ] | **MISSING** - Referenced for mark_price, index_price, etc. |

### Error (`error.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `HyperCoreError` enum | [~] | Basic errors defined |
| Error codes | [~] | Needs comprehensive coverage |

---

## 2. Engine Crate (`crates/engine/`)

### EngineState (`state.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `add_market()` | [~] | Referenced but signature may differ |
| `get_market()` | [~] | Referenced but may need adjustment |
| `get_market_state()` | [ ] | **MISSING** - Referenced in precompiles |
| `get_orderbook()` | [~] | Referenced |
| `get_orderbook_mut()` | [~] | Referenced |
| `get_account()` | [~] | Referenced |
| `get_position()` | [~] | Referenced |
| `get_balance()` | [~] | Referenced |
| `get_nonce()` | [ ] | **MISSING** - Referenced in handlers |
| `increment_nonce()` | [ ] | **MISSING** - Referenced in handlers |
| `deposit()` | [~] | Referenced |
| `withdraw()` | [~] | Referenced |
| `transfer()` | [~] | Referenced |
| `create_order()` | [ ] | **MISSING** - Referenced in app.rs |
| `cancel_order()` | [~] | Referenced |
| `cancel_order_by_cloid()` | [ ] | **MISSING** - Referenced |
| `cancel_all_orders()` | [~] | Referenced |
| `apply_fill()` | [~] | Referenced |
| `update_leverage()` | [~] | Referenced |
| `has_market()` | [ ] | **MISSING** - Referenced in app.rs |
| `compute_state_root()` | [ ] | **MISSING** - Referenced in state.rs |
| `get_all_markets()` | [ ] | **MISSING** - Referenced in handlers |
| `get_market_id_by_name()` | [ ] | **MISSING** - Referenced in handlers |
| `get_market_name()` | [ ] | **MISSING** - Referenced in handlers |
| `get_all_mid_prices()` | [ ] | **MISSING** - Referenced in handlers |
| `get_open_orders()` | [ ] | **MISSING** - Referenced in handlers |
| `get_user_fills()` | [ ] | **MISSING** - Referenced in handlers |
| `get_funding_history()` | [ ] | **MISSING** - Referenced in handlers |
| `get_user_funding_history()` | [ ] | **MISSING** - Referenced in handlers |
| `get_recent_trades()` | [ ] | **MISSING** - Referenced in handlers |
| `get_candles()` | [ ] | **MISSING** - Referenced in handlers |
| `current_timestamp()` | [ ] | **MISSING** - Referenced in handlers |
| `current_height()` | [ ] | **MISSING** - Referenced in indexer |
| `place_order()` | [~] | Referenced in handlers |
| `last_funding_time()` | [ ] | **MISSING** - Referenced in app.rs |
| `set_last_funding_time()` | [ ] | **MISSING** - Referenced in app.rs |
| `apply_funding_payment()` | [ ] | **MISSING** - Referenced in app.rs |
| `apply_liquidation()` | [ ] | **MISSING** - Referenced in app.rs |
| `update_mark_price()` | [ ] | **MISSING** - Referenced in node main |
| `should_apply_funding()` | [ ] | **MISSING** - Referenced in node main |
| `apply_funding()` | [ ] | **MISSING** - Referenced in node main |
| `get_block_hash()` | [ ] | **MISSING** - Referenced in indexer |
| `get_block_timestamp()` | [ ] | **MISSING** - Referenced in indexer |
| `get_block_tx_count()` | [ ] | **MISSING** - Referenced in indexer |
| `get_block_events()` | [ ] | **MISSING** - Referenced in indexer |
| Serialization (Clone, Serialize, Deserialize) | [ ] | **MISSING** - Required for snapshots |

### OrderBook (`orderbook.rs`)
| Item | Status | Notes |
|------|--------|-------|
| BTreeMap-based structure | [~] | Design is sound |
| `insert()` | [~] | Referenced |
| `get_l2_snapshot()` | [~] | Referenced |
| `L2Snapshot` struct | [ ] | Needs `to_levels()` method |

### MatchingEngine (`matching.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `process_order()` | [~] | Signature may differ from usage |

### RiskEngine (`risk.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `check_order_margin()` | [~] | Referenced but signature may differ |
| `calculate_withdrawable()` | [ ] | **MISSING** - Referenced in app.rs |

### FundingEngine (`funding.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `should_apply_funding()` | [~] | Referenced |
| `calculate_funding()` | [~] | Referenced |

### LiquidationEngine (`liquidation.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `find_liquidatable_accounts()` | [~] | Referenced |
| `liquidate_account()` | [~] | Referenced |

### L2Snapshot struct
| Item | Status | Notes |
|------|--------|-------|
| `bids` field | [~] | Vec of levels |
| `asks` field | [~] | Vec of levels |
| `to_levels()` method | [ ] | **MISSING** - Referenced in handlers |
| Level struct with price/size | [~] | Referenced in precompiles |

### Account struct (in engine)
| Item | Status | Notes |
|------|--------|-------|
| `balance` field | [~] | Referenced |
| `positions` field | [ ] | **MISSING** - HashMap referenced in handlers |
| `equity()` method | [ ] | **MISSING** - Referenced |
| `margin_used()` method | [ ] | **MISSING** - Referenced |
| `withdrawable()` method | [ ] | **MISSING** - Referenced |
| `total_notional_position()` method | [ ] | **MISSING** - Referenced |

---

## 3. Chain Crate (`crates/chain/`)

### Transaction (`tx.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `Transaction` struct | [!] | **DESIGN NOTE**: This is for HyperCore actions, NOT EVM txs |
| Missing: gas fields | [!] | Intentional - HyperCore doesn't use gas for exchange txs |
| Missing: value field | [!] | Intentional - amounts in action payload |
| `compute_hash()` | [x] | Uses JSON + nonce |
| `recover_signer()` | [x] | EIP-712 recovery |
| `compute_eip712_hash()` | [~] | Implemented |
| Domain separator | [!] | **BUG**: Chain ID hardcoded to 1337 |
| `OrderWire` | [x] | Wire format for orders |
| `CancelWire` | [x] | Wire format for cancels |
| EIP-712 type hashes | [~] | Implemented but may have encoding issues |

### AppState (`state.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `engine` field (Arc<RwLock>) | [x] | Shared engine state |
| Nonce management | [x] | HashMap-based |
| Block hash tracking | [x] | HashMap-based |
| `begin_block()` | [x] | Sets height/timestamp |
| `end_block()` | [x] | Computes app hash |
| `commit()` | [ ] | **STUB** - No persistence |
| `snapshot()` / `from_snapshot()` | [~] | Implemented but EngineState needs Clone/Serialize |

### HyperCoreApp (`app.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Creates engines |
| `init_from_genesis()` | [~] | Parses genesis JSON |
| `check_tx()` | [~] | Validates transactions |
| `execute_tx()` | [~] | Executes transactions |
| `execute_orders()` | [!] | **ISSUE**: Borrow checker problems with engine |
| `execute_cancels()` | [~] | Implemented |
| `execute_evm_action()` | [ ] | **STUB** - Just emits event |
| `begin_block()` / `end_block()` | [x] | Block lifecycle |
| Query methods | [~] | Basic implementation |

### ABCI Service (`abci.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `info()` | [x] | Returns version/height |
| `query()` | [~] | Basic paths implemented |
| `check_tx()` | [x] | Validates via app |
| `init_chain()` | [~] | Calls genesis init |
| `prepare_proposal()` | [x] | Returns txs as-is |
| `process_proposal()` | [x] | Validates all txs |
| `finalize_block()` | [x] | Executes txs |
| `commit()` | [x] | Calls app commit |
| State sync (snapshots) | [ ] | **NOT IMPLEMENTED** |

### Mempool (`mempool.rs`)
| Item | Status | Notes |
|------|--------|-------|
| Basic structure | [x] | HashMap + BTreeMap |
| `add()` | [!] | **ISSUE**: Calls recover_signer which may fail |
| Expiry pruning | [x] | Implemented |
| Nonce gap handling | [~] | Basic check |

---

## 4. EVM Crate (`crates/evm/`)

### EvmExecutor (`executor.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `new()` | [x] | Constructor |
| `execute_tx()` | [~] | Basic implementation |
| `build_cache_db()` | [!] | **EMPTY** - Doesn't load state! |
| `execute_with_precompiles()` | [!] | **STUB** - Doesn't intercept precompiles |
| `apply_state_changes()` | [x] | Updates internal state |
| `process_core_writer_logs()` | [~] | Parses events |
| `handle_core_writer_log()` | [!] | **HARDCODED**: block=0, timestamp=0 |
| `deploy_contract()` | [~] | Basic implementation |
| `call_contract()` | [~] | Basic implementation |
| `EvmTransaction` struct | [x] | Has from, to, value, data, gas_limit, gas_price, nonce |

### Precompiles (`precompiles.rs`)
| Item | Status | Notes |
|------|--------|-------|
| Address mapping (0x0800-0x0805) | [x] | Correct range |
| Gas costs | [x] | Defined per precompile |
| `get_position()` | [!] | **ISSUE**: Uses blocking_read on async lock |
| `get_account()` | [!] | **ISSUE**: Calls missing Account methods |
| `get_market()` | [!] | **ISSUE**: Calls missing MarketState |
| `get_order()` | [~] | Basic implementation |
| `get_funding()` | [!] | **ISSUE**: Calls missing MarketState.next_funding_time |
| `get_orderbook()` | [~] | Basic implementation |
| ABI encoding helpers | [!] | **ISSUE**: Call missing Decimal methods |

### CoreWriter (`core_writer.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `ActionType` enum | [x] | 5 action types |
| `CoreWriterAction` struct | [x] | Complete |
| `ActionQueue` | [x] | Queue management |
| Decode methods | [~] | Basic implementation |

### EvmState (`state.rs`)
| Item | Status | Notes |
|------|--------|-------|
| Account management | [x] | HashMap-based |
| Storage management | [x] | HashMap-based |
| Code storage | [x] | By code hash |
| Snapshot/restore | [x] | Implemented |

---

## 5. Gateway Crate (`crates/gateway/`)

### API Types (`api.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `InfoRequest` enum | [x] | All request types |
| `ExchangeRequest` struct | [x] | Action + nonce + signature |
| `ExchangeAction` enum | [x] | All action types |
| Response types | [x] | Comprehensive |

### Handlers (`handlers.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `handle_info()` | [x] | Routes to process function |
| `process_info_request()` | [!] | **ISSUE**: Calls many missing EngineState methods |
| `handle_exchange()` | [x] | Routes to process function |
| `process_exchange_request()` | [!] | **ISSUE**: Calls missing methods |
| `verify_signature()` | [!] | **STUB** - Extracts address from r value! |
| `place_order()` | [~] | Calls engine.place_order |
| `parse_address()` | [x] | Hex parsing |

### WebSocket (`websocket.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `WsManager` | [x] | Broadcast channel management |
| `WsSubscription` enum | [x] | All subscription types |
| `WsMessage` enum | [x] | All message types |
| `ws_handler()` | [x] | WebSocket upgrade |
| `handle_socket()` | [~] | Subscription handling |
| Unsubscribe cleanup | [ ] | **MISSING** - Doesn't stop receivers |

### Server (`server.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `GatewayServer` | [x] | HTTP server setup |
| `run()` | [x] | Starts server |
| `run_with_shutdown()` | [x] | Graceful shutdown |
| CORS | [x] | Configured |
| Health check | [x] | `/health` endpoint |

---

## 6. Indexer Crate (`crates/indexer/`)

### Database (`db.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `connect()` | [x] | PgPool connection |
| `run_migrations()` | [~] | Uses sqlx migrations |
| Insert methods | [x] | All tables covered |
| `get_latest_height()` | [x] | Query implementation |

### Ingestion (`ingest.rs`)
| Item | Status | Notes |
|------|--------|-------|
| `Indexer` struct | [x] | Basic structure |
| `run()` | [~] | Main loop |
| `index_block()` | [!] | **ISSUE**: Calls missing engine methods |
| `process_event()` | [~] | Event handling |
| `BlockEvent` enum | [~] | Event types defined |

### Models (`models.rs`)
| Item | Status | Notes |
|------|--------|-------|
| All models | [x] | sqlx::FromRow derived |
| Chrono timestamps | [x] | DateTime<Utc> |

### Queries (`queries.rs`)
| Item | Status | Notes |
|------|--------|-------|
| Query methods | [x] | All implemented |
| Pagination | [~] | limit parameter |
| Time filtering | [~] | start_time/end_time |

---

## 7. Node Crate (`crates/node/`)

### Main (`main.rs`)
| Item | Status | Notes |
|------|--------|-------|
| CLI parsing | [x] | clap-based |
| `start` command | [~] | Starts services |
| `init` command | [x] | Creates genesis.json |
| `export` command | [ ] | **STUB** - Not implemented |
| `import` command | [ ] | **STUB** - Not implemented |
| Gateway start | [x] | Works |
| ABCI server | [!] | **STUB** - Just sleeps forever |
| Indexer start | [ ] | **STUB** - Logs only |
| Mock price feed | [x] | Random walk simulation |
| Funding processor | [~] | Calls engine methods |

---

## 8. Solidity Contracts (`contracts/`)

| Item | Status | Notes |
|------|--------|-------|
| `IPrecompiles.sol` | [x] | Interface definitions |
| `ICoreWriter.sol` | [x] | Interface definition |
| `CoreWriter.sol` | [x] | Event emission |
| `HyperCore.sol` | [x] | Library wrapper |
| `VaultExample.sol` | [x] | Integration example |
| Tests | [ ] | **MISSING** |

---

## 9. SDKs

### TypeScript SDK (`sdk/typescript/`)
| Item | Status | Notes |
|------|--------|-------|
| Types | [x] | Comprehensive |
| Info client | [x] | Read methods |
| Exchange client | [x] | Write methods |
| WebSocket | [x] | Real-time subscriptions |
| Signing | [x] | viem-based EIP-712 |
| Build config | [ ] | **MISSING** tsconfig.json |

### Python SDK (`sdk/python/`)
| Item | Status | Notes |
|------|--------|-------|
| Types (Pydantic) | [x] | Comprehensive |
| Info client | [x] | Async with httpx |
| Exchange client | [x] | Signed requests |
| Signing | [x] | eth-account based |
| Tests | [ ] | **MISSING** |

---

## Critical Issues Summary

### P0 - Blockers (Must fix to run)
1. **Missing EngineState methods** - Many referenced methods don't exist
2. **Missing Decimal methods** - `scale_to`, `to_be_bytes`, etc.
3. **Missing Account/Position fields and methods**
4. **ABCI server is stub** - Doesn't actually listen

### P1 - Security Issues
1. **verify_signature() is stub** - Extracts address from r value (testing only!)
2. **Hardcoded chain_id in domain separator**
3. **blocking_read() on async locks** in precompiles

### P2 - Functionality Gaps
1. **EVM precompiles not actually intercepted** by executor
2. **State persistence not implemented** (commit is empty)
3. **Export/Import commands are stubs**
4. **WebSocket unsubscribe doesn't clean up**

### P3 - Code Quality
1. **Many TODO/placeholder comments**
2. **Missing tests** in contracts, SDKs
3. **Inconsistent error handling**

---

## Recommendations

1. **Start with primitives** - Implement all missing Decimal/Position/Account methods
2. **Complete EngineState** - Add all referenced methods
3. **Implement real signature verification** in gateway handlers
4. **Wire up ABCI server** with tendermint-abci crate
5. **Add integration tests** across crates
6. **Add persistence layer** for state snapshots
