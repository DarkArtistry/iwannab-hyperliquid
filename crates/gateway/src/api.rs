//! API request/response types

use hypercore_primitives::{AccountAddress, Decimal, MarketId, OrderId, Signature, SpotMarketId, TokenIndex};
use serde::{Deserialize, Serialize};

/// Info API request types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InfoRequest {
    /// Get exchange metadata
    Meta,
    /// Get all mid prices
    AllMids,
    /// Get L2 orderbook
    L2Book {
        coin: String,
        #[serde(default = "default_sig_figs")]
        n_sig_figs: u8,
    },
    /// Get user's clearinghouse state
    ClearinghouseState { user: String },
    /// Get user's open orders
    OpenOrders { user: String },
    /// Get user's fills
    UserFills {
        user: String,
        #[serde(rename = "startTime")]
        start_time: Option<u64>,
        #[serde(rename = "endTime")]
        end_time: Option<u64>,
    },
    /// Get funding history for a market
    FundingHistory {
        coin: String,
        #[serde(rename = "startTime")]
        start_time: Option<u64>,
        #[serde(rename = "endTime")]
        end_time: Option<u64>,
    },
    /// Get user's funding payment history
    UserFundingHistory {
        user: String,
        #[serde(rename = "startTime")]
        start_time: Option<u64>,
        #[serde(rename = "endTime")]
        end_time: Option<u64>,
    },
    /// Get recent trades
    RecentTrades {
        coin: String,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    /// Get candle snapshot
    CandleSnapshot {
        coin: String,
        interval: String,
        #[serde(rename = "startTime")]
        start_time: Option<u64>,
        #[serde(rename = "endTime")]
        end_time: Option<u64>,
    },
    // === Spot Market Queries ===
    /// Get spot exchange metadata (tokens and markets)
    SpotMeta,
    /// Get spot L2 orderbook
    SpotL2Book {
        coin: String,
        #[serde(default = "default_sig_figs")]
        n_sig_figs: u8,
    },
    /// Get all spot mid prices
    SpotAllMids,
    /// Get user's spot token balances
    SpotBalances { user: String },
    /// Get user's open spot orders
    SpotOpenOrders {
        user: String,
        /// Optional: filter by market
        market: Option<String>,
    },
    /// Get spot token info by index
    SpotTokenInfo { index: TokenIndex },
    // === Phase 2A: Unified Balance Queries ===
    /// Get unified balances (showing Core and EVM views)
    UnifiedBalances { user: String },
}

fn default_sig_figs() -> u8 {
    5
}

fn default_limit() -> u32 {
    100
}

/// Exchange API request (action + signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub action: ExchangeAction,
    pub nonce: u64,
    pub signature: SignatureWire,
}

/// Exchange action types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExchangeAction {
    /// Place orders
    Order {
        orders: Vec<OrderWire>,
        grouping: String,
    },
    /// Cancel orders by ID
    Cancel { cancels: Vec<CancelWire> },
    /// Cancel orders by client order ID
    CancelByCloid { cancels: Vec<CancelByCloidWire> },
    /// Cancel all orders
    CancelAll,
    /// Update leverage
    UpdateLeverage {
        asset: MarketId,
        #[serde(rename = "isCross")]
        is_cross: bool,
        leverage: u8,
    },
    /// Transfer USDC
    UsdTransfer {
        destination: String,
        amount: String,
    },
    /// Withdraw USDC
    Withdraw {
        destination: String,
        amount: String,
    },
    // === Spot Trading Actions ===
    /// Place spot order
    SpotOrder {
        orders: Vec<SpotOrderWire>,
        grouping: String,
    },
    /// Cancel spot orders
    SpotCancel { cancels: Vec<SpotCancelWire> },
    /// Cancel all spot orders
    SpotCancelAll {
        /// Optional: only cancel in specific market
        market: Option<SpotMarketId>,
    },
    // === Phase 2A: View Transfer Actions ===
    /// Transfer balance between Core and EVM views
    ///
    /// This is NOT a bridge - it adjusts views in the unified state.
    /// The total balance remains unchanged.
    ViewTransfer {
        /// Token index (0 = USDC)
        token: TokenIndex,
        /// Amount to transfer (as string for precision)
        amount: String,
        /// Direction: true = Core -> EVM, false = EVM -> Core
        #[serde(rename = "toEvm")]
        to_evm: bool,
    },
}

