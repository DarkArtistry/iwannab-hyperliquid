//! HyperCore Primitives
//!
//! Shared types and utilities used across all HyperCore components.

pub mod decimal;
pub mod eip712;
pub mod error;
pub mod events;
pub mod market;
pub mod order;
pub mod position;
pub mod spot;
pub mod types;
pub mod unified_state;

pub use decimal::Decimal;
pub use error::{Error, Result};
pub use market::{Market, MarketConfig, MarketId, MarketMeta, MarketState, MarketStatus};
pub use order::{
    CancelReason, ClientOrderId, Order, OrderId, OrderKey, OrderRequest, OrderSide, OrderStatus,
    OrderType, TimeInForce,
};
pub use position::{Position, PositionSummary};
pub use spot::{
    SpotBalance, SpotFill, SpotMarket, SpotMarketConfig, SpotMarketId, SpotMarketState,
    SpotToken, SpotTokenDeployParams, SpotTokenTransfer, TokenIndex, TokenType,
};
pub use types::*;
pub use unified_state::{
    new_shared_unified_state, SharedUnifiedState, UnifiedBalance, UnifiedState, UnifiedStateSnapshot,
    ViewTransferError,
};
pub use events::{
    BlockEvent, FillEvent, FundingAppliedEvent, LeverageUpdatedEvent, LiquidationEvent,
    OrderCanceledEvent, OrderPlacedEvent, PositionUpdatedEvent, UsdTransferEvent,
};
