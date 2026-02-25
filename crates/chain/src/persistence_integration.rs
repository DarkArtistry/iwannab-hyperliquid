//! Persistence Integration for HyperCore Chain
//!
//! This module provides functions to extract state from runtime components
//! and restore state back to them, enabling persistent storage.

use std::collections::HashMap;
use std::sync::Arc;

use hypercore_persistence::{
    BalanceEntry, BlockMetaEntry, ChainState, CloidEntry, CoreState, EvmStateData,
    LeverageEntry, MarketEntry, NonceEntry, OrderEntry, PersistedState, PositionEntry,
    ReservedEntry, SpotMarketEntry, SpotState, SpotTokenEntry, StateExtractor,
    UnifiedBalanceData, SCHEMA_VERSION,
};
use hypercore_primitives::{
    AccountAddress, Decimal, MarketId, OrderId, Position, SharedUnifiedState,
    TokenIndex, UnifiedBalance,
};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::state::{AppState, BlockMeta, SharedEngine, SharedEngineState, SharedSpotEngine};

/// Extract complete state from runtime components into PersistedState
///
/// When `perp_engine` is provided, trading state (positions, orders, leverage,
/// markets, scalars) is extracted from it into `perp.*` fields. The shared
/// `engine` parameter is kept for backward compatibility but its trading state
/// fields are no longer used when perp_engine is available.
pub fn extract_state(
    height: u64,
    timestamp: u64,
    app_hash: [u8; 32],
    unified_state: &SharedUnifiedState,
    engine: &SharedEngineState,
    perp_engine: Option<&SharedEngine>,
    spot_engine: Option<&SharedSpotEngine>,
    nonces: &HashMap<AccountAddress, u64>,
    cloid_index: &HashMap<(AccountAddress, String), (MarketId, OrderId)>,
    block_metadata: &HashMap<u64, BlockMeta>,
) -> PersistedState {
    let mut state = PersistedState::new();
    state.height = height;
    state.timestamp = timestamp;
    state.app_hash = app_hash;

    // Extract unified balances
    {
        let unified = unified_state.read().unwrap();
        let all_balances = unified.get_all_balances_global();
        for ((account, token), balance) in all_balances.iter() {
            state.core.balances.push(BalanceEntry {
                account: *account,
                token: *token,
                balance: UnifiedBalanceData {
                    total: balance.total.to_string_trimmed(),
                    core_view: balance.core_view.to_string_trimmed(),
                    evm_view: balance.evm_view.to_string_trimmed(),
                    decimals: balance.total.decimals(),
                },
            });
        }
    }

    // Extract perpetual engine state
    // When perp_engine is available (CometBFT mode), extract all trading state from it.
    // The perp_engine has the actual positions, orders, leverage, and updated scalars.
    // Store in perp.* fields as the canonical source. Also mirror scalars to core.*
    // for backward compatibility.
    if let Some(perp) = perp_engine {
        let perp_eng = perp.try_read()
            .expect("PerpEngine lock should be uncontested during state extraction");

        // Extract positions from perp_engine
        for (account, positions) in perp_eng.state.get_all_positions_global() {
            for (market_id, position) in positions {
                state.perp.positions.push(PositionEntry {
                    account: *account,
                    market: *market_id,
                    position: position.clone(),
                });
            }
        }

        // Extract leverage from perp_engine
        for (account, leverages) in perp_eng.state.get_all_leverage_global() {
            for (market_id, leverage) in leverages {
                state.perp.leverage.push(LeverageEntry {
                    account: *account,
                    market: *market_id,
                    leverage: *leverage,
                });
            }
        }

        // Extract markets from perp_engine (has current mark_price, funding state)
        // CRITICAL: Fee rates MUST be persisted for consensus safety
        for market in perp_eng.state.get_all_markets() {
            state.perp.markets.push(MarketEntry {
                id: market.id(),
                symbol: market.config.symbol.clone(),
                max_leverage: market.config.max_leverage,
                tick_size: market.config.tick_size.to_string_trimmed(),
                lot_size: market.config.lot_size.to_string_trimmed(),
                maker_fee_rate: market.config.maker_fee_rate.to_string_trimmed(),
                taker_fee_rate: market.config.taker_fee_rate.to_string_trimmed(),
                mark_price: market.state.mark_price.to_string_trimmed(),
                index_price: market.state.index_price.to_string_trimmed(),
                funding_rate: market.state.funding_rate.to_string_trimmed(),
                funding_accumulator: market.state.funding_accumulator.to_string_trimmed(),
                open_interest_long: market.state.open_interest_long.to_string_trimmed(),
                open_interest_short: market.state.open_interest_short.to_string_trimmed(),
                next_funding_time: market.state.next_funding_time,
            });
        }

        // Extract orders from perp_engine orderbooks
        for market in perp_eng.state.get_all_markets() {
            if let Some(orderbook) = perp_eng.state.get_orderbook(market.id()) {
                for order in orderbook.get_all_orders() {
                    state.perp.orders.push(OrderEntry {
                        market: market.id(),
                        order_id: order.id,
                        order: order.clone(),
                    });
                }
            }
        }

        // Scalars: store in perp (canonical) and core (backward compat)
        let next_order_id = perp_eng.state.peek_next_order_id();
        let insurance_fund = perp_eng.state.get_insurance_fund().to_string_trimmed();
        state.perp.next_order_id = next_order_id;
        state.perp.insurance_fund = insurance_fund.clone();
        state.core.next_order_id = next_order_id;
        state.core.insurance_fund = insurance_fund;
    } else {
        // Fallback: extract from shared engine (for tests without perp_engine)
        let eng = engine.try_read()
            .expect("Engine lock should be uncontested during state extraction");

        for (account, positions) in eng.get_all_positions_global() {
            for (market_id, position) in positions {
                state.core.positions.push(PositionEntry {
                    account: *account,
                    market: *market_id,
                    position: position.clone(),
                });
            }
        }

        for (account, leverages) in eng.get_all_leverage_global() {
            for (market_id, leverage) in leverages {
                state.core.leverage.push(LeverageEntry {
                    account: *account,
                    market: *market_id,
                    leverage: *leverage,
                });
            }
        }

        for market in eng.get_all_markets() {
            state.core.markets.push(MarketEntry {
                id: market.id(),
                symbol: market.config.symbol.clone(),
                max_leverage: market.config.max_leverage,
                tick_size: market.config.tick_size.to_string_trimmed(),
                lot_size: market.config.lot_size.to_string_trimmed(),
                maker_fee_rate: market.config.maker_fee_rate.to_string_trimmed(),
                taker_fee_rate: market.config.taker_fee_rate.to_string_trimmed(),
                mark_price: market.state.mark_price.to_string_trimmed(),
                index_price: market.state.index_price.to_string_trimmed(),
                funding_rate: market.state.funding_rate.to_string_trimmed(),
                funding_accumulator: market.state.funding_accumulator.to_string_trimmed(),
                open_interest_long: market.state.open_interest_long.to_string_trimmed(),
                open_interest_short: market.state.open_interest_short.to_string_trimmed(),
                next_funding_time: market.state.next_funding_time,
            });
        }

        for market in eng.get_all_markets() {
            if let Some(orderbook) = eng.get_orderbook(market.id()) {
                for order in orderbook.get_all_orders() {
                    state.core.orders.push(OrderEntry {
                        market: market.id(),
                        order_id: order.id,
                        order: order.clone(),
                    });
                }
            }
        }

        state.core.next_order_id = eng.peek_next_order_id();
        state.core.insurance_fund = eng.get_insurance_fund().to_string_trimmed();
    }

    // Extract spot engine state
    if let Some(spot) = spot_engine {
        let spot_eng = spot.try_read()
            .expect("SpotEngine lock should be uncontested during state extraction");

        // Extract spot tokens
        for token in spot_eng.state.get_all_tokens() {
            state.spot.tokens.push(SpotTokenEntry {
                index: token.index,
                symbol: token.symbol.clone(),
                name: token.name.clone(),
                wei_decimals: token.wei_decimals,
                sz_decimals: token.sz_decimals,
                max_supply: token.max_supply.to_string_trimmed(),
                deployer: token.deployer,
            });
        }

        // Extract spot markets
        // CRITICAL: Fee rates MUST be persisted for consensus safety
        // Different fee rates after restore cause AppHash mismatch and consensus failure
        for market in spot_eng.state.get_all_markets() {
            state.spot.markets.push(SpotMarketEntry {
                id: market.config.id as u32,
                symbol: market.config.symbol.clone(),
                base_token: market.config.base_token,
                quote_token: market.config.quote_token,
                tick_size: market.config.tick_size.to_string_trimmed(),
                lot_size: market.config.lot_size.to_string_trimmed(),
                maker_fee_rate: market.config.maker_fee_rate.to_string_trimmed(),
                taker_fee_rate: market.config.taker_fee_rate.to_string_trimmed(),
            });
        }

        // Extract reserved balances
        for ((account, token), amount) in spot_eng.state.get_all_reserved() {
            state.spot.reserved.push(ReservedEntry {
                account: *account,
                token: *token,
                amount: amount.to_string_trimmed(),
            });
        }

        // Extract spot orders
        for market in spot_eng.state.get_all_markets() {
            for order in spot_eng.state.get_all_orders(market.config.id) {
                state.spot.orders.push(OrderEntry {
                    market: market.config.id,
                    order_id: order.id,
                    order: order.clone(),
                });
            }
        }

        state.spot.next_token_index = spot_eng.state.next_token_index();
        state.spot.next_market_id = spot_eng.state.next_market_id() as u32;
        state.spot.next_order_id = spot_eng.state.peek_next_order_id();
    }

    // Extract nonces
    for (account, nonce) in nonces {
        state.chain.nonces.push(NonceEntry {
            account: *account,
            nonce: *nonce,
        });
    }

    // Extract CLOID index
    for ((account, cloid), (market, order_id)) in cloid_index {
        state.chain.cloid_index.push(CloidEntry {
            account: *account,
            cloid: cloid.clone(),
            market: *market,
            order_id: *order_id,
        });
    }

    // Extract block metadata (last 100 blocks)
    let mut block_entries: Vec<_> = block_metadata.iter().collect();
    block_entries.sort_by_key(|(h, _)| *h);
    for (_, meta) in block_entries.iter().rev().take(100) {
        state.chain.blocks.push(BlockMetaEntry {
            height: meta.height,
            timestamp: meta.timestamp,
            hash: meta.hash,
            app_hash: meta.hash, // Use hash as app_hash for now
            tx_count: meta.tx_count,
        });
    }

    state
}

