//! HyperCore application logic
//!
//! Phase 2B: Full implementation with transaction handlers connected to unified state.
//! Perpetual order execution now goes through the full Engine orchestrator with
//! matching, risk checks, and position management.

use hypercore_primitives::{
    AccountAddress, BlockEvent, BlockHeight, Decimal, FillEvent,
    FundingAppliedEvent, LiquidationEvent, MarketId,
    OrderId, OrderPlacedEvent, OrderSide,
    SharedUnifiedState, Timestamp,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::state::{AppState, BlockMeta, SharedEngineState, SharedSpotEngine};
use crate::tx::{OrderWire, Transaction, TransactionType};

/// Acquire a write lock on a tokio RwLock with retry.
///
/// In CometBFT mode, `FinalizeBlock` runs on a `spawn_blocking` thread.
/// `try_write()` can fail if a gateway info handler concurrently holds a read lock.
/// This helper spins briefly then sleeps 1ms per attempt for up to 100 attempts (~90ms max),
/// which is sufficient for any gateway read to complete.
///
/// Safe in both async (BlockProducer - lock is uncontested) and sync (ABCI - retries
/// until gateway read finishes) contexts.
fn acquire_write_with_retry<'a, T>(lock: &'a RwLock<T>, label: &str) -> Result<tokio::sync::RwLockWriteGuard<'a, T>, AppError> {
    // First try without sleeping (covers the uncontested case)
    if let Ok(guard) = lock.try_write() {
        return Ok(guard);
    }

    // Retry with short sleeps - gateway reads finish quickly
    for attempt in 1..=100 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        if let Ok(guard) = lock.try_write() {
            if attempt > 5 {
                tracing::debug!("{}: acquired write lock after {} retries", label, attempt);
            }
            return Ok(guard);
        }
    }

    tracing::error!("{}: failed to acquire write lock after 100 retries", label);
    Err(AppError::Internal(format!("{} lock contention (100 retries exhausted)", label)))
}

/// Acquire a read lock on a tokio RwLock with retry.
///
/// Similar to acquire_write_with_retry, but for read locks.
/// CRITICAL: This is needed for CLOID cleanup during FinalizeBlock. Using try_read()
/// could fail non-deterministically (e.g., if a write lock is held by another component
/// on some nodes but not others), causing state divergence.
fn acquire_read_with_retry<'a, T>(lock: &'a RwLock<T>, label: &str) -> Result<tokio::sync::RwLockReadGuard<'a, T>, AppError> {
    // First try without sleeping (covers the uncontested case)
    if let Ok(guard) = lock.try_read() {
        return Ok(guard);
    }

    // Retry with short sleeps - writes should finish quickly
    for attempt in 1..=100 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        if let Ok(guard) = lock.try_read() {
            if attempt > 5 {
                tracing::debug!("{}: acquired read lock after {} retries", label, attempt);
            }
            return Ok(guard);
        }
    }

    tracing::error!("{}: failed to acquire read lock after 100 retries", label);
    Err(AppError::Internal(format!("{} lock contention (100 retries exhausted)", label)))
}

/// HyperCore application
pub struct HyperCoreApp {
    /// Application state
    pub state: AppState,
}

