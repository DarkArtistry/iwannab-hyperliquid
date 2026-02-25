//! Channel-based engine interface (Phase B4)
//!
//! Provides a lock-free interface to the matching engine via message passing.
//! The `MatchingCore` runs on a dedicated OS thread, eliminating all RwLock
//! contention on the hot path.
//!
//! ## Architecture
//!
//! ```text
//! Gateway Thread 1 ──┐
//! Gateway Thread 2 ──┤── mpsc::Sender<EngineCommand> ──► MatchingCore Thread
//! Block Producer   ──┘                                      (pinned to core 0)
//!                                                           │
//!                     oneshot::Receiver<Result>  ◄──────────┘
//! ```
//!
//! ## Benefits
//!
//! - No RwLock contention: all engine access is serialized through channels
//! - Cache-friendly: matching engine stays on one core
//! - Predictable latency: no lock convoy or priority inversion
//! - Natural batching: multiple commands processed per wake-up

use std::collections::HashMap;

use hypercore_primitives::{
    AccountAddress, AccountState, Decimal, Fill, MarginMode, Market, MarketId, Order, OrderId,
    OrderRequest, Position, Result, Timestamp,
};
use tokio::sync::{mpsc, oneshot};

use crate::state::{BlockMetadata, EngineStateSnapshot};
use crate::Engine;

/// Commands that can be sent to the matching engine
pub enum EngineCommand {
    /// Place an order
    PlaceOrder {
        account: AccountAddress,
        request: OrderRequest,
        timestamp: Timestamp,
        reply: oneshot::Sender<Result<(Order, Vec<Fill>)>>,
    },
    /// Cancel an order
    CancelOrder {
        account: AccountAddress,
        market_id: MarketId,
        order_id: OrderId,
        reply: oneshot::Sender<Result<Order>>,
    },
    /// Cancel all orders for account in a specific market
    CancelAll {
        account: AccountAddress,
        market_id: MarketId,
        reply: oneshot::Sender<Result<Vec<Order>>>,
    },
    /// Cancel all orders for account across ALL markets
    CancelAllMarkets {
        account: AccountAddress,
        reply: oneshot::Sender<Vec<(MarketId, Vec<Order>)>>,
    },
    /// Update leverage
    UpdateLeverage {
        account: AccountAddress,
        market_id: MarketId,
        leverage: u8,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Process funding for all markets
    ProcessFunding {
        timestamp: Timestamp,
        reply: oneshot::Sender<Vec<(MarketId, Decimal)>>,
    },
    /// Process liquidations
    ProcessLiquidations {
        timestamp: Timestamp,
        reply: oneshot::Sender<Vec<(AccountAddress, MarketId, Decimal)>>,
    },
    /// Check trigger orders after mark price update
    CheckTriggers {
        market_id: MarketId,
        timestamp: Timestamp,
        reply: oneshot::Sender<Vec<(Order, Vec<Fill>)>>,
    },
    /// Remove a specific trigger order
    RemoveTriggerOrder {
        order_id: OrderId,
        reply: oneshot::Sender<Option<Order>>,
    },
    /// Get orderbook L2 snapshot
    GetOrderbook {
        market_id: MarketId,
        depth: usize,
        reply: oneshot::Sender<Option<crate::orderbook::L2Snapshot>>,
    },
    /// Set margin mode for an account
    SetMarginMode {
        account: AccountAddress,
        mode: MarginMode,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Store block metadata
    StoreBlockMetadata {
        height: u64,
        metadata: BlockMetadata,
        reply: oneshot::Sender<()>,
    },
    /// Get a full engine state snapshot (for persistence/state extraction)
    GetEngineSnapshot {
        reply: oneshot::Sender<EngineStateSnapshot>,
    },
    /// Get the next order ID (for CLOID tracking)
    PeekNextOrderId {
        reply: oneshot::Sender<OrderId>,
    },
    // --- Read queries (for gateway info requests) ---
    /// Get all mid prices
    GetAllMidPrices {
        reply: oneshot::Sender<Vec<(MarketId, Decimal)>>,
    },
    /// Get all markets
    GetAllMarkets {
        reply: oneshot::Sender<Vec<(MarketId, Market)>>,
    },
    /// Get all positions for an account
    GetAllPositions {
        account: AccountAddress,
        reply: oneshot::Sender<Vec<(MarketId, Position)>>,
    },
    /// Get all orders for an account
    GetAllUserOrders {
        account: AccountAddress,
        reply: oneshot::Sender<Vec<Order>>,
    },
    /// Get user trigger orders
    GetUserTriggerOrders {
        account: AccountAddress,
        reply: oneshot::Sender<Vec<Order>>,
    },
    /// Get user fills
    GetUserFills {
        account: AccountAddress,
        limit: Option<usize>,
        reply: oneshot::Sender<Vec<Fill>>,
    },
    /// Get recent trades for a market
    GetRecentTrades {
        market_id: MarketId,
        limit: Option<usize>,
        reply: oneshot::Sender<Vec<Fill>>,
    },
    /// Get account state
    GetAccount {
        account: AccountAddress,
        reply: oneshot::Sender<Option<AccountState>>,
    },
    /// Get a position
    GetPosition {
        account: AccountAddress,
        market_id: MarketId,
        reply: oneshot::Sender<Option<Position>>,
    },
    /// Check if an order exists in the book
    CheckOrderExists {
        market_id: MarketId,
        order_id: OrderId,
        reply: oneshot::Sender<bool>,
    },
    /// Get leverage for account in market
    GetLeverage {
        account: AccountAddress,
        market_id: MarketId,
        reply: oneshot::Sender<u8>,
    },
    /// Get all positions globally (for persistence extraction)
    GetAllPositionsGlobal {
        reply: oneshot::Sender<HashMap<AccountAddress, HashMap<MarketId, Position>>>,
    },
    /// Get all leverage globally (for persistence extraction)
    GetAllLeverageGlobal {
        reply: oneshot::Sender<HashMap<AccountAddress, HashMap<MarketId, u8>>>,
    },
    /// Shutdown the engine thread
    Shutdown,
}

// Manual Debug impl since oneshot::Sender doesn't implement Debug
impl std::fmt::Debug for EngineCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlaceOrder { account, request, timestamp, .. } => {
                f.debug_struct("PlaceOrder")
                    .field("account", account)
                    .field("market_id", &request.market_id)
                    .field("timestamp", timestamp)
                    .finish()
            }
            Self::CancelOrder { account, market_id, order_id, .. } => {
                f.debug_struct("CancelOrder")
                    .field("account", account)
                    .field("market_id", market_id)
                    .field("order_id", order_id)
                    .finish()
            }
            Self::Shutdown => write!(f, "Shutdown"),
            _ => write!(f, "EngineCommand"),
        }
    }
}