/// Restore PersistedState into runtime components
///
/// This function restores state from a PersistedState into the runtime components.
/// Restores: unified balances, shared engine (markets only when perp_engine
/// will be used later), and spot engine state.
/// Returns the height, timestamp, and app_hash that were restored.
///
/// For perp_engine restoration, call `restore_perp_engine_state()` separately
/// after the perp_engine has been created.
pub fn restore_state(
    state: &PersistedState,
    unified_state: &SharedUnifiedState,
    engine: &SharedEngineState,
    spot_engine: Option<&SharedSpotEngine>,
) -> Result<(u64, u64, [u8; 32]), RestoreError> {
    let perp_engine: Option<&SharedEngine> = None; // Perp engine restored separately
    info!(
        "Restoring state: height={}, timestamp={}, app_hash={}, balances={}, positions={}, orders={}",
        state.height,
        state.timestamp,
        hex::encode(&state.app_hash),
        state.core.balances.len(),
        state.perp.positions.len(),
        state.perp.orders.len(),
    );

    // Validate schema version
    if state.schema_version != SCHEMA_VERSION {
        return Err(RestoreError::VersionMismatch {
            expected: SCHEMA_VERSION,
            found: state.schema_version,
        });
    }

    // Validate state before restoring
    state.validate().map_err(|e| RestoreError::ValidationFailed(e))?;

    // Restore unified balances
    {
        let mut unified = unified_state.write().unwrap();

        for balance in &state.core.balances {
            // Use the stored decimals field, falling back to token-based defaults
            // for backwards compatibility with older snapshots
            let decimals = if balance.balance.decimals > 0 {
                balance.balance.decimals
            } else if balance.token == 0 {
                6  // USDC
            } else {
                18 // Other tokens
            };

            let total = Decimal::from_str_exact(&balance.balance.total, decimals)
                .unwrap_or_else(|| Decimal::from_raw(0, decimals));
            let core_view = Decimal::from_str_exact(&balance.balance.core_view, decimals)
                .unwrap_or_else(|| Decimal::from_raw(0, decimals));
            let evm_view = Decimal::from_str_exact(&balance.balance.evm_view, decimals)
                .unwrap_or_else(|| Decimal::from_raw(0, decimals));

            // Set balance directly
            unified.set_balance(balance.account, balance.token, UnifiedBalance {
                total,
                core_view,
                evm_view,
            });
        }

        info!("Restored {} unified balances", state.core.balances.len());
    }

    // Restore perpetual engine state
    //
    // When perp_engine is available, restore trading state directly into it from
    // perp.* fields (with fallback to core.* for older snapshots).
    // When perp_engine is not available (tests), restore into the shared engine from core.*.
    //
    // NOTE: Use try_write() instead of blocking_write() because this function
    // is called from within a tokio async runtime (#[tokio::main]).
    // At startup there is no lock contention, so try_write() always succeeds.
    if let Some(perp) = perp_engine {
        let mut perp_eng = perp.try_write()
            .expect("PerpEngine lock should be uncontested during startup state restore");

        // Use perp.* fields (canonical), fall back to core.* for older snapshots
        let markets = if !state.perp.markets.is_empty() { &state.perp.markets } else { &state.core.markets };
        let positions = if !state.perp.positions.is_empty() { &state.perp.positions } else { &state.core.positions };
        let leverage = if !state.perp.leverage.is_empty() { &state.perp.leverage } else { &state.core.leverage };
        let orders = if !state.perp.orders.is_empty() { &state.perp.orders } else { &state.core.orders };
        let next_order_id = if state.perp.next_order_id > 0 { state.perp.next_order_id } else { state.core.next_order_id };
        let insurance_fund = if !state.perp.insurance_fund.is_empty() { &state.perp.insurance_fund } else { &state.core.insurance_fund };

        restore_engine_state(&mut perp_eng.state, markets, positions, leverage, orders, next_order_id, insurance_fund);

        info!(
            "Restored perp_engine: {} positions, {} orders, {} markets (source={})",
            positions.len(), orders.len(), markets.len(),
            if !state.perp.positions.is_empty() { "perp" } else { "core-fallback" },
        );
    }
    // Always restore shared engine markets (needed for AppHash market_root in non-perp-engine path)
    {
        let mut eng = engine.try_write()
            .expect("Engine lock should be uncontested during startup state restore");

        let markets = &state.core.markets;
        // Only restore markets into shared engine (positions/orders stay empty)
        for market_entry in markets {
            let market = build_market_from_entry(market_entry);
            eng.add_market(market);
        }

        // Only restore positions/orders/leverage into shared engine if no perp_engine
        if perp_engine.is_none() {
            restore_engine_state(
                &mut eng,
                &state.core.markets,
                &state.core.positions,
                &state.core.leverage,
                &state.core.orders,
                state.core.next_order_id,
                &state.core.insurance_fund,
            );
            info!(
                "Restored shared engine: {} positions, {} orders, {} markets",
                state.core.positions.len(), state.core.orders.len(), state.core.markets.len()
            );
        } else {
            // With perp_engine, shared engine only needs scalars for backward compat
            eng.set_next_order_id(state.core.next_order_id);
            if let Some(fund) = Decimal::from_str_exact(&state.core.insurance_fund, 6) {
                eng.set_insurance_fund(fund);
            }
            info!("Restored shared engine: {} markets (positions/orders in perp_engine)", state.core.markets.len());
        }
    }

    // Restore spot engine state
    if let Some(spot) = spot_engine {
        let mut spot_eng = spot.try_write()
            .expect("SpotEngine lock should be uncontested during startup state restore");

        // Restore tokens
        for token_entry in &state.spot.tokens {
            let max_supply = Decimal::from_str_exact(&token_entry.max_supply, token_entry.wei_decimals)
                .unwrap_or_else(|| Decimal::from_raw(0, token_entry.wei_decimals));

            // Use the new() constructor which sets up defaults
            let token = hypercore_primitives::SpotToken::new(
                token_entry.index,
                token_entry.name.clone(),
                token_entry.symbol.clone(),
                token_entry.wei_decimals,
                token_entry.sz_decimals,
                max_supply,
                token_entry.deployer,
            );
            spot_eng.state.restore_token(token);
        }

        // Restore spot markets
        // CRITICAL: Fee rates MUST be restored from persisted values for consensus safety
        // Using hardcoded values causes AppHash mismatch after node restart
        for market_entry in &state.spot.markets {
            let tick_size = Decimal::price(&market_entry.tick_size);
            let lot_size = Decimal::size(&market_entry.lot_size);

            // Restore fee rates from persisted values, with fallback for backwards compatibility
            let maker_fee_rate = if market_entry.maker_fee_rate.is_empty() {
                // Default for older snapshots without fee rates
                warn!(
                    "Spot market {} has no persisted maker_fee_rate, using default -0.0002",
                    market_entry.symbol
                );
                Decimal::rate("-0.0002") // -2 bps rebate (matches SpotMarketConfig::new_usdc_pair)
            } else {
                Decimal::rate(&market_entry.maker_fee_rate)
            };
            let taker_fee_rate = if market_entry.taker_fee_rate.is_empty() {
                // Default for older snapshots without fee rates
                warn!(
                    "Spot market {} has no persisted taker_fee_rate, using default 0.0005",
                    market_entry.symbol
                );
                Decimal::rate("0.0005") // 5 bps (matches SpotMarketConfig::new_usdc_pair)
            } else {
                Decimal::rate(&market_entry.taker_fee_rate)
            };

            debug!(
                "Restoring spot market {}: maker_fee={}, taker_fee={}",
                market_entry.symbol,
                maker_fee_rate.to_string_trimmed(),
                taker_fee_rate.to_string_trimmed()
            );

            spot_eng.state.restore_market(hypercore_primitives::SpotMarketConfig {
                id: market_entry.id as u8,
                symbol: market_entry.symbol.clone(),
                base_token: market_entry.base_token,
                quote_token: market_entry.quote_token,
                tick_size,
                lot_size,
                min_order_size: lot_size,
                max_order_size: Decimal::from_raw(1_000_000_000, 6),
                maker_fee_rate,
                taker_fee_rate,
                is_active: true,
            });
        }

        // Restore reserved balances
        for reserved in &state.spot.reserved {
            let decimals = if reserved.token == 0 { 6 } else { 18 };
            let amount = Decimal::from_str_exact(&reserved.amount, decimals)
                .unwrap_or_else(|| Decimal::from_raw(0, decimals));
            spot_eng.state.set_reserved(reserved.account, reserved.token, amount);
        }

        // Restore spot orders
        for order_entry in &state.spot.orders {
            spot_eng.state.restore_order(order_entry.market, order_entry.order.clone());
        }

        // Restore counters
        spot_eng.state.set_next_token_index(state.spot.next_token_index);
        spot_eng.state.set_next_market_id(state.spot.next_market_id as u8);
        spot_eng.state.set_next_order_id(state.spot.next_order_id);

        info!(
            "Restored {} spot tokens, {} spot markets, {} spot orders",
            state.spot.tokens.len(),
            state.spot.markets.len(),
            state.spot.orders.len()
        );
    }

    info!(
        "State restoration complete: height={}, {} balances",
        state.height,
        state.core.balances.len()
    );

    Ok((state.height, state.timestamp, state.app_hash))
}