impl Default for HyperCoreApp {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperCoreApp {
    /// Create new application with default state
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
        }
    }

    /// Create application with shared components (for node integration)
    pub fn with_shared_state(
        unified_state: SharedUnifiedState,
        engine: SharedEngineState,
        spot_engine: Option<SharedSpotEngine>,
    ) -> Self {
        Self {
            state: AppState::with_shared_state(unified_state, engine, spot_engine),
        }
    }

    /// Initialize from genesis state
    ///
    /// This is called during ABCI InitChain. All validators must execute this
    /// identically to produce the same initial state.
    ///
    /// # Genesis State Processing Order
    /// 1. Initialize perpetual markets (BTC-PERP, ETH-PERP, etc.)
    /// 2. Initialize spot tokens (TEST, etc. - USDC is implicit at index 0)
    /// 3. Credit account balances to appropriate views (core or evm)
    pub fn init_from_genesis(&mut self, genesis_bytes: &[u8]) -> Result<(), AppError> {
        let genesis: GenesisState = serde_json::from_slice(genesis_bytes)
            .map_err(|e| AppError::GenesisError(format!("Failed to parse genesis: {}", e)))?;

        tracing::info!(
            "Initializing from genesis: chain_id={}, markets={}, tokens={}, balances={}",
            genesis.chain_id,
            genesis.markets.len(),
            genesis.spot_tokens.len(),
            genesis.balances.len()
        );

        // 1. Initialize perpetual markets
        if !genesis.markets.is_empty() {
            let mut engine = acquire_write_with_retry(&self.state.engine, "Engine (genesis init)")?;
            for market in &genesis.markets {
                let m: hypercore_primitives::Market = market.clone().into();
                engine.add_market(m);
                tracing::info!("Genesis: Added perpetual market {} ({})", market.symbol, market.id);
            }
        }

        // 2. Initialize spot tokens
        // Note: Spot tokens require the SpotEngine to be available
        // For single-node mode, this is handled in node/main.rs
        // For CometBFT mode, tokens are initialized here if spot_engine is available
        if !genesis.spot_tokens.is_empty() {
            if let Some(ref spot_engine) = self.state.spot_engine {
                let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (genesis init)")?;
                let deployer = AccountAddress::from([0u8; 20]); // System deployer
                // Use genesis timestamp (0) for deterministic genesis state
                // CRITICAL: Do not use SystemTime::now() - it breaks consensus determinism
                // All validators must produce identical genesis state
                let genesis_timestamp: u64 = 0;

                for token in &genesis.spot_tokens {
                    let params = hypercore_primitives::SpotTokenDeployParams {
                        name: token.name.clone(),
                        symbol: token.symbol.clone(),
                        wei_decimals: token.wei_decimals,
                        sz_decimals: token.sz_decimals,
                        max_supply: Decimal::from_str(&token.max_supply)
                            .unwrap_or_else(|_| Decimal::from_raw(1_000_000_000_000_000_000_000_000_000i128, 18)),
                        genesis_allocations: vec![],
                        erc20_address: None,
                    };

                    match engine.deploy_token(params, deployer, genesis_timestamp) {
                        Ok(deployed) => {
                            tracing::info!(
                                "Genesis: Deployed spot token {} at index {}",
                                deployed.symbol,
                                deployed.index
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Genesis: Failed to deploy token {}: {}", token.symbol, e);
                        }
                    }
                }
            } else {
                tracing::debug!("Genesis: Spot engine not available, skipping token deployment");
            }
        }

        // 3. Initialize account balances with view allocation
        if !genesis.balances.is_empty() {
            let mut unified = self.state.unified_state.write().unwrap();

            for balance in &genesis.balances {
                // Determine decimals based on token index
                // Token 0 (USDC) = 6 decimals, others = 18 decimals (configurable)
                let decimals = if balance.token == 0 { 6 } else { 18 };

                // IMPORTANT: Use from_str_exact with correct decimals
                // from_str() uses PRICE_DECIMALS (8) which causes decimal mismatch
                let amount = Decimal::from_str_exact(&balance.amount, decimals)
                    .unwrap_or_else(|| {
                        // Try parsing as raw integer
                        if let Ok(raw) = balance.amount.parse::<i128>() {
                            Decimal::from_raw(raw, decimals)
                        } else {
                            tracing::warn!(
                                "Genesis: Invalid amount '{}' for {:?}, skipping",
                                balance.amount,
                                balance.address
                            );
                            Decimal::from_raw(0, decimals)
                        }
                    });

                if amount.raw() == 0 {
                    continue;
                }

                // Credit to appropriate view
                match balance.view {
                    BalanceView::Core => {
                        unified.credit(balance.address, balance.token, amount);
                        tracing::debug!(
                            "Genesis: Credited {:?} token {} amount {} to core_view",
                            balance.address,
                            balance.token,
                            amount.to_string_trimmed()
                        );
                    }
                    BalanceView::Evm => {
                        unified.credit_evm(balance.address, balance.token, amount);
                        tracing::debug!(
                            "Genesis: Credited {:?} token {} amount {} to evm_view",
                            balance.address,
                            balance.token,
                            amount.to_string_trimmed()
                        );
                    }
                }
            }

            tracing::info!(
                "Genesis: Initialized {} balance entries",
                genesis.balances.len()
            );
        }

        Ok(())
    }

    /// Check transaction validity (for mempool admission)
    ///
    /// Uses current system time for timestamp-based nonce validation.
    /// This is acceptable for CheckTx since mempool admission is NOT consensus-critical.
    /// Each node can independently decide which transactions to accept into its mempool.
    pub fn check_tx(&self, tx: &Transaction) -> Result<(), AppError> {
        // Get current time for mempool validation
        // Note: This is NOT consensus-critical - each node validates mempool independently
        let current_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Basic validation with current time as reference
        self.state.validate_tx(tx, current_time_ms)?;
        Ok(())
    }

    /// Execute a transaction (sync version for ABCI)
    ///
    /// This is the main entry point for transaction execution from the ABCI layer.
    /// Uses blocking operations internally to work in sync context.
    ///
    /// # Determinism
    /// This function is deterministic: given the same transaction and timestamp,
    /// all validators will produce identical results. The timestamp parameter
    /// is the consensus-agreed block timestamp, ensuring all nodes use the same
    /// reference time for timestamp-based nonce validation.
    pub fn execute_tx(&mut self, tx: &Transaction, timestamp: Timestamp) -> Result<TxResult, AppError> {
        // Validate using block timestamp for deterministic nonce validation
        // CRITICAL: Must use block timestamp, not SystemTime::now(), for consensus
        self.state.validate_tx(tx, timestamp)?;

        let sender = tx.sender()?;

        // Process based on transaction type
        let events = match &tx.action {
            TransactionType::Order { orders, grouping: _ } => {
                self.execute_orders_sync(sender, orders, timestamp)?
            }

            TransactionType::Cancel { cancels } => {
                self.execute_cancels_sync(sender, cancels)?
            }

            TransactionType::CancelByCloid { cancels } => {
                self.execute_cancel_by_cloid_sync(sender, cancels)?
            }

            TransactionType::CancelAll => {
                self.execute_cancel_all_sync(sender)?
            }

            TransactionType::UpdateLeverage { asset, is_cross: _, leverage } => {
                self.execute_update_leverage_sync(sender, *asset, *leverage)?
            }

            TransactionType::UsdTransfer { destination, amount } => {
                self.execute_usd_transfer_sync(sender, *destination, amount)?
            }

            TransactionType::Withdraw { destination, amount } => {
                self.execute_withdraw_sync(sender, *destination, amount)?
            }

            TransactionType::EvmAction { action_type, data } => {
                self.execute_evm_action_sync(sender, *action_type, data)?
            }

            TransactionType::SpotOrder { orders, grouping: _ } => {
                self.execute_orders_sync(sender, orders, timestamp)?
            }

            TransactionType::SpotCancelAll { market } => {
                self.execute_spot_cancel_all_sync(sender, *market)?
            }

            TransactionType::ViewTransfer { token, amount, to_evm } => {
                self.execute_view_transfer_sync(sender, *token, amount, *to_evm)?
            }

            TransactionType::SetMarginMode { mode } => {
                self.execute_set_margin_mode_sync(sender, *mode)?
            }

            TransactionType::CancelTrigger { asset, order_id } => {
                self.execute_cancel_trigger_sync(sender, *asset, *order_id)?
            }
        };

        // Update nonces (supports both sequential and timestamp-based)
        self.state.increment_nonce(sender);
        self.state.update_timestamp_nonce(sender, tx.nonce);

        Ok(TxResult {
            success: true,
            events,
            gas_used: 1, // Simplified gas accounting
        })
    }

    /// Execute orders (sync version using blocking locks)
    fn execute_orders_sync(
        &mut self,
        sender: AccountAddress,
        orders: &[OrderWire],
        timestamp: Timestamp,
    ) -> Result<Vec<Event>, AppError> {
        use hypercore_primitives::OrderId;

        let mut events = Vec::new();
        // Collect CLOIDs to register after releasing engine locks
        let mut cloids_to_register: Vec<(String, MarketId, OrderId)> = Vec::new();
        // Collect order IDs that were fully filled (for CLOID cleanup)
        let mut filled_order_cloids: Vec<(MarketId, OrderId)> = Vec::new();

        for order_wire in orders {
            let market_id = order_wire.a;

            // Market IDs >= 128 are spot markets, < 128 are perps
            if market_id >= 128 {
                // Spot order - execute and collect result
                let order_request = order_wire.to_order_request(sender)
                    .map_err(|e| AppError::TxError(e))?;

                let result = if let Some(spot_engine) = &self.state.spot_engine {
                    let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (place_order)")?;
                    engine.place_order(sender, market_id, order_request, timestamp)
                } else {
                    continue;
                };

                match result {
                    Ok((order, fills)) => {
                        // Only register CLOID if order is resting (not fully filled immediately)
                        if let Some(ref cloid) = order.client_order_id {
                            if !order.is_filled() && !order.remaining_size.is_zero() {
                                cloids_to_register.push((cloid.clone(), market_id, order.id));
                            }
                        }

                        // Create BlockEvent::Fill for each spot fill
                        // Also track maker orders that may have been fully filled
                        let mut spot_filled_maker_orders: Vec<(MarketId, OrderId)> = Vec::new();
                        for fill in &fills {
                            let trade_id = self.state.next_trade_id();

                            // Track maker order for CLOID cleanup check
                            spot_filled_maker_orders.push((fill.market_id, fill.maker_order_id));

                            // Maker side: opposite of taker
                            // If taker is buying, maker is selling
                            let maker_side = if fill.is_taker_buy { OrderSide::Sell } else { OrderSide::Buy };
                            let maker_event = BlockEvent::Fill(FillEvent {
                                trade_id,
                                order_id: fill.maker_order_id,
                                account: fill.maker,
                                market_id: fill.market_id,
                                side: maker_side,
                                price: fill.price,
                                size: fill.base_size,
                                fee: fill.maker_fee,
                                is_taker: false,
                                realized_pnl: 0,
                                timestamp: fill.timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&maker_event) {
                                self.state.add_pending_block_event(json);
                            }

                            // Taker side
                            let taker_side = if fill.is_taker_buy { OrderSide::Buy } else { OrderSide::Sell };
                            let taker_event = BlockEvent::Fill(FillEvent {
                                trade_id,
                                order_id: fill.taker_order_id,
                                account: fill.taker,
                                market_id: fill.market_id,
                                side: taker_side,
                                price: fill.price,
                                size: fill.base_size,
                                fee: fill.taker_fee,
                                is_taker: true,
                                realized_pnl: 0,
                                timestamp: fill.timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&taker_event) {
                                self.state.add_pending_block_event(json);
                            }
                        }

                        // Clean up CLOIDs for fully filled spot maker orders
                        if !spot_filled_maker_orders.is_empty() {
                            if let Some(spot_engine) = &self.state.spot_engine {
                                if let Ok(engine) = spot_engine.try_read() {
                                    for (mkt_id, oid) in spot_filled_maker_orders {
                                        // Check if order still exists
                                        let order_exists = engine.state.get_order(mkt_id, oid).is_some();
                                        if !order_exists {
                                            // Order was fully filled, schedule CLOID removal
                                            filled_order_cloids.push((mkt_id, oid));
                                        }
                                    }
                                }
                            }
                        }

                        // Emit OrderPlaced event for resting spot order
                        if !order.is_filled() && !order.remaining_size.is_zero() {
                            let order_placed = BlockEvent::OrderPlaced(OrderPlacedEvent {
                                order_id: order.id,
                                account: sender,
                                market_id,
                                side: order.side,
                                price: order.price,
                                size: order.original_size,
                                reduce_only: false, // Spot orders don't have reduce_only
                                client_order_id: order.client_order_id.clone(),
                                timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&order_placed) {
                                self.state.add_pending_block_event(json);
                            }
                        }

                        events.push(Event::new("spot_order")
                            .add_attribute("order_id", &order.id.to_string())
                            .add_attribute("sender", &format!("{:?}", sender))
                            .add_attribute("market_id", &market_id.to_string())
                            .add_attribute("fills", &fills.len().to_string()));
                    }
                    Err(e) => {
                        tracing::warn!("Spot order failed: {}", e);
                        events.push(Event::new("spot_order_failed")
                            .add_attribute("error", &e.to_string())
                            .add_attribute("market_id", &market_id.to_string()));
                    }
                }
            } else {
                // Perpetual order - execute through full Engine
                let order_request = order_wire.to_order_request(sender)
                    .map_err(|e| AppError::TxError(e))?;

                let result = if let Some(perp_engine) = &self.state.perp_engine {
                    let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (place_order)")?;
                    Some(engine.place_order(sender, order_request, timestamp))
                } else {
                    None
                };

                match result {
                    Some(Ok((order, fills))) => {
                        // Only register CLOID if order is resting (not fully filled immediately)
                        if let Some(ref cloid) = order.client_order_id {
                            if !order.is_filled() && order.should_rest() {
                                cloids_to_register.push((cloid.clone(), market_id, order.id));
                            }
                            // Note: If taker order was fully filled, we don't register its CLOID
                            // because there's no order to look up by CLOID anymore
                        }

                        // Create BlockEvent::Fill for each fill (both maker and taker)
                        // Also track maker orders that may have been fully filled
                        let mut filled_maker_orders: Vec<(MarketId, OrderId)> = Vec::new();
                        for fill in &fills {
                            let trade_id = self.state.next_trade_id();
                            let fill_price = Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS);
                            let fill_size = Decimal::from_raw(fill.size as i128, Decimal::SIZE_DECIMALS);

                            // Track maker order for CLOID cleanup check
                            filled_maker_orders.push((fill.market_id, fill.maker_order_id));

                            // Maker fill event
                            let maker_side = if fill.is_taker_buy { OrderSide::Sell } else { OrderSide::Buy };
                            let maker_event = BlockEvent::Fill(FillEvent {
                                trade_id,
                                order_id: fill.maker_order_id,
                                account: fill.maker,
                                market_id: fill.market_id,
                                side: maker_side,
                                price: fill_price,
                                size: fill_size,
                                fee: Decimal::from_raw(fill.maker_fee.abs(), Decimal::USDC_DECIMALS),
                                is_taker: false,
                                realized_pnl: fill.realized_pnl_maker,
                                timestamp: fill.timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&maker_event) {
                                self.state.add_pending_block_event(json);
                            }

                            // Taker fill event
                            let taker_side = if fill.is_taker_buy { OrderSide::Buy } else { OrderSide::Sell };
                            let taker_event = BlockEvent::Fill(FillEvent {
                                trade_id,
                                order_id: fill.taker_order_id,
                                account: fill.taker,
                                market_id: fill.market_id,
                                side: taker_side,
                                price: fill_price,
                                size: fill_size,
                                fee: Decimal::from_raw(fill.taker_fee as i128, Decimal::USDC_DECIMALS),
                                is_taker: true,
                                realized_pnl: fill.realized_pnl_taker,
                                timestamp: fill.timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&taker_event) {
                                self.state.add_pending_block_event(json);
                            }

                            // Keep generic event for backward compatibility
                            events.push(Event::new("perp_fill")
                                .add_attribute("maker", &format!("{:?}", fill.maker))
                                .add_attribute("taker", &format!("{:?}", fill.taker))
                                .add_attribute("price", &fill.price.to_string())
                                .add_attribute("size", &fill.size.to_string())
                                .add_attribute("market_id", &fill.market_id.to_string()));
                        }

                        // Clean up CLOIDs for fully filled maker orders
                        // If the order is no longer in the orderbook, it was fully filled
                        // CRITICAL: We MUST use blocking_read() here, not try_read(), because
                        // try_read() can fail non-deterministically (e.g., RPC handler holds lock
                        // on some nodes but not others), causing CLOID state divergence.
                        if !filled_maker_orders.is_empty() {
                            if let Some(perp_engine) = &self.state.perp_engine {
                                let engine = acquire_read_with_retry(perp_engine, "Perp engine (CLOID cleanup)")?;
                                for (mkt_id, oid) in filled_maker_orders {
                                    // Check if order still exists in orderbook
                                    let order_exists = engine.state
                                        .get_orderbook(mkt_id)
                                        .map(|book| book.get(oid).is_some())
                                        .unwrap_or(false);
                                    if !order_exists {
                                        // Order was fully filled, schedule CLOID removal
                                        filled_order_cloids.push((mkt_id, oid));
                                    }
                                }
                            }
                        }

                        // Emit OrderPlaced event for resting order
                        if !order.is_filled() && order.should_rest() {
                            let order_placed = BlockEvent::OrderPlaced(OrderPlacedEvent {
                                order_id: order.id,
                                account: sender,
                                market_id,
                                side: order.side,
                                price: order.price,
                                size: order.original_size,
                                reduce_only: order.reduce_only,
                                client_order_id: order.client_order_id.clone(),
                                timestamp,
                            });
                            if let Ok(json) = serde_json::to_value(&order_placed) {
                                self.state.add_pending_block_event(json);
                            }
                        }

                        events.push(Event::new("perp_order")
                            .add_attribute("order_id", &order.id.to_string())
                            .add_attribute("sender", &format!("{:?}", sender))
                            .add_attribute("market_id", &market_id.to_string())
                            .add_attribute("fills", &fills.len().to_string())
                            .add_attribute("remaining", &order.remaining_size.to_string_trimmed()));
                    }
                    Some(Err(e)) => {
                        tracing::warn!("Perp order failed: {}", e);
                        events.push(Event::new("perp_order_failed")
                            .add_attribute("error", &e.to_string())
                            .add_attribute("market_id", &market_id.to_string()));
                    }
                    None => {
                        events.push(Event::new("perp_order_skipped")
                            .add_attribute("sender", &format!("{:?}", sender))
                            .add_attribute("market_id", &market_id.to_string())
                            .add_attribute("reason", "no_perp_engine"));
                    }
                }
            }
        }

        // Register CLOIDs after releasing engine locks
        for (cloid, market_id, order_id) in cloids_to_register {
            self.state.register_cloid(sender, cloid, market_id, order_id);
        }

        // Remove CLOIDs for fully filled orders (makers that were matched)
        // This is critical for state determinism: if a maker order with a CLOID is fully
        // filled during matching, we must remove its CLOID to prevent state divergence
        // between nodes that process different transactions at different times.
        for (market_id, order_id) in filled_order_cloids {
            self.state.remove_cloid_by_order(market_id, order_id);
        }

        Ok(events)
    }

    /// Execute cancel orders (sync version)
    fn execute_cancels_sync(
        &mut self,
        sender: AccountAddress,
        cancels: &[crate::tx::CancelWire],
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();
        let mut cloids_to_remove: Vec<String> = Vec::new();

        for cancel in cancels {
            let market_id = cancel.a;
            let order_id = cancel.o;

            if market_id >= 128 {
                // Spot cancel - get result then process
                let result = if let Some(spot_engine) = &self.state.spot_engine {
                    let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (cancel_order)")?;
                    engine.cancel_order(sender, market_id, order_id)
                } else {
                    continue;
                };

                match result {
                    Ok(order) => {
                        if let Some(ref cloid) = order.client_order_id {
                            cloids_to_remove.push(cloid.clone());
                        }
                        events.push(Event::new("spot_cancel")
                            .add_attribute("order_id", &order_id.to_string())
                            .add_attribute("market_id", &market_id.to_string()));
                    }
                    Err(e) => {
                        tracing::warn!("Spot cancel failed: {}", e);
                        events.push(Event::new("spot_cancel_failed")
                            .add_attribute("order_id", &order_id.to_string())
                            .add_attribute("error", &e.to_string()));
                    }
                }
            } else {
                // Perp cancel - get result then process
                let result = if let Some(perp_engine) = &self.state.perp_engine {
                    let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (cancel_order)")?;
                    Some(engine.cancel_order(sender, market_id, order_id))
                } else {
                    None
                };

                match result {
                    Some(Ok(order)) => {
                        if let Some(ref cloid) = order.client_order_id {
                            cloids_to_remove.push(cloid.clone());
                        }
                        events.push(Event::new("perp_cancel")
                            .add_attribute("order_id", &order_id.to_string())
                            .add_attribute("market_id", &market_id.to_string()));
                    }
                    Some(Err(e)) => {
                        tracing::warn!("Perp cancel failed: {}", e);
                        events.push(Event::new("perp_cancel_failed")
                            .add_attribute("order_id", &order_id.to_string())
                            .add_attribute("error", &e.to_string()));
                    }
                    None => {
                        events.push(Event::new("perp_cancel_skipped")
                            .add_attribute("order_id", &order_id.to_string())
                            .add_attribute("reason", "no_perp_engine"));
                    }
                }
            }
        }

        // Remove CLOIDs after releasing engine locks
        for cloid in cloids_to_remove {
            self.state.remove_cloid(sender, &cloid);
        }

        Ok(events)
    }

    /// Execute cancel by client order ID (sync version)
    fn execute_cancel_by_cloid_sync(
        &mut self,
        sender: AccountAddress,
        cancels: &[crate::tx::CancelByCloidWire],
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();
        let mut cloids_to_remove: Vec<String> = Vec::new();

        for cancel in cancels {
            // Look up order by client order ID
            let order_lookup = self.state.get_order_by_cloid(sender, &cancel.cloid);

            if let Some((market_id, order_id)) = order_lookup {
                // Verify market matches
                if market_id != cancel.asset {
                    events.push(Event::new("cancel_cloid_failed")
                        .add_attribute("cloid", &cancel.cloid)
                        .add_attribute("error", "market_mismatch"));
                    continue;
                }

                // Execute the cancel
                if market_id >= 128 {
                    // Spot cancel
                    let result = if let Some(spot_engine) = &self.state.spot_engine {
                        let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (cancel_by_cloid)")?;
                        engine.cancel_order(sender, market_id, order_id)
                    } else {
                        continue;
                    };

                    match result {
                        Ok(_) => {
                            cloids_to_remove.push(cancel.cloid.clone());
                            events.push(Event::new("spot_cancel_by_cloid")
                                .add_attribute("cloid", &cancel.cloid)
                                .add_attribute("order_id", &order_id.to_string())
                                .add_attribute("market_id", &market_id.to_string()));
                        }
                        Err(e) => {
                            events.push(Event::new("cancel_cloid_failed")
                                .add_attribute("cloid", &cancel.cloid)
                                .add_attribute("error", &e.to_string()));
                        }
                    }
                } else {
                    // Perp cancel
                    let result = if let Some(perp_engine) = &self.state.perp_engine {
                        let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (cancel_by_cloid)")?;
                        engine.cancel_order(sender, market_id, order_id)
                    } else {
                        continue;
                    };

                    match result {
                        Ok(_) => {
                            cloids_to_remove.push(cancel.cloid.clone());
                            events.push(Event::new("perp_cancel_by_cloid")
                                .add_attribute("cloid", &cancel.cloid)
                                .add_attribute("order_id", &order_id.to_string())
                                .add_attribute("market_id", &market_id.to_string()));
                        }
                        Err(e) => {
                            events.push(Event::new("cancel_cloid_failed")
                                .add_attribute("cloid", &cancel.cloid)
                                .add_attribute("error", &e.to_string()));
                        }
                    }
                }
            } else {
                events.push(Event::new("cancel_cloid_not_found")
                    .add_attribute("cloid", &cancel.cloid)
                    .add_attribute("market_id", &cancel.asset.to_string()));
            }
        }

        // Remove CLOIDs after releasing engine locks
        for cloid in cloids_to_remove {
            self.state.remove_cloid(sender, &cloid);
        }

        Ok(events)
    }

    /// Execute cancel all orders (sync version)
    fn execute_cancel_all_sync(
        &mut self,
        sender: AccountAddress,
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();
        let mut spot_cancelled = 0;
        let mut perp_cancelled = 0;
        let mut cloids_to_remove: Vec<String> = Vec::new();

        // Cancel all spot orders (iterate over spot markets: 128-255)
        if let Some(spot_engine) = &self.state.spot_engine {
            let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (cancel_all)")?;
            let markets: Vec<_> = engine.state.get_all_markets().iter().map(|m| m.config.id).collect();
            for market_id in markets {
                if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                    for order in &canceled {
                        if let Some(ref cloid) = order.client_order_id {
                            cloids_to_remove.push(cloid.clone());
                        }
                    }
                    spot_cancelled += canceled.len();
                }
            }
        }

        // Cancel all perp orders
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (cancel_all)")?;
            // Get all perpetual markets (0-127)
            let markets: Vec<_> = engine.state.get_all_markets().iter().map(|m| m.id()).collect();
            for market_id in markets {
                if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                    for order in &canceled {
                        if let Some(ref cloid) = order.client_order_id {
                            cloids_to_remove.push(cloid.clone());
                        }
                    }
                    perp_cancelled += canceled.len();
                }
            }
        }

        // Remove all collected CLOIDs after releasing engine locks
        for cloid in cloids_to_remove {
            self.state.remove_cloid(sender, &cloid);
        }

        events.push(Event::new("cancel_all")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("spot_cancelled", &spot_cancelled.to_string())
            .add_attribute("perp_cancelled", &perp_cancelled.to_string()));

        Ok(events)
    }

    /// Execute spot cancel all (sync version)
    fn execute_spot_cancel_all_sync(
        &mut self,
        sender: AccountAddress,
        market: Option<u8>,
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();
        let mut cancelled = 0;

        if let Some(spot_engine) = &self.state.spot_engine {
            let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (spot_cancel_all)")?;

            if let Some(market_id) = market {
                if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                    cancelled += canceled.len();
                }
            } else {
                let markets: Vec<_> = engine.state.get_all_markets().iter().map(|m| m.config.id).collect();
                for market_id in markets {
                    if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                        cancelled += canceled.len();
                    }
                }
            }
        }

        events.push(Event::new("spot_cancel_all")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("cancelled", &cancelled.to_string()));

        Ok(events)
    }

    /// Execute view transfer (sync version)
    fn execute_view_transfer_sync(
        &mut self,
        sender: AccountAddress,
        token: u8,
        amount: &str,
        to_evm: bool,
    ) -> Result<Vec<Event>, AppError> {
        if let Some(spot_engine) = &self.state.spot_engine {
            let mut engine = acquire_write_with_retry(spot_engine, "Spot engine (view_transfer)")?;

            let token_info = engine.state.get_token(token)
                .ok_or(AppError::Internal(format!("Token {} not found", token)))?;
            let decimal_amount = Decimal::from_str_exact(amount, token_info.wei_decimals)
                .ok_or(AppError::Internal(format!("Invalid amount: {}", amount)))?;

            engine.view_transfer(sender, token, decimal_amount, to_evm)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            Ok(vec![Event::new("view_transfer")
                .add_attribute("sender", &format!("{:?}", sender))
                .add_attribute("token", &token.to_string())
                .add_attribute("amount", amount)
                .add_attribute("to_evm", &to_evm.to_string())])
        } else {
            Err(AppError::Internal("Spot engine not available".to_string()))
        }
    }

    /// Execute leverage update (sync version)
    fn execute_update_leverage_sync(
        &mut self,
        sender: AccountAddress,
        asset: MarketId,
        leverage: u8,
    ) -> Result<Vec<Event>, AppError> {
        // Use full Engine for validation and state update
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (update_leverage)")?;
            match engine.update_leverage(sender, asset, leverage) {
                Ok(()) => {
                    Ok(vec![Event::new("update_leverage")
                        .add_attribute("asset", &asset.to_string())
                        .add_attribute("leverage", &leverage.to_string())])
                }
                Err(e) => {
                    Err(AppError::Internal(format!("Leverage update failed: {}", e)))
                }
            }
        } else {
            // Fallback to EngineState for legacy compatibility
            let mut engine = acquire_write_with_retry(&self.state.engine, "Engine (update_leverage fallback)")?;
            engine.set_leverage(sender, asset, leverage);
            Ok(vec![Event::new("update_leverage")
                .add_attribute("asset", &asset.to_string())
                .add_attribute("leverage", &leverage.to_string())])
        }
    }

    /// Execute set margin mode (sync version)
    fn execute_set_margin_mode_sync(
        &mut self,
        sender: AccountAddress,
        mode: hypercore_primitives::MarginMode,
    ) -> Result<Vec<Event>, AppError> {
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (set_margin_mode)")?;
            engine
                .set_margin_mode(sender, mode)
                .map_err(|e| AppError::Internal(format!("Set margin mode failed: {}", e)))?;
        }
        Ok(vec![Event::new("set_margin_mode")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("mode", &format!("{:?}", mode))])
    }

    /// Execute cancel trigger order (sync version)
    fn execute_cancel_trigger_sync(
        &mut self,
        sender: AccountAddress,
        _asset: hypercore_primitives::MarketId,
        order_id: hypercore_primitives::OrderId,
    ) -> Result<Vec<Event>, AppError> {
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine =
                acquire_write_with_retry(perp_engine, "Perp engine (cancel_trigger)")?;
            let removed = engine.state.remove_trigger_order(order_id);
            if removed.is_none() {
                return Err(AppError::Internal(format!(
                    "Trigger order {} not found",
                    order_id
                )));
            }
        }
        Ok(vec![Event::new("cancel_trigger")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("order_id", &format!("{}", order_id))])
    }

    /// Execute USD transfer (sync version)
    fn execute_usd_transfer_sync(
        &mut self,
        sender: AccountAddress,
        destination: AccountAddress,
        amount: &str,
    ) -> Result<Vec<Event>, AppError> {
        // USD transfers use USDC (6 decimals)
        let amount_decimal = Decimal::from_str_exact(amount, Decimal::USDC_DECIMALS)
            .ok_or(AppError::InvalidAmount)?;

        // Transfer using unified state
        let unified = self.state.unified_state.read().unwrap();

        // Check balance
        let sender_balance = unified.get_core_view(sender, 0);
        tracing::info!(
            "USD Transfer: sender={:?}, balance={}, amount={}",
            sender, sender_balance.to_string_trimmed(), amount_decimal.to_string_trimmed()
        );
        drop(unified);

        let mut unified = self.state.unified_state.write().unwrap();
        let sender_balance = unified.get_core_view(sender, 0);
        if sender_balance < amount_decimal {
            tracing::warn!(
                "Insufficient balance: sender={:?}, balance={}, required={}",
                sender, sender_balance.to_string_trimmed(), amount_decimal.to_string_trimmed()
            );
            return Err(AppError::Internal("Insufficient balance".to_string()));
        }

        // Execute transfer (core view to core view)
        if !unified.transfer_core(sender, destination, 0, amount_decimal) {
            return Err(AppError::Internal("Transfer failed".to_string()));
        }

        Ok(vec![Event::new("usd_transfer")
            .add_attribute("from", &format!("{:?}", sender))
            .add_attribute("to", &format!("{:?}", destination))
            .add_attribute("amount", amount)])
    }

    /// Execute withdrawal (sync version)
    fn execute_withdraw_sync(
        &mut self,
        sender: AccountAddress,
        destination: AccountAddress,
        amount: &str,
    ) -> Result<Vec<Event>, AppError> {
        // Withdrawals use USDC (6 decimals)
        let amount_decimal = Decimal::from_str_exact(amount, Decimal::USDC_DECIMALS)
            .ok_or(AppError::InvalidAmount)?;

        // Debit from unified state
        let mut unified = self.state.unified_state.write().unwrap();

        if !unified.debit_core(sender, 0, amount_decimal) {
            return Err(AppError::Internal("Insufficient balance for withdrawal".to_string()));
        }

        // In production, this would queue a withdrawal to L1
        Ok(vec![Event::new("withdraw")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("destination", &format!("{:?}", destination))
            .add_attribute("amount", amount)])
    }

    /// Execute EVM action (sync version)
    ///
    /// Handles actions queued by smart contracts via CoreWriter:
    /// - Action 0: Deposit from EVM view to Core view
    /// - Action 1: Withdraw from Core view to EVM view
    /// - Action 2: View transfer Core -> EVM (alias for action 1)
    /// - Action 3: View transfer EVM -> Core (alias for action 0)
    ///
    /// Data format (ABI-encoded):
    /// - bytes 0-31: token index (uint256, only last byte used)
    /// - bytes 32-63: amount (uint256)
    fn execute_evm_action_sync(
        &mut self,
        sender: AccountAddress,
        action_type: u8,
        data: &[u8],
    ) -> Result<Vec<Event>, AppError> {
        // Parse token and amount from ABI-encoded data
        let (token, amount) = if data.len() >= 64 {
            let token = data[31]; // Last byte of first 32-byte word
            // Parse amount from bytes 32-63 (uint256, we use lower 128 bits)
            let amount_bytes = &data[32..64];
            let high = u128::from_be_bytes(amount_bytes[0..16].try_into().unwrap_or([0; 16]));
            let low = u128::from_be_bytes(amount_bytes[16..32].try_into().unwrap_or([0; 16]));
            // If high bits are set, value is too large - use max i128
            let amount_raw = if high > 0 { i128::MAX } else { low as i128 };
            // Amount is in 18 decimals (EVM standard), convert to token decimals
            let token_decimals = if token == 0 { 6 } else { 18 };
            let amount = Decimal::from_raw(amount_raw, 18).scale_to(token_decimals);
            (token, amount)
        } else if data.is_empty() {
            // Empty data means this is just an event notification
            return match action_type {
                0 | 3 => Ok(vec![Event::new("evm_deposit")
                    .add_attribute("sender", &format!("{:?}", sender))
                    .add_attribute("status", "no_data")]),
                1 | 2 => Ok(vec![Event::new("evm_withdraw")
                    .add_attribute("sender", &format!("{:?}", sender))
                    .add_attribute("status", "no_data")]),
                _ => Ok(vec![Event::new("evm_action_unknown")
                    .add_attribute("action_type", &action_type.to_string())]),
            };
        } else {
            return Err(AppError::Internal(format!(
                "Invalid EVM action data length: {} (expected 64 bytes)",
                data.len()
            )));
        };

        let mut unified = self.state.unified_state.write().unwrap();

        match action_type {
            0 | 3 => {
                // Deposit: Move from EVM view to Core view
                if let Err(e) = unified.transfer_to_core_view(sender, token, amount) {
                    return Err(AppError::Internal(format!(
                        "View transfer to core failed: token={}, amount={}, error={:?}",
                        token, amount.to_string_trimmed(), e
                    )));
                }

                let new_core = unified.get_core_view(sender, token);
                let new_evm = unified.get_evm_view(sender, token);

                Ok(vec![Event::new("evm_deposit")
                    .add_attribute("sender", &format!("{:?}", sender))
                    .add_attribute("token", &token.to_string())
                    .add_attribute("amount", &amount.to_string_trimmed())
                    .add_attribute("new_core_view", &new_core.to_string_trimmed())
                    .add_attribute("new_evm_view", &new_evm.to_string_trimmed())])
            }
            1 | 2 => {
                // Withdraw: Move from Core view to EVM view
                if let Err(e) = unified.transfer_to_evm_view(sender, token, amount) {
                    return Err(AppError::Internal(format!(
                        "View transfer to EVM failed: token={}, amount={}, error={:?}",
                        token, amount.to_string_trimmed(), e
                    )));
                }

                let new_core = unified.get_core_view(sender, token);
                let new_evm = unified.get_evm_view(sender, token);

                Ok(vec![Event::new("evm_withdraw")
                    .add_attribute("sender", &format!("{:?}", sender))
                    .add_attribute("token", &token.to_string())
                    .add_attribute("amount", &amount.to_string_trimmed())
                    .add_attribute("new_core_view", &new_core.to_string_trimmed())
                    .add_attribute("new_evm_view", &new_evm.to_string_trimmed())])
            }
            _ => {
                Ok(vec![Event::new("evm_action_unknown")
                    .add_attribute("action_type", &action_type.to_string())])
            }
        }
    }

    /// Begin a new block
    pub fn begin_block(&mut self, height: BlockHeight, timestamp: Timestamp) {
        self.state.height = height;
        self.state.timestamp = timestamp;
    }

    /// End the current block
    ///
    /// Processes funding rate payments and liquidations for perpetual markets.
    /// Collects all block events (fills, orders, funding, liquidations) for indexing.
    pub fn end_block(&mut self) -> Vec<ValidatorUpdate> {
        // Start with pending events accumulated during tx execution (fills, orders)
        let mut events: Vec<serde_json::Value> = self.state.take_pending_block_events();

        // Process funding rates
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (end_block funding)")
                .expect("perp_engine write lock should be acquirable during end_block");
            let funding_rates = engine.process_funding(self.state.timestamp);
            drop(engine); // Release write lock before acquiring read lock on engine state

            for (market_id, rate) in &funding_rates {
                // Get market state for mark/index prices from EngineState
                let (mark_price, index_price) = if let Ok(engine_state) = self.state.engine.try_read() {
                    let result = engine_state.get_market_state(*market_id)
                        .map(|s| (s.mark_price, s.index_price))
                        .unwrap_or((Decimal::ZERO, Decimal::ZERO));
                    result
                } else {
                    (Decimal::ZERO, Decimal::ZERO)
                };

                let event = BlockEvent::FundingApplied(FundingAppliedEvent {
                    market_id: *market_id,
                    funding_rate: rate.raw_value(), // Convert Decimal to i128
                    mark_price,
                    index_price,
                    timestamp: self.state.timestamp,
                });
                if let Ok(json) = serde_json::to_value(&event) {
                    events.push(json);
                }
            }
            if !funding_rates.is_empty() {
                tracing::info!("Applied funding to {} markets", funding_rates.len());
            }
        }

        // Process liquidations
        if let Some(perp_engine) = &self.state.perp_engine {
            let mut engine = acquire_write_with_retry(perp_engine, "Perp engine (end_block liquidations)")
                .expect("perp_engine write lock should be acquirable during end_block");
            let liquidations = engine.process_liquidations(self.state.timestamp);
            drop(engine); // Release write lock

            for (account, market_id, size) in &liquidations {
                // Get mark price for liquidation price from EngineState
                let price = if let Ok(engine_state) = self.state.engine.try_read() {
                    engine_state.get_market_state(*market_id)
                        .map(|s| s.mark_price)
                        .unwrap_or(Decimal::ZERO)
                } else {
                    Decimal::ZERO
                };

                let event = BlockEvent::Liquidation(LiquidationEvent {
                    account: *account,
                    market_id: *market_id,
                    size: size.raw_value(), // Convert Decimal to i128
                    price,
                    pnl: 0, // PnL calculation happens in engine
                    timestamp: self.state.timestamp,
                });
                if let Ok(json) = serde_json::to_value(&event) {
                    events.push(json);
                }
            }
            if !liquidations.is_empty() {
                tracing::info!("Processed {} liquidations", liquidations.len());
            }
        }

        // Store events for this block (in AppState)
        self.state.store_block_events(self.state.height, events.clone());

        // Finalize block state - builds Merkle trees and computes app hash
        // Phase 3C: This caches the Merkle trees for efficient proof generation
        let app_hash = self.state.end_block();
        let tx_count = self.state.pending_txs().len() as u32;
        self.state.store_block_metadata(BlockMeta {
            height: self.state.height,
            timestamp: self.state.timestamp,
            hash: app_hash,
            proposer: None,
            tx_count,
        });

        // Also store block metadata to shared EngineState (for indexer access)
        {
            use hypercore_engine::BlockMetadata;
            match acquire_write_with_retry(&self.state.engine, "Engine (block metadata)") {
                Ok(mut engine) => {
                    engine.store_block_metadata(
                        self.state.height,
                        BlockMetadata {
                            hash: app_hash,
                            timestamp: self.state.timestamp,
                            tx_count,
                            events,
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!("Could not acquire engine write lock for block metadata storage: {}", e);
                }
            }
        }

        vec![]
    }

    /// Commit state changes
    ///
    /// Returns the app hash that was computed during end_block.
    /// Note: We return the stored hash rather than recomputing it because
    /// compute_app_hash() includes self.app_hash (previous hash) as input,
    /// and that field was already updated by end_block().
    pub fn commit(&mut self) -> [u8; 32] {
        self.state.commit();
        self.state.app_hash
    }

    /// Get current height
    pub fn current_height(&self) -> BlockHeight {
        self.state.height
    }

    /// Get unified state reference
    pub fn unified_state(&self) -> &SharedUnifiedState {
        &self.state.unified_state
    }

    /// Get engine state reference
    pub fn engine(&self) -> &SharedEngineState {
        &self.state.engine
    }

    // === Persistence Helper Methods ===

    /// Get all nonces (for persistence)
    pub fn get_nonces(&self) -> &std::collections::HashMap<AccountAddress, u64> {
        self.state.get_all_nonces()
    }

    /// Get all CLOID mappings (for persistence)
    pub fn get_cloid_index(&self) -> &std::collections::HashMap<(AccountAddress, String), (MarketId, OrderId)> {
        self.state.get_all_cloid_mappings()
    }

    /// Get all block metadata (for persistence)
    pub fn get_block_metadata(&self) -> &std::collections::HashMap<BlockHeight, crate::state::BlockMeta> {
        self.state.get_all_block_metadata()
    }

    /// Get block events for a specific height (for WebSocket broadcasting)
    pub fn get_block_events(&self, height: BlockHeight) -> Vec<serde_json::Value> {
        self.state.get_block_events(height)
    }

    /// Get all block hashes (for persistence)
    pub fn get_block_hashes(&self) -> &std::collections::HashMap<BlockHeight, [u8; 32]> {
        self.state.get_all_block_hashes()
    }

    /// Create a snapshot of the current state (blocking version)
    pub fn snapshot_blocking(&self) -> crate::state::AppStateSnapshot {
        self.state.snapshot_blocking()
    }

    /// Restore state from a snapshot (blocking version)
    pub fn restore_blocking(&mut self, snapshot: crate::state::AppStateSnapshot) {
        self.state.restore_blocking(snapshot)
    }

    /// Generate a Merkle proof for a nonce entry
    pub fn generate_nonce_proof(&self, address: &AccountAddress) -> Option<crate::merkle::MerkleProof> {
        self.state.prove_nonce(*address)
    }

    /// Get the nonce Merkle root
    pub fn get_nonce_root(&self) -> [u8; 32] {
        self.state.get_nonce_root()
    }

    /// Query state (for ABCI Query)
    pub async fn query(&self, path: &str, data: &[u8]) -> Result<Vec<u8>, AppError> {
        match path {
            "/account" if data.len() == 20 => {
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(data);
                let address = AccountAddress::from(addr_bytes);

                let engine = self.state.engine.read().await;
                if let Some(account) = engine.get_account(address) {
                    serde_json::to_vec(account)
                        .map_err(|e| AppError::QueryError(e.to_string()))
                } else {
                    Ok(vec![])
                }
            }

            "/balance" if data.len() >= 20 => {
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(&data[0..20]);
                let address = AccountAddress::from(addr_bytes);
                let token = if data.len() > 20 { data[20] } else { 0 };

                let balance = self.state.get_core_balance(address, token);
                serde_json::to_vec(&balance.to_string_trimmed())
                    .map_err(|e| AppError::QueryError(e.to_string()))
            }

            "/market" if !data.is_empty() => {
                let market_id = data[0];
                let engine = self.state.engine.read().await;
                if let Some(market) = engine.get_market(market_id) {
                    serde_json::to_vec(market)
                        .map_err(|e| AppError::QueryError(e.to_string()))
                } else {
                    Ok(vec![])
                }
            }

            _ => Err(AppError::QueryError(format!("Unknown path: {}", path))),
        }
    }

    /// Query account state (sync version for ABCI)
    pub fn query_account(&self, data: &[u8]) -> Result<Vec<u8>, AppError> {
        if data.len() != 20 {
            return Err(AppError::QueryError("Invalid address length".to_string()));
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(data);
        let address = AccountAddress::from(addr_bytes);

        let engine = self.state.engine.try_read()
            .map_err(|_| AppError::QueryError("Engine lock contention".to_string()))?;
        if let Some(account) = engine.get_account(address) {
            serde_json::to_vec(account)
                .map_err(|e| AppError::QueryError(e.to_string()))
        } else {
            Ok(vec![])
        }
    }

    /// Query position (sync version for ABCI)
    pub fn query_position(&self, data: &[u8]) -> Result<Vec<u8>, AppError> {
        if data.len() < 21 {
            return Err(AppError::QueryError("Invalid query data".to_string()));
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&data[0..20]);
        let address = AccountAddress::from(addr_bytes);
        let market_id = data[20];

        let engine = self.state.engine.try_read()
            .map_err(|_| AppError::QueryError("Engine lock contention".to_string()))?;
        if let Some(position) = engine.get_position(address, market_id) {
            serde_json::to_vec(position)
                .map_err(|e| AppError::QueryError(e.to_string()))
        } else {
            Ok(vec![])
        }
    }

    /// Query orderbook (sync version for ABCI)
    pub fn query_orderbook(&self, data: &[u8]) -> Result<Vec<u8>, AppError> {
        if data.is_empty() {
            return Err(AppError::QueryError("Invalid market id".to_string()));
        }
        let market_id = data[0];

        let engine = self.state.engine.try_read()
            .map_err(|_| AppError::QueryError("Engine lock contention".to_string()))?;
        if let Some(orderbook) = engine.get_orderbook(market_id) {
            let snapshot = orderbook.get_l2_snapshot(50);
            serde_json::to_vec(&snapshot)
                .map_err(|e| AppError::QueryError(e.to_string()))
        } else {
            Ok(vec![])
        }
    }
}

/// Transaction execution result
#[derive(Debug, Clone)]
pub struct TxResult {
    pub success: bool,
    pub events: Vec<Event>,
    pub gas_used: u64,
}

/// Event generated by transaction execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub r#type: String,
    pub attributes: Vec<EventAttribute>,
}

