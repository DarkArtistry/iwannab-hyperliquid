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

use crate::state::{AppState, BlockMeta, SharedEngineState, SharedSpotEngine};

/// Extract complete state from runtime components into PersistedState
pub fn extract_state(
    height: u64,
    timestamp: u64,
    app_hash: [u8; 32],
    unified_state: &SharedUnifiedState,
    engine: &SharedEngineState,
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
                },
            });
        }
    }

    // Extract perpetual engine state
    {
        let eng = engine.blocking_read();

        // Extract positions
        for (account, positions) in eng.get_all_positions_global() {
            for (market_id, position) in positions {
                state.core.positions.push(PositionEntry {
                    account: *account,
                    market: *market_id,
                    position: position.clone(),
                });
            }
        }

        // Extract leverage settings
        for (account, leverages) in eng.get_all_leverage_global() {
            for (market_id, leverage) in leverages {
                state.core.leverage.push(LeverageEntry {
                    account: *account,
                    market: *market_id,
                    leverage: *leverage,
                });
            }
        }

        // Extract markets
        for market in eng.get_all_markets() {
            state.core.markets.push(MarketEntry {
                id: market.id(),
                symbol: market.config.symbol.clone(),
                max_leverage: market.config.max_leverage,
                tick_size: market.config.tick_size.to_string_trimmed(),
                lot_size: market.config.lot_size.to_string_trimmed(),
            });
        }

        // Extract orders from orderbooks
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
        let spot_eng = spot.blocking_read();

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
        for market in spot_eng.state.get_all_markets() {
            state.spot.markets.push(SpotMarketEntry {
                id: market.config.id as u32,
                symbol: market.config.symbol.clone(),
                base_token: market.config.base_token,
                quote_token: market.config.quote_token,
                tick_size: market.config.tick_size.to_string_trimmed(),
                lot_size: market.config.lot_size.to_string_trimmed(),
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
/// Returns the height, timestamp, and app_hash that were restored.
pub fn restore_state(
    state: &PersistedState,
    unified_state: &SharedUnifiedState,
    engine: &SharedEngineState,
    spot_engine: Option<&SharedSpotEngine>,
) -> Result<(u64, u64, [u8; 32]), RestoreError> {
    info!("Restoring state from height {}", state.height);

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
            // Determine decimals based on token
            let decimals = if balance.token == 0 { 6 } else { 18 };

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
    {
        let mut eng = engine.blocking_write();

        // Restore markets
        for market_entry in &state.core.markets {
            let tick_size = Decimal::price(&market_entry.tick_size);
            let lot_size = Decimal::size(&market_entry.lot_size);

            // Use new() which sets sensible defaults
            let mut config = hypercore_primitives::MarketConfig::new(
                market_entry.id,
                market_entry.symbol.clone(),
                market_entry.max_leverage,
            );
            config.tick_size = tick_size;
            config.lot_size = lot_size;

            let market = hypercore_primitives::Market::new(
                config,
                Decimal::price("0"), // Mark price will be updated by price feed
                0,
            );
            eng.add_market(market);
        }

        // Restore positions
        for pos_entry in &state.core.positions {
            eng.set_position(pos_entry.account, pos_entry.market, pos_entry.position.clone());
        }

        // Restore leverage
        for lev_entry in &state.core.leverage {
            eng.set_leverage(lev_entry.account, lev_entry.market, lev_entry.leverage);
        }

        // Restore orders
        for order_entry in &state.core.orders {
            if let Some(orderbook) = eng.get_orderbook_mut(order_entry.market) {
                orderbook.restore_order(order_entry.order.clone());
            }
        }

        // Restore next_order_id
        eng.set_next_order_id(state.core.next_order_id);

        // Restore insurance fund
        if let Some(fund) = Decimal::from_str_exact(&state.core.insurance_fund, 6) {
            eng.set_insurance_fund(fund);
        }

        info!(
            "Restored {} positions, {} orders, {} markets",
            state.core.positions.len(),
            state.core.orders.len(),
            state.core.markets.len()
        );
    }

    // Restore spot engine state
    if let Some(spot) = spot_engine {
        let mut spot_eng = spot.blocking_write();

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
        for market_entry in &state.spot.markets {
            let tick_size = Decimal::price(&market_entry.tick_size);
            let lot_size = Decimal::size(&market_entry.lot_size);

            spot_eng.state.restore_market(hypercore_primitives::SpotMarketConfig {
                id: market_entry.id as u8,
                symbol: market_entry.symbol.clone(),
                base_token: market_entry.base_token,
                quote_token: market_entry.quote_token,
                tick_size,
                lot_size,
                min_order_size: lot_size,
                max_order_size: Decimal::from_raw(1_000_000_000, 6),
                maker_fee_rate: Decimal::from_raw(10, 6), // 0.001%
                taker_fee_rate: Decimal::from_raw(20, 6), // 0.002%
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
            None,
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
            None,
            &nonces,
            &cloid_index,
            &block_metadata,
        );

        assert_eq!(state.core.balances.len(), 1);
        assert_eq!(state.core.balances[0].balance.total, "100");
    }
}