/// Order wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderWire {
    pub a: MarketId,
    pub b: bool,
    pub p: String,
    pub s: String,
    pub r: bool,
    pub t: OrderTypeWire,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

/// Order type wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OrderTypeWire {
    Limit { limit: LimitTif },
    Trigger { trigger: TriggerWire },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitTif {
    pub tif: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerWire {
    pub is_market: bool,
    pub trigger_px: String,
    pub tpsl: String,
}

/// Cancel wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelWire {
    pub a: MarketId,
    pub o: OrderId,
}

/// Cancel by CLOID wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelByCloidWire {
    pub asset: MarketId,
    pub cloid: String,
}

/// Spot order wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOrderWire {
    /// Spot market ID
    pub a: SpotMarketId,
    /// Is buy (true) or sell (false)
    pub b: bool,
    /// Price
    pub p: String,
    /// Size (in base token)
    pub s: String,
    /// Reduce only (not applicable for spot, but kept for consistency)
    #[serde(default)]
    pub r: bool,
    /// Order type
    pub t: OrderTypeWire,
    /// Client order ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

/// Spot cancel wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotCancelWire {
    /// Spot market ID
    pub a: SpotMarketId,
    /// Order ID to cancel
    pub o: OrderId,
}

/// Signature wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureWire {
    pub r: String,
    pub s: String,
    pub v: u8,
}

impl TryFrom<SignatureWire> for Signature {
    type Error = &'static str;

    fn try_from(wire: SignatureWire) -> Result<Self, Self::Error> {
        use alloy_primitives::B256;
        use std::str::FromStr;

        let r = B256::from_str(&wire.r).map_err(|_| "invalid r value")?;
        let s = B256::from_str(&wire.s).map_err(|_| "invalid s value")?;
        Ok(Signature { r, s, v: wire.v })
    }
}

/// API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Success(T),
    Error(ApiError),
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        ApiResponse::Success(data)
    }

    pub fn error(message: impl Into<String>) -> Self {
        ApiResponse::Error(ApiError {
            status: "err".to_string(),
            response: message.into(),
        })
    }
}

/// API error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub status: String,
    pub response: String,
}

/// Meta response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub universe: Vec<MarketMetaResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMetaResponse {
    pub name: String,
    pub sz_decimals: u8,
    pub max_leverage: u8,
    pub tick_size: String,
    pub lot_size: String,
    pub maker_fee: String,
    pub taker_fee: String,
}

/// L2 book response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookResponse {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<L2LevelResponse>, Vec<L2LevelResponse>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2LevelResponse {
    pub px: String,
    pub sz: String,
    pub n: u32,
}

/// Clearinghouse state response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseStateResponse {
    pub margin_summary: MarginSummaryResponse,
    pub cross_margin_summary: CrossMarginSummaryResponse,
    pub asset_positions: Vec<AssetPositionResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginSummaryResponse {
    pub account_value: String,
    pub total_ntl_pos: String,
    pub total_raw_usd: String,
    pub total_margin_used: String,
    pub withdrawable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossMarginSummaryResponse {
    pub account_value: String,
    pub total_ntl_pos: String,
    pub total_margin_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPositionResponse {
    pub position: PositionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionResponse {
    pub coin: String,
    pub szi: String,
    pub entry_px: String,
    pub unrealized_pnl: String,
    pub leverage: u8,
    pub liquidation_px: Option<String>,
    pub margin_used: String,
    pub return_on_equity: String,
}

/// Open order response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrderResponse {
    pub coin: String,
    pub oid: OrderId,
    pub side: String,
    pub limit_px: String,
    pub sz: String,
    pub orig_sz: String,
    pub timestamp: u64,
    pub cloid: Option<String>,
}

/// Fill response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillResponse {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub fee: String,
    pub oid: OrderId,
    pub tid: u64,
    pub crossed: bool,
    pub closed_pnl: String,
}

/// Funding rate response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRateResponse {
    pub coin: String,
    pub funding_rate: String,
    pub premium: String,
    pub time: u64,
}