/// Matching engine core that runs on a dedicated thread
///
/// Processes commands sequentially from the channel, ensuring
/// all state mutations are serialized without locks.
pub struct MatchingCore {
    engine: Engine,
    rx: mpsc::Receiver<EngineCommand>,
    /// CPU core to pin to (Phase E2). Only used when `core-pinning` feature is enabled.
    core_id: Option<usize>,
}

impl MatchingCore {
    /// Create a new matching core
    pub fn new(engine: Engine, rx: mpsc::Receiver<EngineCommand>) -> Self {
        Self {
            engine,
            rx,
            core_id: None,
        }
    }

    /// Set the CPU core to pin the matching thread to (Phase E2)
    pub fn with_core_pin(mut self, core_id: usize) -> Self {
        self.core_id = Some(core_id);
        self
    }

    /// Run the engine loop (blocks forever until Shutdown or channel closed)
    pub fn run(mut self) {
        #[cfg(feature = "core-pinning")]
        if let Some(core_id) = self.core_id {
            if crate::pin_to_core(core_id) {
                tracing::info!("MatchingCore pinned to CPU core {}", core_id);
            } else {
                tracing::warn!("Failed to pin MatchingCore to core {}", core_id);
            }
        }

        tracing::info!("MatchingCore started on dedicated thread");

        loop {
            let first = match self.rx.blocking_recv() {
                Some(cmd) => cmd,
                None => break,
            };

            if matches!(first, EngineCommand::Shutdown) {
                tracing::info!("MatchingCore received shutdown command");
                break;
            }

            // Batch: drain any additional pending commands (non-blocking)
            let mut batch = Vec::with_capacity(64);
            batch.push(first);
            while batch.len() < 256 {
                match self.rx.try_recv() {
                    Ok(cmd) => batch.push(cmd),
                    Err(_) => break,
                }
            }

            let batch_size = batch.len();
            if batch_size > 1 {
                tracing::trace!("Processing batch of {} commands", batch_size);
            }

            for cmd in batch {
                self.process_command(cmd);
            }
        }

        tracing::info!("MatchingCore stopped");
    }