impl Event {
    pub fn new(event_type: &str) -> Self {
        Self {
            r#type: event_type.to_string(),
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(mut self, key: &str, value: &str) -> Self {
        self.attributes.push(EventAttribute {
            key: key.to_string(),
            value: value.to_string(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
}

/// Validator update for end block
#[derive(Debug, Clone)]
pub struct ValidatorUpdate {
    pub pub_key: Vec<u8>,
    pub power: i64,
}

/// Genesis state
///
/// Defines the initial state of the blockchain at genesis (block 0).
/// All validators must start with identical genesis state for consensus.
///
/// # Example Genesis JSON
/// ```json
/// {
///   "chain_id": "hypercore-devnet",
///   "markets": [...],
///   "spot_tokens": [...],
///   "balances": [
///     {"address": "0xf39F...", "token": 0, "amount": "100000000000", "view": "core"}
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisState {
    /// Chain identifier (e.g., "hypercore-devnet", "hypercore-mainnet")
    pub chain_id: String,
    /// Perpetual markets to initialize (IDs 0-127)
    #[serde(default)]
    pub markets: Vec<GenesisMarket>,
    /// Spot tokens to deploy (IDs 1+, USDC is implicitly 0)
    #[serde(default)]
    pub spot_tokens: Vec<GenesisSpotToken>,
    /// Initial account balances with view allocation
    #[serde(default)]
    pub balances: Vec<GenesisBalance>,
}

/// Genesis balance entry
///
/// Specifies an initial balance for an account/token pair with view allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisBalance {
    /// Account address (hex format: "0x...")
    pub address: AccountAddress,
    /// Token index (0 = USDC, 1+ = deployed tokens)
    pub token: u8,
    /// Amount as decimal string (e.g., "100000.0" for 100k)
    pub amount: String,
    /// Which view to credit: "core" for trading, "evm" for smart contracts
    #[serde(default = "default_view")]
    pub view: BalanceView,
}

fn default_view() -> BalanceView {
    BalanceView::Core
}

/// Balance view allocation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BalanceView {
    /// Core view - available for HyperCore trading (orders, margin)
    #[default]
    Core,
    /// EVM view - available for HyperEVM operations (gas, contracts)
    Evm,
}

/// Genesis spot token configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisSpotToken {
    /// Token index (1+, 0 is reserved for USDC)
    pub index: u8,
    /// Token name (e.g., "Test Token")
    pub name: String,
    /// Token symbol (e.g., "TEST")
    pub symbol: String,
    /// Wei decimals (typically 18)
    #[serde(default = "default_wei_decimals")]
    pub wei_decimals: u8,
    /// Size decimals for order sizing
    #[serde(default = "default_sz_decimals")]
    pub sz_decimals: u8,
    /// Max supply as decimal string
    #[serde(default = "default_max_supply")]
    pub max_supply: String,
}

fn default_max_supply() -> String { "1000000000".to_string() }

fn default_wei_decimals() -> u8 { 18 }
fn default_sz_decimals() -> u8 { 4 }

/// Genesis market configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisMarket {
    pub id: MarketId,
    pub symbol: String,
    pub max_leverage: u8,
    #[serde(alias = "initial_price")]
    pub initial_mark_price: String,
}

impl From<GenesisMarket> for hypercore_primitives::Market {
    fn from(m: GenesisMarket) -> Self {
        hypercore_primitives::Market::new(
            hypercore_primitives::MarketConfig::new(
                m.id,
                m.symbol,
                m.max_leverage,
            ),
            Decimal::price(&m.initial_mark_price),
            0,
        )
    }
}

/// Application errors
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Genesis error: {0}")]
    GenesisError(String),
    #[error("Transaction error: {0}")]
    TxError(#[from] crate::tx::TransactionError),
    #[error("State error: {0}")]
    StateError(#[from] crate::state::StateError),
    #[error("Invalid amount")]
    InvalidAmount,
    #[error("Query error: {0}")]
    QueryError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use hypercore_engine::EngineState;
    use hypercore_primitives::new_shared_unified_state;

    #[test]
    fn test_app_creation() {
        let app = HyperCoreApp::new();
        assert_eq!(app.current_height(), 0);
    }

    #[tokio::test]
    async fn test_app_with_shared_state() {
        let unified = new_shared_unified_state();
        let engine = Arc::new(RwLock::new(EngineState::new()));

        let app = HyperCoreApp::with_shared_state(
            Arc::clone(&unified),
            Arc::clone(&engine),
            None,
        );

        assert_eq!(app.current_height(), 0);

        // Verify shared state is actually shared
        {
            let mut state = unified.write().unwrap();
            state.credit(
                AccountAddress::from([0x42u8; 20]),
                0,
                Decimal::from_raw(1000, 6),
            );
        }

        let balance = app.state.get_core_balance(
            AccountAddress::from([0x42u8; 20]),
            0,
        );
        assert_eq!(balance.raw(), 1000);
    }
}
