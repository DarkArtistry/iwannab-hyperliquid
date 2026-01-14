//! API handlers for HTTP endpoints

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    AccountAddress, Decimal, OrderRequest, OrderSide, OrderType, TimeInForce,
};
use tokio::sync::RwLock;

use crate::api::{ExchangeAction, ExchangeRequest, InfoRequest};
use crate::server::AppState;

/// Handler for POST /info endpoint
pub async fn handle_info(
    State(state): State<AppState>,
    Json(request): Json<InfoRequest>,
) -> impl IntoResponse {
    match process_info_request(&state.engine, &state.spot_engine, request).await {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

/// Handler for POST /exchange endpoint
pub async fn handle_exchange(
    State(state): State<AppState>,
    Json(request): Json<ExchangeRequest>,
) -> impl IntoResponse {
    match process_exchange_request(&state.engine, &state.spot_engine, request).await {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

/// Process an info request
async fn process_info_request(
    engine: &Arc<RwLock<EngineState>>,
    spot_engine: &Arc<RwLock<SpotEngine>>,
    request: InfoRequest,
) -> Result<Value, HandlerError> {
    // Handle spot-specific requests first
    match &request {
        InfoRequest::SpotMeta
        | InfoRequest::SpotL2Book { .. }
        | InfoRequest::SpotAllMids
        | InfoRequest::SpotBalances { .. }
        | InfoRequest::SpotOpenOrders { .. }
        | InfoRequest::SpotTokenInfo { .. }
        | InfoRequest::UnifiedBalances { .. } => {
            return process_spot_info_request(spot_engine, request).await;
        }
        _ => {}
    }

    let engine = engine.read().await;

    match request {
        InfoRequest::Meta => {
            // Return exchange metadata
            let markets: Vec<_> = engine.get_all_markets().iter().map(|m| {
                json!({
                    "name": m.config.symbol,
                    "szDecimals": m.config.size_decimals,
                    "maxLeverage": m.config.max_leverage,
                })
            }).collect();

            Ok(json!({
                "universe": markets
            }))
        }

        InfoRequest::AllMids => {
            // Return mid prices for all markets
            let mids: std::collections::HashMap<String, String> = engine
                .get_all_markets()
                .iter()
                .filter_map(|m| {
                    engine.get_orderbook(m.config.id)
                        .and_then(|book| book.mid_price())
                        .map(|mid| (m.config.symbol.clone(), mid.to_string_trimmed()))
                })
                .collect();

            Ok(json!(mids))
        }

        InfoRequest::L2Book { coin, n_sig_figs: _ } => {
            // Find market by name
            let markets = engine.get_all_markets();
            let market = markets.iter()
                .find(|m| m.config.symbol == coin)
                .ok_or(HandlerError::MarketNotFound(coin.clone()))?;
            let market_id = market.config.id;

            let book = engine.get_orderbook(market_id)
                .ok_or(HandlerError::MarketNotFound(coin))?;

            let (bids, asks) = book.get_l2(20);

            Ok(json!({
                "levels": [
                    bids.iter().map(|l| [l.price.to_string_trimmed(), l.size.to_string_trimmed()]).collect::<Vec<_>>(),
                    asks.iter().map(|l| [l.price.to_string_trimmed(), l.size.to_string_trimmed()]).collect::<Vec<_>>(),
                ]
            }))
        }

        InfoRequest::ClearinghouseState { user } => {
            let address = parse_address(&user)?;

            if let Some(account) = engine.get_account(address) {
                Ok(json!({
                    "marginSummary": {
                        "accountValue": account.balance.to_string(),
                        "totalNtlPos": "0",
                        "totalRawUsd": account.balance.to_string(),
                        "totalMarginUsed": "0",
                        "withdrawable": account.balance.to_string(),
                    },
                    "crossMarginSummary": {},
                    "crossMaintenanceMarginUsed": "0",
                    "assetPositions": [],
                }))
            } else {
                Ok(json!({
                    "marginSummary": {
                        "accountValue": "0",
                        "totalNtlPos": "0",
                        "totalRawUsd": "0",
                        "totalMarginUsed": "0",
                        "withdrawable": "0",
                    },
                    "crossMarginSummary": {},
                    "crossMaintenanceMarginUsed": "0",
                    "assetPositions": [],
                }))
            }
        }

        InfoRequest::OpenOrders { user } => {
            let _address = parse_address(&user)?;
            // TODO: Implement open orders query
            Ok(json!([]))
        }

        InfoRequest::UserFills { user, .. } => {
            let _address = parse_address(&user)?;
            // TODO: Implement user fills query
            Ok(json!([]))
        }

        InfoRequest::UserFundingHistory { user, .. } => {
            let _address = parse_address(&user)?;
            // TODO: Implement user funding query
            Ok(json!([]))
        }

        InfoRequest::FundingHistory { coin, .. } => {
            let _coin = coin;
            // TODO: Implement funding history query
            Ok(json!([]))
        }

        InfoRequest::RecentTrades { coin, .. } => {
            let _coin = coin;
            // TODO: Implement recent trades query
            Ok(json!([]))
        }

        InfoRequest::CandleSnapshot { coin, interval, .. } => {
            let _coin = coin;
            let _interval = interval;
            // TODO: Implement candles query
            Ok(json!([]))
        }

        // Spot requests are handled above
        InfoRequest::SpotMeta
        | InfoRequest::SpotL2Book { .. }
        | InfoRequest::SpotAllMids
        | InfoRequest::SpotBalances { .. }
        | InfoRequest::SpotOpenOrders { .. }
        | InfoRequest::SpotTokenInfo { .. }
        | InfoRequest::UnifiedBalances { .. } => {
            unreachable!("Spot requests handled in process_spot_info_request")
        }
    }
}

/// Process spot-specific info requests
async fn process_spot_info_request(
    spot_engine: &Arc<RwLock<SpotEngine>>,
    request: InfoRequest,
) -> Result<Value, HandlerError> {
    let engine = spot_engine.read().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    match request {
        InfoRequest::SpotMeta => {
            // Return spot exchange metadata (tokens and markets)
            let tokens: Vec<_> = engine.state.get_all_tokens().iter().map(|t| {
                json!({
                    "index": t.index,
                    "name": t.name,
                    "symbol": t.symbol,
                    "weiDecimals": t.wei_decimals,
                    "szDecimals": t.sz_decimals,
                    "systemAddress": format!("{:?}", t.system_address),
                })
            }).collect();

            let markets: Vec<_> = engine.state.get_all_markets().iter().map(|m| {
                json!({
                    "id": m.config.id,
                    "name": m.config.symbol,
                    "baseToken": m.config.base_token,
                    "quoteToken": m.config.quote_token,
                    "tickSize": m.config.tick_size.to_string_trimmed(),
                    "lotSize": m.config.lot_size.to_string_trimmed(),
                    "minOrderSize": m.config.min_order_size.to_string_trimmed(),
                    "makerFee": m.config.maker_fee_rate.to_string_trimmed(),
                    "takerFee": m.config.taker_fee_rate.to_string_trimmed(),
                })
            }).collect();

            Ok(json!({
                "tokens": tokens,
                "universe": markets
            }))
        }

        InfoRequest::SpotL2Book { coin, n_sig_figs: _ } => {
            // Find spot market by name
            let market = engine.state.find_market_by_symbol(&coin)
                .ok_or(HandlerError::MarketNotFound(coin.clone()))?;
            let market_id = market.id();

            let book = engine.state.get_orderbook(market_id)
                .ok_or(HandlerError::MarketNotFound(coin))?;

            let (bids, asks) = book.get_l2(20);

            Ok(json!({
                "coin": market.config.symbol,
                "time": now,
                "levels": [
                    bids.iter().map(|l| [l.price.to_string_trimmed(), l.size.to_string_trimmed()]).collect::<Vec<_>>(),
                    asks.iter().map(|l| [l.price.to_string_trimmed(), l.size.to_string_trimmed()]).collect::<Vec<_>>(),
                ]
            }))
        }

        InfoRequest::SpotAllMids => {
            // Return mid prices for all spot markets
            let mids: std::collections::HashMap<String, String> = engine.state
                .get_all_markets()
                .iter()
                .filter_map(|m| {
                    engine.state.get_orderbook(m.id())
                        .and_then(|book| book.mid_price())
                        .map(|mid| (m.config.symbol.clone(), mid.to_string_trimmed()))
                })
                .collect();

            Ok(json!(mids))
        }

        InfoRequest::SpotBalances { user } => {
            let address = parse_address(&user)?;

            let balances: Vec<_> = engine.state.get_all_balances(address)
                .iter()
                .filter_map(|(token_index, balance)| {
                    let token = engine.state.get_token(*token_index)?;
                    Some(json!({
                        "tokenIndex": token.index,
                        "symbol": token.symbol,
                        "total": balance.total.to_string_trimmed(),
                        "reserved": balance.reserved.to_string_trimmed(),
                        "available": balance.available().to_string_trimmed(),
                    }))
                })
                .collect();

            Ok(json!(balances))
        }

        InfoRequest::SpotOpenOrders { user, market } => {
            let address = parse_address(&user)?;

            // If market is specified, only get orders for that market
            let orders: Vec<_> = if let Some(market_name) = market {
                let m = engine.state.find_market_by_symbol(&market_name)
                    .ok_or(HandlerError::MarketNotFound(market_name))?;

                engine.state.get_orders_by_account(address, m.id())
                    .iter()
                    .map(|o| json!({
                        "market": m.config.symbol,
                        "marketId": m.id(),
                        "oid": o.id,
                        "side": if matches!(o.side, OrderSide::Buy) { "B" } else { "A" },
                        "limitPx": o.price.to_string_trimmed(),
                        "sz": o.remaining_size.to_string_trimmed(),
                        "origSz": o.original_size.to_string_trimmed(),
                        "timestamp": o.timestamp,
                        "cloid": o.client_order_id,
                    }))
                    .collect()
            } else {
                // Get orders from all spot markets
                engine.state.get_all_markets()
                    .iter()
                    .flat_map(|m| {
                        engine.state.get_orders_by_account(address, m.id())
                            .iter()
                            .map(|o| json!({
                                "market": m.config.symbol.clone(),
                                "marketId": m.id(),
                                "oid": o.id,
                                "side": if matches!(o.side, OrderSide::Buy) { "B" } else { "A" },
                                "limitPx": o.price.to_string_trimmed(),
                                "sz": o.remaining_size.to_string_trimmed(),
                                "origSz": o.original_size.to_string_trimmed(),
                                "timestamp": o.timestamp,
                                "cloid": o.client_order_id.clone(),
                            }))
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };

            Ok(json!(orders))
        }

        InfoRequest::SpotTokenInfo { index } => {
            let token = engine.state.get_token(index)
                .ok_or(HandlerError::Internal(format!("Token {} not found", index)))?;

            Ok(json!({
                "index": token.index,
                "name": token.name,
                "symbol": token.symbol,
                "weiDecimals": token.wei_decimals,
                "szDecimals": token.sz_decimals,
                "maxSupply": token.max_supply.to_string_trimmed(),
                "circulatingSupply": token.circulating_supply.to_string_trimmed(),
                "systemAddress": format!("{:?}", token.system_address),
                "deployer": format!("{:?}", token.deployer),
            }))
        }

        // Phase 2A: Unified Balance Queries
        InfoRequest::UnifiedBalances { user } => {
            let address = parse_address(&user)?;

            // Read from unified state through the spot engine
            let unified_state = engine.state.unified_state();
            let unified = unified_state.read().unwrap();

            // Get all balances for this user and convert to response format
            let balances: Vec<_> = unified.get_all_balances(address)
                .iter()
                .filter_map(|(token_index, unified_balance)| {
                    let token = engine.state.get_token(*token_index)?;
                    Some(json!({
                        "tokenIndex": token.index,
                        "symbol": token.symbol,
                        "total": unified_balance.total.to_string_trimmed(),
                        "coreView": unified_balance.core_view.to_string_trimmed(),
                        "evmView": unified_balance.evm_view.to_string_trimmed(),
                    }))
                })
                .collect();

            Ok(json!({
                "balances": balances
            }))
        }

        // Other requests are handled in process_info_request
        _ => Err(HandlerError::Internal("Unexpected request type".to_string())),
    }
}

/// Process an exchange request
async fn process_exchange_request(
    engine: &Arc<RwLock<EngineState>>,
    spot_engine: &Arc<RwLock<SpotEngine>>,
    request: ExchangeRequest,
) -> Result<Value, HandlerError> {
    // Verify signature (stub - in production, implement proper EIP-712 verification)
    let sender = verify_signature(&request)?;

    // Handle spot-specific actions and view transfers
    match &request.action {
        ExchangeAction::SpotOrder { .. }
        | ExchangeAction::SpotCancel { .. }
        | ExchangeAction::SpotCancelAll { .. }
        | ExchangeAction::ViewTransfer { .. } => {
            return process_spot_exchange_request(spot_engine, sender, request).await;
        }
        _ => {}
    }

    let mut engine = engine.write().await;

    match request.action {
        ExchangeAction::Order { orders, grouping: _ } => {
            let mut results = Vec::new();

            for order_wire in orders {
                // Parse order - a is already the market_id
                let market_id = order_wire.a;

                // Verify market exists
                if !engine.has_market(market_id) {
                    return Err(HandlerError::MarketNotFound(market_id.to_string()));
                }

                let price = Decimal::from_str(&order_wire.p)
                    .map_err(|_| HandlerError::InvalidParameter("price".to_string()))?;
                let size = Decimal::from_str(&order_wire.s)
                    .map_err(|_| HandlerError::InvalidParameter("size".to_string()))?;

                // TODO: Create and execute order through engine
                results.push(json!({
                    "status": "pending",
                    "market": order_wire.a,
                    "price": price.to_string_trimmed(),
                    "size": size.to_string_trimmed(),
                }));
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "order",
                    "data": {
                        "statuses": results
                    }
                }
            }))
        }

        ExchangeAction::Cancel { cancels } => {
            let mut results = Vec::new();

            for cancel in cancels {
                // TODO: Cancel order through engine
                results.push(json!({
                    "status": "pending",
                    "oid": cancel.o,
                    "market": cancel.a,
                }));
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "cancel",
                    "data": {
                        "statuses": results
                    }
                }
            }))
        }

        ExchangeAction::CancelByCloid { cancels } => {
            let mut results = Vec::new();

            for cancel in cancels {
                // TODO: Cancel order by client order ID
                results.push(json!({
                    "status": "pending",
                    "cloid": cancel.cloid,
                }));
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "cancelByCloid",
                    "data": {
                        "statuses": results
                    }
                }
            }))
        }

        ExchangeAction::UsdTransfer { destination, amount } => {
            let dest = parse_address(&destination)?;
            let _amount = Decimal::from_str(&amount)
                .map_err(|_| HandlerError::InvalidParameter("amount".to_string()))?;

            // TODO: Execute transfer
            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "usdTransfer",
                    "data": {
                        "destination": destination,
                        "amount": amount
                    }
                }
            }))
        }

        ExchangeAction::Withdraw { destination, amount } => {
            let _dest = parse_address(&destination)?;
            let _amount = Decimal::from_str(&amount)
                .map_err(|_| HandlerError::InvalidParameter("amount".to_string()))?;

            // TODO: Execute withdrawal
            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "withdraw",
                    "data": {
                        "destination": destination,
                        "amount": amount
                    }
                }
            }))
        }

        ExchangeAction::UpdateLeverage { asset, is_cross, leverage } => {
            // TODO: Update leverage through engine
            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "updateLeverage",
                    "data": {
                        "asset": asset,
                        "isCross": is_cross,
                        "leverage": leverage
                    }
                }
            }))
        }

        ExchangeAction::CancelAll => {
            // TODO: Cancel all orders for user
            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "cancelAll",
                    "data": {}
                }
            }))
        }

        // Spot actions and view transfers are handled above
        ExchangeAction::SpotOrder { .. }
        | ExchangeAction::SpotCancel { .. }
        | ExchangeAction::SpotCancelAll { .. }
        | ExchangeAction::ViewTransfer { .. } => {
            unreachable!("Spot actions handled in process_spot_exchange_request")
        }
    }
}