    fn process_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::PlaceOrder {
                account,
                request,
                timestamp,
                reply,
            } => {
                let result = self.engine.place_order(account, request, timestamp);
                let _ = reply.send(result);
            }
            EngineCommand::CancelOrder {
                account,
                market_id,
                order_id,
                reply,
            } => {
                let result = self.engine.cancel_order(account, market_id, order_id);
                let _ = reply.send(result);
            }
            EngineCommand::CancelAll {
                account,
                market_id,
                reply,
            } => {
                let result = self.engine.cancel_all_orders(account, market_id);
                let _ = reply.send(result);
            }
            EngineCommand::CancelAllMarkets { account, reply } => {
                let market_ids: Vec<MarketId> = self
                    .engine
                    .state
                    .get_all_markets()
                    .iter()
                    .map(|m| m.config.id)
                    .collect();
                let mut results = Vec::new();
                for market_id in market_ids {
                    if let Ok(orders) = self.engine.cancel_all_orders(account, market_id) {
                        if !orders.is_empty() {
                            results.push((market_id, orders));
                        }
                    }
                }
                let _ = reply.send(results);
            }
            EngineCommand::UpdateLeverage {
                account,
                market_id,
                leverage,
                reply,
            } => {
                let result = self.engine.update_leverage(account, market_id, leverage);
                let _ = reply.send(result);
            }
            EngineCommand::ProcessFunding { timestamp, reply } => {
                let rates = self.engine.process_funding(timestamp);
                let _ = reply.send(rates);
            }
            EngineCommand::ProcessLiquidations { timestamp, reply } => {
                let liquidations = self.engine.process_liquidations(timestamp);
                let _ = reply.send(liquidations);
            }
            EngineCommand::CheckTriggers {
                market_id,
                timestamp,
                reply,
            } => {
                let mark = self
                    .engine
                    .state
                    .get_market(market_id)
                    .map(|m| m.state.mark_price)
                    .unwrap_or(Decimal::from_raw(0, Decimal::PRICE_DECIMALS));
                let results = self.engine.check_triggers(market_id, mark, timestamp);
                let _ = reply.send(results);
            }
            EngineCommand::RemoveTriggerOrder { order_id, reply } => {
                let result = self.engine.state.remove_trigger_order(order_id);
                let _ = reply.send(result);
            }
            EngineCommand::GetOrderbook {
                market_id,
                depth,
                reply,
            } => {
                let snapshot = self
                    .engine
                    .state
                    .get_orderbook(market_id)
                    .map(|book| book.get_l2_snapshot(depth));
                let _ = reply.send(snapshot);
            }
            EngineCommand::SetMarginMode {
                account,
                mode,
                reply,
            } => {
                let result = self.engine.set_margin_mode(account, mode);
                let _ = reply.send(result);
            }
            EngineCommand::StoreBlockMetadata {
                height,
                metadata,
                reply,
            } => {
                self.engine.state.store_block_metadata(height, metadata);
                let _ = reply.send(());
            }
            EngineCommand::GetEngineSnapshot { reply } => {
                let snapshot = self.engine.state.snapshot();
                let _ = reply.send(snapshot);
            }
            EngineCommand::PeekNextOrderId { reply } => {
                let id = self.engine.state.peek_next_order_id();
                let _ = reply.send(id);
            }
            EngineCommand::GetAllMidPrices { reply } => {
                let prices = self.engine.state.get_all_mid_prices();
                let _ = reply.send(prices);
            }
            EngineCommand::GetAllMarkets { reply } => {
                let markets = self
                    .engine
                    .state
                    .get_all_markets()
                    .iter()
                    .map(|m| (m.config.id, (*m).clone()))
                    .collect();
                let _ = reply.send(markets);
            }
            EngineCommand::GetAllPositions { account, reply } => {
                let positions = self.engine.state.get_all_positions(account);
                let _ = reply.send(positions);
            }
            EngineCommand::GetAllUserOrders { account, reply } => {
                let orders: Vec<Order> = self
                    .engine
                    .state
                    .get_all_user_orders(account)
                    .into_iter()
                    .cloned()
                    .collect();
                let _ = reply.send(orders);
            }
            EngineCommand::GetUserTriggerOrders { account, reply } => {
                let orders: Vec<Order> = self
                    .engine
                    .state
                    .get_user_trigger_orders(account)
                    .into_iter()
                    .cloned()
                    .collect();
                let _ = reply.send(orders);
            }
            EngineCommand::GetUserFills {
                account,
                limit,
                reply,
            } => {
                let fills: Vec<Fill> = self
                    .engine
                    .state
                    .get_user_fills(account, limit)
                    .into_iter()
                    .cloned()
                    .collect();
                let _ = reply.send(fills);
            }
            EngineCommand::GetRecentTrades {
                market_id,
                limit,
                reply,
            } => {
                let trades: Vec<Fill> = self
                    .engine
                    .state
                    .get_recent_trades(market_id, limit)
                    .into_iter()
                    .cloned()
                    .collect();
                let _ = reply.send(trades);
            }
            EngineCommand::GetAccount { account, reply } => {
                let state = self.engine.state.get_account(account).cloned();
                let _ = reply.send(state);
            }
            EngineCommand::GetPosition {
                account,
                market_id,
                reply,
            } => {
                let pos = self.engine.state.get_position(account, market_id).cloned();
                let _ = reply.send(pos);
            }
            EngineCommand::CheckOrderExists {
                market_id,
                order_id,
                reply,
            } => {
                let exists = self
                    .engine
                    .state
                    .get_orderbook(market_id)
                    .map(|book| book.get(order_id).is_some())
                    .unwrap_or(false);
                let _ = reply.send(exists);
            }
            EngineCommand::GetLeverage {
                account,
                market_id,
                reply,
            } => {
                let lev = self.engine.state.get_leverage(account, market_id);
                let _ = reply.send(lev);
            }
            EngineCommand::GetAllPositionsGlobal { reply } => {
                let positions = self
                    .engine
                    .state
                    .get_all_positions_global()
                    .iter()
                    .map(|(addr, positions)| {
                        (*addr, positions.iter().map(|(mid, pos)| (*mid, pos.clone())).collect())
                    })
                    .collect();
                let _ = reply.send(positions);
            }
            EngineCommand::GetAllLeverageGlobal { reply } => {
                let leverage = self
                    .engine
                    .state
                    .get_all_leverage_global()
                    .iter()
                    .map(|(addr, leverages)| {
                        (*addr, leverages.iter().map(|(mid, lev)| (*mid, *lev)).collect())
                    })
                    .collect();
                let _ = reply.send(leverage);
            }
            EngineCommand::Shutdown => {
                tracing::info!("MatchingCore received shutdown command");
            }
        }
    }

    /// Get a reference to the inner engine (for initialization before spawning)
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get a mutable reference to the inner engine (for initialization before spawning)
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