/// Restore nonces and CLOID index into AppState
pub fn restore_chain_state(
    state: &PersistedState,
    nonces: &mut HashMap<AccountAddress, u64>,
    cloid_index: &mut HashMap<(AccountAddress, String), (MarketId, OrderId)>,
    block_hashes: &mut HashMap<u64, [u8; 32]>,
) {
    // Restore nonces
    for nonce_entry in &state.chain.nonces {
        nonces.insert(nonce_entry.account, nonce_entry.nonce);
    }

    // Restore CLOID index
    for cloid_entry in &state.chain.cloid_index {
        cloid_index.insert(
            (cloid_entry.account, cloid_entry.cloid.clone()),
            (cloid_entry.market, cloid_entry.order_id),
        );
    }

    // Restore block hashes
    for block_entry in &state.chain.blocks {
        block_hashes.insert(block_entry.height, block_entry.app_hash);
    }

    info!(
        "Restored chain state: {} nonces, {} cloids, {} block hashes",
        nonces.len(),
        cloid_index.len(),
        block_hashes.len()
    );
}

/// Restore perp_engine state from persisted data.
///
/// Called AFTER the perp_engine has been created and default markets added.
/// Restores positions, orders, leverage, detailed market state, and scalars
/// from `perp.*` fields (with fallback to `core.*` for older snapshots).
pub fn restore_perp_engine_state(
    state: &PersistedState,
    perp_engine: &SharedEngine,
) {
    let mut perp_eng = perp_engine.try_write()
        .expect("PerpEngine lock should be uncontested during startup state restore");

    // Use perp.* fields (canonical), fall back to core.* for older snapshots
    let markets = if !state.perp.markets.is_empty() { &state.perp.markets } else { &state.core.markets };
    let positions = if !state.perp.positions.is_empty() { &state.perp.positions } else { &state.core.positions };
    let leverage = if !state.perp.leverage.is_empty() { &state.perp.leverage } else { &state.core.leverage };
    let orders = if !state.perp.orders.is_empty() { &state.perp.orders } else { &state.core.orders };
    let next_order_id = if state.perp.next_order_id > 0 { state.perp.next_order_id } else { state.core.next_order_id };
    let insurance_fund = if !state.perp.insurance_fund.is_empty() { &state.perp.insurance_fund } else { &state.core.insurance_fund };

    // Restore detailed market state (mark_price, funding, OI, fee rates)
    for market_entry in markets {
        if let Some(market) = perp_eng.state.get_market_mut(market_entry.id) {
            if !market_entry.mark_price.is_empty() {
                market.state.mark_price = Decimal::price(&market_entry.mark_price);
            }
            if !market_entry.index_price.is_empty() {
                market.state.index_price = Decimal::price(&market_entry.index_price);
            }
            if !market_entry.funding_rate.is_empty() {
                market.state.funding_rate = Decimal::price(&market_entry.funding_rate);
            }
            if !market_entry.funding_accumulator.is_empty() {
                market.state.funding_accumulator = Decimal::price(&market_entry.funding_accumulator);
            }
            if !market_entry.open_interest_long.is_empty() {
                market.state.open_interest_long = Decimal::size(&market_entry.open_interest_long);
            }
            if !market_entry.open_interest_short.is_empty() {
                market.state.open_interest_short = Decimal::size(&market_entry.open_interest_short);
            }
            if market_entry.next_funding_time > 0 {
                market.state.next_funding_time = market_entry.next_funding_time;
            }
            // Restore fee rates if persisted
            if !market_entry.maker_fee_rate.is_empty() {
                market.config.maker_fee_rate = Decimal::rate(&market_entry.maker_fee_rate);
            }
            if !market_entry.taker_fee_rate.is_empty() {
                market.config.taker_fee_rate = Decimal::rate(&market_entry.taker_fee_rate);
            }
        }
    }

    // Restore positions
    for pos_entry in positions {
        perp_eng.state.set_position(pos_entry.account, pos_entry.market, pos_entry.position.clone());
    }

    // Restore leverage
    for lev_entry in leverage {
        perp_eng.state.set_leverage(lev_entry.account, lev_entry.market, lev_entry.leverage);
    }

    // Restore orders into ALL perp_engine data structures
    // (orders HashMap + orderbook BTreeMap + account_orders index)
    for order_entry in orders {
        perp_eng.state.restore_order(order_entry.order.clone());
    }

    // Restore scalars
    perp_eng.state.set_next_order_id(next_order_id);
    if let Some(fund) = Decimal::from_str_exact(insurance_fund, 6) {
        perp_eng.state.set_insurance_fund(fund);
    }

    info!(
        "Restored perp_engine: {} positions, {} orders, {} leverage, next_order_id={} (source={})",
        positions.len(), orders.len(), leverage.len(), next_order_id,
        if !state.perp.positions.is_empty() { "perp" } else { "core-fallback" },
    );
}