/// Process spot-specific exchange requests
async fn process_spot_exchange_request(
    spot_engine: &Arc<RwLock<SpotEngine>>,
    sender: AccountAddress,
    request: ExchangeRequest,
) -> Result<Value, HandlerError> {
    let mut engine = spot_engine.write().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    match request.action {
        ExchangeAction::SpotOrder { orders, grouping: _ } => {
            let mut results = Vec::new();

            for order_wire in orders {
                let market_id = order_wire.a;

                // Verify market exists
                let market = engine.state.get_market(market_id)
                    .ok_or(HandlerError::MarketNotFound(market_id.to_string()))?
                    .clone();

                // Parse price and size
                let price = Decimal::from_str(&order_wire.p)
                    .map_err(|_| HandlerError::InvalidParameter("price".to_string()))?;

                // Get token decimals for proper size parsing
                let base_token = engine.state.get_token(market.config.base_token)
                    .ok_or(HandlerError::Internal("Base token not found".to_string()))?;
                let size = Decimal::from_str_exact(&order_wire.s, base_token.wei_decimals)
                    .ok_or(HandlerError::InvalidParameter("size".to_string()))?;

                let side = if order_wire.b { OrderSide::Buy } else { OrderSide::Sell };

                // Parse order type
                let order_type = parse_order_type(&order_wire.t)?;

                let request = OrderRequest {
                    market_id,
                    side,
                    price,
                    size,
                    order_type,
                    reduce_only: false, // Not applicable for spot
                    client_order_id: order_wire.c.clone(),
                };

                match engine.place_order(sender, market_id, request, now) {
                    Ok((order, fills)) => {
                        if order.is_filled() {
                            // Calculate average fill price
                            let total_value: i128 = fills.iter()
                                .map(|f| f.quote_size.raw())
                                .sum();
                            let total_size: i128 = fills.iter()
                                .map(|f| f.base_size.raw())
                                .sum();
                            let avg_px = if total_size > 0 {
                                Decimal::from_raw(total_value * 10000 / total_size, 4).to_string_trimmed()
                            } else {
                                "0".to_string()
                            };

                            results.push(json!({
                                "filled": {
                                    "totalSz": order.original_size.to_string_trimmed(),
                                    "avgPx": avg_px,
                                    "oid": order.id
                                }
                            }));
                        } else if !fills.is_empty() {
                            // Partially filled, resting
                            results.push(json!({
                                "resting": {
                                    "oid": order.id
                                }
                            }));
                        } else {
                            // Not filled, resting
                            results.push(json!({
                                "resting": {
                                    "oid": order.id
                                }
                            }));
                        }
                    }
                    Err(e) => {
                        results.push(json!({
                            "error": e.to_string()
                        }));
                    }
                }
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "spotOrder",
                    "data": {
                        "statuses": results
                    }
                }
            }))
        }

        ExchangeAction::SpotCancel { cancels } => {
            let mut results = Vec::new();

            for cancel in cancels {
                match engine.cancel_order(sender, cancel.a, cancel.o) {
                    Ok(_) => {
                        results.push(json!("success"));
                    }
                    Err(e) => {
                        results.push(json!({
                            "error": e.to_string()
                        }));
                    }
                }
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "spotCancel",
                    "data": {
                        "statuses": results
                    }
                }
            }))
        }

        ExchangeAction::SpotCancelAll { market } => {
            let mut count = 0;

            if let Some(market_id) = market {
                // Cancel all in specific market
                match engine.cancel_all_orders(sender, market_id) {
                    Ok(canceled) => count = canceled.len(),
                    Err(_) => {}
                }
            } else {
                // Collect all market IDs first to avoid borrow issue
                let market_ids: Vec<_> = engine.state.get_all_markets()
                    .iter()
                    .map(|m| m.id())
                    .collect();

                // Cancel all in all spot markets
                for market_id in market_ids {
                    if let Ok(canceled) = engine.cancel_all_orders(sender, market_id) {
                        count += canceled.len();
                    }
                }
            }

            Ok(json!({
                "status": "ok",
                "response": {
                    "type": "spotCancelAll",
                    "data": {
                        "canceledCount": count
                    }
                }
            }))
        }

        // Phase 2A: View Transfer - NOT a bridge, just view adjustment
        ExchangeAction::ViewTransfer { token, amount, to_evm } => {
            // Parse amount based on token decimals
            let token_info = engine.state.get_token(token)
                .ok_or(HandlerError::Internal(format!("Token {} not found", token)))?;
            let decimal_amount = Decimal::from_str_exact(&amount, token_info.wei_decimals)
                .ok_or(HandlerError::InvalidParameter("amount".to_string()))?;

            // Execute view transfer through spot engine
            match engine.view_transfer(sender, token, decimal_amount, to_evm) {
                Ok(()) => {
                    // Get updated balances to return in response
                    let unified_state = engine.state.unified_state();
                    let unified = unified_state.read().unwrap();
                    let balance = unified.get_balance_or_default(sender, token);

                    Ok(json!({
                        "status": "ok",
                        "response": {
                            "type": "viewTransfer",
                            "data": {
                                "newCoreView": balance.core_view.to_string_trimmed(),
                                "newEvmView": balance.evm_view.to_string_trimmed(),
                                "total": balance.total.to_string_trimmed()
                            }
                        }
                    }))
                }
                Err(e) => {
                    Err(HandlerError::Internal(e.to_string()))
                }
            }
        }

        // Other actions are handled in process_exchange_request
        _ => Err(HandlerError::Internal("Unexpected action type".to_string())),
    }
}