/// Handle for sending commands to the matching engine
///
/// This is the public-facing interface. It can be cloned and shared
/// across multiple async tasks. Provides both async and blocking variants.
///
/// - Use async methods from tokio async contexts (gateway, WebSocket)
/// - Use blocking methods from synchronous contexts (ABCI FinalizeBlock)
#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<EngineCommand>,
}

// Helper macro to reduce boilerplate for async send+recv pattern
macro_rules! engine_cmd {
    ($self:ident, $variant:ident { $($field:ident),* $(,)? }) => {{
        let (reply_tx, reply_rx) = oneshot::channel();
        $self.tx
            .send(EngineCommand::$variant {
                $($field,)*
                reply: reply_tx,
            })
            .await
            .map_err(|_| hypercore_primitives::Error::Internal("Engine channel closed".into()))?;
        reply_rx
            .await
            .map_err(|_| hypercore_primitives::Error::Internal("Engine reply dropped".into()))
    }};
}

// Helper macro for blocking send+recv
macro_rules! engine_cmd_blocking {
    ($self:ident, $variant:ident { $($field:ident),* $(,)? }) => {{
        let (reply_tx, reply_rx) = oneshot::channel();
        $self.tx
            .blocking_send(EngineCommand::$variant {
                $($field,)*
                reply: reply_tx,
            })
            .map_err(|_| hypercore_primitives::Error::Internal("Engine channel closed".into()))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| hypercore_primitives::Error::Internal("Engine reply dropped".into()))
    }};
}