/// Candle response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleResponse {
    #[serde(rename = "T")]
    pub time: u64,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "v")]
    pub volume: String,
    #[serde(rename = "n")]
    pub trades: u32,
}

/// Order placement response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponseData {
    #[serde(rename = "type")]
    pub response_type: String,
    pub data: OrderResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponseInner {
    pub statuses: Vec<OrderStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OrderStatus {
    Resting(RestingOrder),
    Filled(FilledOrder),
    Error(OrderError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestingOrder {
    pub resting: RestingOrderData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestingOrderData {
    pub oid: OrderId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilledOrder {
    pub filled: FilledOrderData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilledOrderData {
    pub total_sz: String,
    pub avg_px: String,
    pub oid: OrderId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderError {
    pub error: String,
}

/// Cancel response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelResponseData {
    #[serde(rename = "type")]
    pub response_type: String,
    pub data: CancelResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelResponseInner {
    pub statuses: Vec<CancelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CancelStatus {
    Success(String),
    Error(CancelError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelError {
    pub error: String,
}

// === Spot Response Types ===

/// Spot metadata response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotMetaResponse {
    pub tokens: Vec<SpotTokenResponse>,
    pub universe: Vec<SpotMarketMetaResponse>,
}

/// Spot token info response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotTokenResponse {
    pub index: TokenIndex,
    pub name: String,
    pub symbol: String,
    pub wei_decimals: u8,
    pub sz_decimals: u8,
    pub system_address: String,
}

/// Spot market metadata response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotMarketMetaResponse {
    pub id: SpotMarketId,
    pub name: String,
    pub base_token: TokenIndex,
    pub quote_token: TokenIndex,
    pub sz_decimals: u8,
    pub tick_size: String,
    pub lot_size: String,
    pub min_order_size: String,
    pub maker_fee: String,
    pub taker_fee: String,
}

/// Spot L2 book response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotL2BookResponse {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<L2LevelResponse>, Vec<L2LevelResponse>),
}

/// Spot balance response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotBalanceResponse {
    pub token_index: TokenIndex,
    pub symbol: String,
    pub total: String,
    pub reserved: String,
    pub available: String,
}

/// Spot open order response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotOpenOrderResponse {
    pub market: String,
    pub market_id: SpotMarketId,
    pub oid: OrderId,
    pub side: String,
    pub limit_px: String,
    pub sz: String,
    pub orig_sz: String,
    pub timestamp: u64,
    pub cloid: Option<String>,
}

// === Phase 2A: Unified Balance Response Types ===

/// Unified balance response (shows Core and EVM views)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedBalanceResponse {
    pub token_index: TokenIndex,
    pub symbol: String,
    /// Total balance (source of truth)
    pub total: String,
    /// Balance available for HyperCore trading
    pub core_view: String,
    /// Balance available for HyperEVM operations
    pub evm_view: String,
}

/// Unified balances list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedBalancesResponse {
    pub balances: Vec<UnifiedBalanceResponse>,
}

/// View transfer response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewTransferResponse {
    /// New Core view balance after transfer
    pub new_core_view: String,
    /// New EVM view balance after transfer
    pub new_evm_view: String,
    /// Total balance (unchanged)
    pub total: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_request_parsing() {
        let json = r#"{"type": "meta"}"#;
        let req: InfoRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, InfoRequest::Meta));
    }

    #[test]
    fn test_exchange_request_parsing() {
        let json = r#"{
            "action": {
                "type": "order",
                "orders": [{
                    "a": 0,
                    "b": true,
                    "p": "50000.0",
                    "s": "0.1",
                    "r": false,
                    "t": {"limit": {"tif": "Gtc"}}
                }],
                "grouping": "na"
            },
            "nonce": 1234567890,
            "signature": {
                "r": "0x123",
                "s": "0x456",
                "v": 27
            }
        }"#;

        let req: ExchangeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.nonce, 1234567890);
    }

    #[test]
    fn test_api_response_serialization() {
        let success: ApiResponse<MetaResponse> = ApiResponse::success(MetaResponse {
            universe: vec![],
        });

        let json = serde_json::to_string(&success).unwrap();
        assert!(json.contains("universe"));

        let error: ApiResponse<()> = ApiResponse::error("Something went wrong");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("err"));
    }
}