/// Parse order type from wire format
fn parse_order_type(wire: &crate::api::OrderTypeWire) -> Result<OrderType, HandlerError> {
    match wire {
        crate::api::OrderTypeWire::Limit { limit } => {
            let tif = match limit.tif.to_lowercase().as_str() {
                "gtc" => TimeInForce::Gtc,
                "ioc" => TimeInForce::Ioc,
                "fok" => TimeInForce::Fok,
                "alo" | "postonly" => TimeInForce::Alo,
                _ => TimeInForce::Gtc,
            };
            Ok(OrderType::Limit { tif })
        }
        crate::api::OrderTypeWire::Trigger { .. } => {
            // Trigger orders not yet supported for spot
            Err(HandlerError::InvalidParameter("Trigger orders not supported for spot".to_string()))
        }
    }
}

/// Verify signature and extract sender address
/// NOTE: This is a stub implementation for development.
/// Production must implement proper EIP-712 signature verification.
fn verify_signature(request: &ExchangeRequest) -> Result<AccountAddress, HandlerError> {
    // STUB: Extract address from signature r value for testing
    // In production, this MUST verify the EIP-712 signature properly
    let sig = &request.signature;
    let r_hex = sig.r.strip_prefix("0x").unwrap_or(&sig.r);

    if r_hex.len() >= 40 {
        let addr_hex = &r_hex[r_hex.len() - 40..];
        parse_address(&format!("0x{}", addr_hex))
    } else {
        Err(HandlerError::InvalidSignature)
    }
}

/// Parse hex address string to AccountAddress
fn parse_address(s: &str) -> Result<AccountAddress, HandlerError> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);

    if hex_str.len() != 40 {
        return Err(HandlerError::InvalidAddress(s.to_string()));
    }

    let bytes = hex::decode(hex_str)
        .map_err(|_| HandlerError::InvalidAddress(s.to_string()))?;

    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Ok(AccountAddress::from(addr))
}

/// Handler errors
#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    #[error("Market not found: {0}")]
    MarketNotFound(String),
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("Order not found: {0}")]
    OrderNotFound(u64),
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_address() {
        let addr = parse_address("0x1234567890123456789012345678901234567890").unwrap();
        assert_eq!(addr.as_slice()[0], 0x12);
    }

    #[test]
    fn test_parse_address_no_prefix() {
        let addr = parse_address("1234567890123456789012345678901234567890").unwrap();
        assert_eq!(addr.as_slice()[0], 0x12);
    }
}