impl EngineHandle {
    /// Create a new engine handle and matching core
    ///
    /// Returns (handle, core). The core should be moved to a dedicated thread:
    /// ```ignore
    /// let (handle, core) = EngineHandle::new(engine, 10_000);
    /// std::thread::spawn(move || core.run());
    /// ```
    pub fn new(engine: Engine, channel_size: usize) -> (Self, MatchingCore) {
        let (tx, rx) = mpsc::channel(channel_size);
        let handle = Self { tx };
        let core = MatchingCore::new(engine, rx);
        (handle, core)
    }

    // ========================================================================
    // Async methods (for gateway / tokio contexts)
    // ========================================================================

    /// Place an order through the engine
    pub async fn place_order(
        &self,
        account: AccountAddress,
        request: OrderRequest,
        timestamp: Timestamp,
    ) -> Result<(Order, Vec<Fill>)> {
        engine_cmd!(self, PlaceOrder { account, request, timestamp })?
    }

    /// Cancel an order
    pub async fn cancel_order(
        &self,
        account: AccountAddress,
        market_id: MarketId,
        order_id: OrderId,
    ) -> Result<Order> {
        engine_cmd!(self, CancelOrder { account, market_id, order_id })?
    }

    /// Cancel all orders for account in market
    pub async fn cancel_all_orders(
        &self,
        account: AccountAddress,
        market_id: MarketId,
    ) -> Result<Vec<Order>> {
        engine_cmd!(self, CancelAll { account, market_id })?
    }

    /// Update leverage
    pub async fn update_leverage(
        &self,
        account: AccountAddress,
        market_id: MarketId,
        leverage: u8,
    ) -> Result<()> {
        engine_cmd!(self, UpdateLeverage { account, market_id, leverage })?
    }

    /// Process funding
    pub async fn process_funding(
        &self,
        timestamp: Timestamp,
    ) -> std::result::Result<Vec<(MarketId, Decimal)>, hypercore_primitives::Error> {
        engine_cmd!(self, ProcessFunding { timestamp })
    }

    /// Process liquidations
    pub async fn process_liquidations(
        &self,
        timestamp: Timestamp,
    ) -> std::result::Result<Vec<(AccountAddress, MarketId, Decimal)>, hypercore_primitives::Error>
    {
        engine_cmd!(self, ProcessLiquidations { timestamp })
    }

    /// Get orderbook snapshot
    pub async fn get_orderbook(
        &self,
        market_id: MarketId,
        depth: usize,
    ) -> std::result::Result<Option<crate::orderbook::L2Snapshot>, hypercore_primitives::Error>
    {
        engine_cmd!(self, GetOrderbook { market_id, depth })
    }

    /// Set margin mode for an account
    pub async fn set_margin_mode(
        &self,
        account: AccountAddress,
        mode: MarginMode,
    ) -> Result<()> {
        engine_cmd!(self, SetMarginMode { account, mode })?
    }

    /// Get all mid prices
    pub async fn get_all_mid_prices(
        &self,
    ) -> std::result::Result<Vec<(MarketId, Decimal)>, hypercore_primitives::Error> {
        engine_cmd!(self, GetAllMidPrices {})
    }

    /// Get all markets
    pub async fn get_all_markets(
        &self,
    ) -> std::result::Result<Vec<(MarketId, Market)>, hypercore_primitives::Error> {
        engine_cmd!(self, GetAllMarkets {})
    }