/// Error type for state restoration
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("Schema version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: u32, found: u32 },

    #[error("State validation failed: {0}")]
    ValidationFailed(String),

    #[error("Missing required data: {0}")]
    MissingData(String),

    #[error("Invalid data format: {0}")]
    InvalidFormat(String),
}

/// Build a Market from a persisted MarketEntry
fn build_market_from_entry(market_entry: &MarketEntry) -> hypercore_primitives::Market {
    let tick_size = Decimal::price(&market_entry.tick_size);
    let lot_size = Decimal::size(&market_entry.lot_size);

    let maker_fee_rate = if market_entry.maker_fee_rate.is_empty() {
        Decimal::rate("0.0002")
    } else {
        Decimal::rate(&market_entry.maker_fee_rate)
    };
    let taker_fee_rate = if market_entry.taker_fee_rate.is_empty() {
        Decimal::rate("0.0005")
    } else {
        Decimal::rate(&market_entry.taker_fee_rate)
    };

    let mut config = hypercore_primitives::MarketConfig::new(
        market_entry.id,
        market_entry.symbol.clone(),
        market_entry.max_leverage,
    );
    config.tick_size = tick_size;
    config.lot_size = lot_size;
    config.maker_fee_rate = maker_fee_rate;
    config.taker_fee_rate = taker_fee_rate;

    let mark_price = if market_entry.mark_price.is_empty() {
        Decimal::price("0")
    } else {
        Decimal::price(&market_entry.mark_price)
    };

    let mut market = hypercore_primitives::Market::new(
        config,
        mark_price,
        market_entry.next_funding_time,
    );

    if !market_entry.index_price.is_empty() {
        market.state.index_price = Decimal::price(&market_entry.index_price);
    }
    if !market_entry.funding_rate.is_empty() {
        market.state.funding_rate = Decimal::price(&market_entry.funding_rate);
    }
    if !market_entry.funding_accumulator.is_empty() {
        market.state.funding_accumulator = Decimal::price(&market_entry.funding_accumulator);
    }
    if !market_entry.open_interest_long.is_empty() {
        market.state.open_interest_long = Decimal::size(&market_entry.open_interest_long);
    }
    if !market_entry.open_interest_short.is_empty() {
        market.state.open_interest_short = Decimal::size(&market_entry.open_interest_short);
    }

    market
}

