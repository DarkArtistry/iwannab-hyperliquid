//! HyperCore application logic
//!
//! Phase 2B: Full implementation with transaction handlers connected to unified state.

use hypercore_engine::EngineState;
use hypercore_primitives::{
    AccountAddress, BlockHeight, Decimal, MarketId,
    SharedUnifiedState, Timestamp, new_shared_unified_state,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::state::{AppState, SharedEngineState, SharedSpotEngine};
use crate::tx::{OrderWire, Transaction, TransactionType};

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
    pub fn init_from_genesis(&mut self, genesis_bytes: &[u8]) -> Result<(), AppError> {
        let genesis: GenesisState = serde_json::from_slice(genesis_bytes)
            .map_err(|e| AppError::GenesisError(e.to_string()))?;

        // Initialize markets - need async context for write lock
        // For genesis, we use blocking writes since this happens at startup
        let engine = self.state.engine.blocking_write();
        // Note: Markets would be added here, but we need mutable access
        // For now, markets are initialized in node/main.rs
        drop(engine);

        // Initialize accounts with balances in unified state
        let mut unified = self.state.unified_state.write().unwrap();
        for (address, balance) in genesis.balances {
            unified.credit(address, 0, Decimal::from_raw(balance, 6));
        }

        Ok(())
    }

    /// Check transaction validity
    pub fn check_tx(&self, tx: &Transaction) -> Result<(), AppError> {
        // Basic validation
        self.state.validate_tx(tx)?;
        Ok(())
    }

    /// Execute a transaction (sync version for ABCI)
    ///
    /// This is the main entry point for transaction execution from the ABCI layer.
    /// Uses blocking operations internally to work in sync context.
    pub fn execute_tx(&mut self, tx: &Transaction, timestamp: Timestamp) -> Result<TxResult, AppError> {
        // Validate
        self.state.validate_tx(tx)?;

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
        };

        // Increment nonce
        self.state.increment_nonce(sender);

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
        let mut events = Vec::new();

        for order_wire in orders {
            let market_id = order_wire.a;

            // Market IDs >= 128 are spot markets, < 128 are perps
            if market_id >= 128 {
                // Spot order
                if let Some(spot_engine) = &self.state.spot_engine {
                    let order_request = order_wire.to_order_request(sender)
                        .map_err(|e| AppError::TxError(e))?;

                    let mut engine = spot_engine.blocking_write();
                    // SpotEngine.place_order takes: (account, market_id, request, timestamp)
                    match engine.place_order(sender, market_id, order_request, timestamp) {
                        Ok((order, fills)) => {
                            events.push(Event::new("spot_order")
                                .add_attribute("order_id", &order.id.to_string())
                                .add_attribute("sender", &format!("{:?}", sender))
                                .add_attribute("market_id", &market_id.to_string())
                                .add_attribute("fills", &fills.len().to_string()));
                        }
                        Err(e) => {
                            tracing::warn!("Spot order failed: {}", e);
                        }
                    }
                }
            } else {
                // Perpetual order - currently handled directly by gateway
                // TODO: Route through full Engine orchestrator when ABCI is fully integrated
                events.push(Event::new("perp_order_queued")
                    .add_attribute("sender", &format!("{:?}", sender))
                    .add_attribute("market_id", &market_id.to_string())
                    .add_attribute("status", "pending_integration"));
            }
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

        for cancel in cancels {
            let market_id = cancel.a;
            let order_id = cancel.o;

            if market_id >= 128 {
                // Spot cancel
                if let Some(spot_engine) = &self.state.spot_engine {
                    let mut engine = spot_engine.blocking_write();
                    match engine.cancel_order(sender, market_id, order_id) {
                        Ok(_) => {
                            events.push(Event::new("spot_cancel")
                                .add_attribute("order_id", &order_id.to_string())
                                .add_attribute("market_id", &market_id.to_string()));
                        }
                        Err(e) => {
                            tracing::warn!("Spot cancel failed: {}", e);
                        }
                    }
                }
            } else {
                // Perp cancel - handled directly by gateway
                events.push(Event::new("perp_cancel_queued")
                    .add_attribute("order_id", &order_id.to_string())
                    .add_attribute("market_id", &market_id.to_string())
                    .add_attribute("status", "pending_integration"));
            }
        }

        Ok(events)
    }

    /// Execute cancel by client order ID (sync version)
    fn execute_cancel_by_cloid_sync(
        &mut self,
        _sender: AccountAddress,
        cancels: &[crate::tx::CancelByCloidWire],
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();

        for cancel in cancels {
            // TODO: Implement cancel by cloid - requires cloid index in engine
            events.push(Event::new("cancel_cloid_queued")
                .add_attribute("cloid", &cancel.cloid)
                .add_attribute("market_id", &cancel.asset.to_string())
                .add_attribute("status", "pending_implementation"));
        }

        Ok(events)
    }

    /// Execute cancel all orders (sync version)
    fn execute_cancel_all_sync(
        &mut self,
        sender: AccountAddress,
    ) -> Result<Vec<Event>, AppError> {
        let mut events = Vec::new();
        let mut cancelled_count = 0;

        // Cancel all spot orders (iterate over spot markets: 128-255)
        if let Some(spot_engine) = &self.state.spot_engine {
            let mut engine = spot_engine.blocking_write();
            // Get all spot markets and cancel orders in each
            let markets: Vec<_> = engine.state.get_all_markets().iter().map(|m| m.config.id).collect();
            for market_id in markets {
                if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                    cancelled_count += canceled.len();
                }
            }
        }

        // Perp cancel all - would need full Engine integration
        events.push(Event::new("cancel_all")
            .add_attribute("sender", &format!("{:?}", sender))
            .add_attribute("spot_cancelled", &cancelled_count.to_string())
            .add_attribute("perp_status", "pending_integration"));

        Ok(events)
    }

    /// Execute leverage update (sync version)
    fn execute_update_leverage_sync(
        &mut self,
        sender: AccountAddress,
        asset: MarketId,
        leverage: u8,
    ) -> Result<Vec<Event>, AppError> {
        let mut engine = self.state.engine.blocking_write();
        engine.set_leverage(sender, asset, leverage);

        Ok(vec![Event::new("update_leverage")
            .add_attribute("asset", &asset.to_string())
            .add_attribute("leverage", &leverage.to_string())])
    }

    /// Execute USD transfer (sync version)
    fn execute_usd_transfer_sync(
        &mut self,
        sender: AccountAddress,
        destination: AccountAddress,
        amount: &str,
    ) -> Result<Vec<Event>, AppError> {
        let amount_decimal = Decimal::from_str(amount)
            .map_err(|_| AppError::InvalidAmount)?;

        // Transfer using unified state
        let mut unified = self.state.unified_state.write().unwrap();

        // Check balance
        let sender_balance = unified.get_core_view(sender, 0);
        if sender_balance < amount_decimal {
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
        let amount_decimal = Decimal::from_str(amount)
            .map_err(|_| AppError::InvalidAmount)?;

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
    fn execute_evm_action_sync(
        &mut self,
        sender: AccountAddress,
        action_type: u8,
        _data: &[u8],
    ) -> Result<Vec<Event>, AppError> {
        // EVM actions are processed by the EVM executor
        // This is a passthrough for queued writes from CoreWriter

        match action_type {
            0 => {
                // Placeholder for deposit from EVM to Core
                Ok(vec![Event::new("evm_deposit")
                    .add_attribute("sender", &format!("{:?}", sender))])
            }
            1 => {
                // Placeholder for withdraw from Core to EVM
                Ok(vec![Event::new("evm_withdraw")
                    .add_attribute("sender", &format!("{:?}", sender))])
            }
            2 => {
                // View transfer: Core -> EVM
                Ok(vec![Event::new("view_transfer_to_evm")
                    .add_attribute("sender", &format!("{:?}", sender))])
            }
            3 => {
                // View transfer: EVM -> Core
                Ok(vec![Event::new("view_transfer_to_core")
                    .add_attribute("sender", &format!("{:?}", sender))])
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
    pub fn end_block(&mut self) -> Vec<ValidatorUpdate> {
        // TODO: Process funding, liquidations, etc.
        vec![]
    }

    /// Commit state changes
    pub fn commit(&mut self) -> [u8; 32] {
        self.state.compute_app_hash()
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

        // Use blocking read for sync context
        let engine = self.state.engine.blocking_read();
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

        let engine = self.state.engine.blocking_read();
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

        let engine = self.state.engine.blocking_read();
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisState {
    pub chain_id: String,
    pub markets: Vec<GenesisMarket>,
    pub balances: Vec<(AccountAddress, i128)>,
}

/// Genesis market configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisMarket {
    pub id: MarketId,
    pub symbol: String,
    pub max_leverage: u8,
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