    /// Get all positions for an account
    pub async fn get_all_positions(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Vec<(MarketId, Position)>, hypercore_primitives::Error> {
        engine_cmd!(self, GetAllPositions { account })
    }

    /// Get all user orders
    pub async fn get_all_user_orders(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Vec<Order>, hypercore_primitives::Error> {
        engine_cmd!(self, GetAllUserOrders { account })
    }

    /// Get user fills
    pub async fn get_user_fills(
        &self,
        account: AccountAddress,
        limit: Option<usize>,
    ) -> std::result::Result<Vec<Fill>, hypercore_primitives::Error> {
        engine_cmd!(self, GetUserFills { account, limit })
    }

    /// Get recent trades
    pub async fn get_recent_trades(
        &self,
        market_id: MarketId,
        limit: Option<usize>,
    ) -> std::result::Result<Vec<Fill>, hypercore_primitives::Error> {
        engine_cmd!(self, GetRecentTrades { market_id, limit })
    }

    /// Shutdown the engine
    pub async fn shutdown(&self) {
        let _ = self.tx.send(EngineCommand::Shutdown).await;
    }

    /// Check if the channel is still open
    pub fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }

    // ========================================================================
    // Blocking methods (for ABCI FinalizeBlock / synchronous contexts)
    // ========================================================================

    /// Place an order (blocking)
    pub fn place_order_blocking(
        &self,
        account: AccountAddress,
        request: OrderRequest,
        timestamp: Timestamp,
    ) -> Result<(Order, Vec<Fill>)> {
        engine_cmd_blocking!(self, PlaceOrder { account, request, timestamp })?
    }

    /// Cancel an order (blocking)
    pub fn cancel_order_blocking(
        &self,
        account: AccountAddress,
        market_id: MarketId,
        order_id: OrderId,
    ) -> Result<Order> {
        engine_cmd_blocking!(self, CancelOrder { account, market_id, order_id })?
    }

    /// Cancel all orders in a market (blocking)
    pub fn cancel_all_blocking(
        &self,
        account: AccountAddress,
        market_id: MarketId,
    ) -> Result<Vec<Order>> {
        engine_cmd_blocking!(self, CancelAll { account, market_id })?
    }

    /// Cancel all orders across all markets (blocking)
    pub fn cancel_all_markets_blocking(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Vec<(MarketId, Vec<Order>)>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, CancelAllMarkets { account })
    }

    /// Update leverage (blocking)
    pub fn update_leverage_blocking(
        &self,
        account: AccountAddress,
        market_id: MarketId,
        leverage: u8,
    ) -> Result<()> {
        engine_cmd_blocking!(self, UpdateLeverage { account, market_id, leverage })?
    }