/// Restore trading state (markets, positions, leverage, orders, scalars) into an EngineState
fn restore_engine_state(
    eng: &mut hypercore_engine::EngineState,
    markets: &[MarketEntry],
    positions: &[PositionEntry],
    leverage: &[LeverageEntry],
    orders: &[OrderEntry],
    next_order_id: OrderId,
    insurance_fund: &str,
) {
    for market_entry in markets {
        let market = build_market_from_entry(market_entry);
        debug!(
            "Restored market {}: mark_price={}, funding_rate={}",
            market_entry.symbol,
            market.state.mark_price.to_string_trimmed(),
            market.state.funding_rate.to_string_trimmed(),
        );
        eng.add_market(market);
    }

    for pos_entry in positions {
        eng.set_position(pos_entry.account, pos_entry.market, pos_entry.position.clone());
    }

    for lev_entry in leverage {
        eng.set_leverage(lev_entry.account, lev_entry.market, lev_entry.leverage);
    }

    // Restore orders into ALL data structures
    // (orders HashMap + orderbook BTreeMap + account_orders index)
    for order_entry in orders {
        eng.restore_order(order_entry.order.clone());
    }

    eng.set_next_order_id(next_order_id);
    if let Some(fund) = Decimal::from_str_exact(insurance_fund, 6) {
        eng.set_insurance_fund(fund);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypercore_engine::EngineState;
    use hypercore_primitives::new_shared_unified_state;

    #[test]
    fn test_extract_empty_state() {
        let unified_state = new_shared_unified_state();
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let nonces = HashMap::new();
        let cloid_index = HashMap::new();
        let block_metadata = HashMap::new();

        let state = extract_state(
            1,
            1000,
            [0u8; 32],
            &unified_state,
            &engine,
            None, // perp_engine
            None, // spot_engine
            &nonces,
            &cloid_index,
            &block_metadata,
        );

        assert_eq!(state.height, 1);
        assert_eq!(state.timestamp, 1000);
        assert!(state.core.balances.is_empty());
    }

    #[test]
    fn test_extract_with_balances() {
        let unified_state = new_shared_unified_state();
        let engine = Arc::new(RwLock::new(EngineState::new()));

        // Add a balance
        {
            let mut unified = unified_state.write().unwrap();
            let account = AccountAddress::from([0x42u8; 20]);
            unified.credit(account, 0, Decimal::from_raw(100_000_000, 6));
        }

        let nonces = HashMap::new();
        let cloid_index = HashMap::new();
        let block_metadata = HashMap::new();

        let state = extract_state(
            1,
            1000,
            [0u8; 32],
            &unified_state,
            &engine,
            None, // perp_engine
            None, // spot_engine
            &nonces,
            &cloid_index,
            &block_metadata,
        );

        assert_eq!(state.core.balances.len(), 1);
        assert_eq!(state.core.balances[0].balance.total, "100");
    }
}