    /// Process funding (blocking)
    pub fn process_funding_blocking(
        &self,
        timestamp: Timestamp,
    ) -> std::result::Result<Vec<(MarketId, Decimal)>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, ProcessFunding { timestamp })
    }

    /// Process liquidations (blocking)
    pub fn process_liquidations_blocking(
        &self,
        timestamp: Timestamp,
    ) -> std::result::Result<Vec<(AccountAddress, MarketId, Decimal)>, hypercore_primitives::Error>
    {
        engine_cmd_blocking!(self, ProcessLiquidations { timestamp })
    }

    /// Check triggers (blocking)
    pub fn check_triggers_blocking(
        &self,
        market_id: MarketId,
        timestamp: Timestamp,
    ) -> std::result::Result<Vec<(Order, Vec<Fill>)>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, CheckTriggers { market_id, timestamp })
    }

    /// Remove a trigger order (blocking)
    pub fn remove_trigger_order_blocking(
        &self,
        order_id: OrderId,
    ) -> std::result::Result<Option<Order>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, RemoveTriggerOrder { order_id })
    }

    /// Set margin mode (blocking)
    pub fn set_margin_mode_blocking(
        &self,
        account: AccountAddress,
        mode: MarginMode,
    ) -> Result<()> {
        engine_cmd_blocking!(self, SetMarginMode { account, mode })?
    }

    /// Store block metadata (blocking)
    pub fn store_block_metadata_blocking(
        &self,
        height: u64,
        metadata: BlockMetadata,
    ) -> std::result::Result<(), hypercore_primitives::Error> {
        engine_cmd_blocking!(self, StoreBlockMetadata { height, metadata })
    }

    /// Get engine state snapshot (blocking)
    pub fn get_engine_snapshot_blocking(
        &self,
    ) -> std::result::Result<EngineStateSnapshot, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetEngineSnapshot {})
    }

    /// Peek at next order ID (blocking)
    pub fn peek_next_order_id_blocking(
        &self,
    ) -> std::result::Result<OrderId, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, PeekNextOrderId {})
    }

    /// Get all markets (blocking)
    pub fn get_all_markets_blocking(
        &self,
    ) -> std::result::Result<Vec<(MarketId, Market)>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetAllMarkets {})
    }

    /// Check if order exists in book (blocking)
    pub fn check_order_exists_blocking(
        &self,
        market_id: MarketId,
        order_id: OrderId,
    ) -> std::result::Result<bool, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, CheckOrderExists { market_id, order_id })
    }

    /// Get leverage (blocking)
    pub fn get_leverage_blocking(
        &self,
        account: AccountAddress,
        market_id: MarketId,
    ) -> std::result::Result<u8, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetLeverage { account, market_id })
    }

    /// Get all positions globally (blocking, for persistence)
    pub fn get_all_positions_global_blocking(
        &self,
    ) -> std::result::Result<
        HashMap<AccountAddress, HashMap<MarketId, Position>>,
        hypercore_primitives::Error,
    > {
        engine_cmd_blocking!(self, GetAllPositionsGlobal {})
    }

    /// Get all leverage globally (blocking, for persistence)
    pub fn get_all_leverage_global_blocking(
        &self,
    ) -> std::result::Result<
        HashMap<AccountAddress, HashMap<MarketId, u8>>,
        hypercore_primitives::Error,
    > {
        engine_cmd_blocking!(self, GetAllLeverageGlobal {})
    }

    /// Get account state (blocking)
    pub fn get_account_blocking(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Option<AccountState>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetAccount { account })
    }

    /// Get position (blocking)
    pub fn get_position_blocking(
        &self,
        account: AccountAddress,
        market_id: MarketId,
    ) -> std::result::Result<Option<Position>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetPosition { account, market_id })
    }

    /// Get all positions for an account (blocking)
    pub fn get_all_positions_blocking(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Vec<(MarketId, Position)>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetAllPositions { account })
    }

    /// Get orderbook snapshot (blocking)
    pub fn get_orderbook_blocking(
        &self,
        market_id: MarketId,
        depth: usize,
    ) -> std::result::Result<Option<crate::orderbook::L2Snapshot>, hypercore_primitives::Error>
    {
        engine_cmd_blocking!(self, GetOrderbook { market_id, depth })
    }

    /// Get user trigger orders (blocking)
    pub fn get_user_trigger_orders_blocking(
        &self,
        account: AccountAddress,
    ) -> std::result::Result<Vec<Order>, hypercore_primitives::Error> {
        engine_cmd_blocking!(self, GetUserTriggerOrders { account })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineConfig;
    use alloy_primitives::Address;
    use hypercore_primitives::{MarketConfig, OrderType};

    fn setup_engine() -> Engine {
        let mut engine = Engine::new(EngineConfig::default());
        engine.add_market(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        engine
    }

    #[tokio::test]
    async fn test_engine_handle_place_order() {
        let mut engine = setup_engine();
        engine.state.deposit(Address::repeat_byte(1), 10000_000000);

        let (handle, core) = EngineHandle::new(engine, 100);
        let core_handle = std::thread::spawn(move || core.run());

        let request = OrderRequest {
            market_id: 0,
            side: hypercore_primitives::OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
            trigger_price: None,
            trigger_direction: None,
        };

        let result = handle
            .place_order(Address::repeat_byte(1), request, 1000)
            .await;
        assert!(result.is_ok());

        let (order, fills) = result.unwrap();
        assert!(fills.is_empty());
        assert_eq!(order.remaining_size, Decimal::size("0.1"));

        handle.shutdown().await;
        core_handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_engine_handle_cancel_order() {
        let mut engine = setup_engine();
        engine.state.deposit(Address::repeat_byte(1), 10000_000000);

        let (handle, core) = EngineHandle::new(engine, 100);
        let core_handle = std::thread::spawn(move || core.run());

        let request = OrderRequest {
            market_id: 0,
            side: hypercore_primitives::OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
            trigger_price: None,
            trigger_direction: None,
        };

        let (order, _) = handle
            .place_order(Address::repeat_byte(1), request, 1000)
            .await
            .unwrap();

        let cancel_result = handle
            .cancel_order(Address::repeat_byte(1), 0, order.id)
            .await;
        assert!(cancel_result.is_ok());

        handle.shutdown().await;
        core_handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_engine_handle_matching() {
        let mut engine = setup_engine();
        engine.state.deposit(Address::repeat_byte(1), 10000_000000);
        engine.state.deposit(Address::repeat_byte(2), 10000_000000);

        let (handle, core) = EngineHandle::new(engine, 100);
        let core_handle = std::thread::spawn(move || core.run());

        let maker_req = OrderRequest {
            market_id: 0,
            side: hypercore_primitives::OrderSide::Sell,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
            trigger_price: None,
            trigger_direction: None,
        };
        let (_, maker_fills) = handle
            .place_order(Address::repeat_byte(1), maker_req, 1000)
            .await
            .unwrap();
        assert!(maker_fills.is_empty());

        let taker_req = OrderRequest {
            market_id: 0,
            side: hypercore_primitives::OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
            trigger_price: None,
            trigger_direction: None,
        };
        let (taker_order, taker_fills) = handle
            .place_order(Address::repeat_byte(2), taker_req, 2000)
            .await
            .unwrap();
        assert_eq!(taker_fills.len(), 1);
        assert!(taker_order.is_filled());

        handle.shutdown().await;
        core_handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_engine_handle_concurrent() {
        let mut engine = setup_engine();
        for i in 1..=10u8 {
            engine
                .state
                .deposit(Address::repeat_byte(i), 100000_000000);
        }

        let (handle, core) = EngineHandle::new(engine, 1000);
        let core_handle = std::thread::spawn(move || core.run());

        let mut tasks = Vec::new();
        for i in 1..=10u8 {
            let h = handle.clone();
            tasks.push(tokio::spawn(async move {
                let request = OrderRequest {
                    market_id: 0,
                    side: if i % 2 == 0 {
                        hypercore_primitives::OrderSide::Buy
                    } else {
                        hypercore_primitives::OrderSide::Sell
                    },
                    price: Decimal::price("65000"),
                    size: Decimal::size("0.01"),
                    order_type: OrderType::default(),
                    reduce_only: false,
                    client_order_id: None,
                    trigger_price: None,
                    trigger_direction: None,
                };
                h.place_order(Address::repeat_byte(i), request, i as u64 * 1000)
                    .await
            }));
        }

        for task in tasks {
            let result = task.await.unwrap();
            assert!(result.is_ok(), "Order should succeed: {:?}", result);
        }

        handle.shutdown().await;
        core_handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_engine_handle_snapshot() {
        let mut engine = setup_engine();
        engine.state.deposit(Address::repeat_byte(1), 10000_000000);

        let (handle, core) = EngineHandle::new(engine, 100);
        let core_handle = std::thread::spawn(move || core.run());

        // Place an order
        let request = OrderRequest {
            market_id: 0,
            side: hypercore_primitives::OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
            trigger_price: None,
            trigger_direction: None,
        };
        handle
            .place_order(Address::repeat_byte(1), request, 1000)
            .await
            .unwrap();

        // Get snapshot - should include the order
        let (reply_tx, reply_rx) = oneshot::channel();
        handle
            .tx
            .send(EngineCommand::GetEngineSnapshot { reply: reply_tx })
            .await
            .unwrap();
        let snapshot = reply_rx.await.unwrap();
        assert!(!snapshot.orders.is_empty());

        handle.shutdown().await;
        core_handle.join().unwrap();
    }
}
