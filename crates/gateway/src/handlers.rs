//! API handlers for HTTP endpoints
//!
//! Phase 2B: Handlers now route perp transactions through the ABCI layer
//! for proper consensus and state management.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use sha3::Digest;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hypercore_chain::HyperCoreApp;
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    AccountAddress, Decimal, OrderRequest, OrderSide, OrderType, Signature, TimeInForce,
};
use tokio::sync::RwLock;

use crate::api::{ExchangeAction, ExchangeRequest, InfoRequest};
use crate::server::AppState;

/// Handler for POST /info endpoint
pub async fn handle_info(
    State(state): State<AppState>,
    Json(request): Json<InfoRequest>,
) -> impl IntoResponse {
    // Validate request first (fail-fast)
    if let Err(e) = state.validator.validate_info_request(&request) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Validation error: {}", e)})),
        );
    }

    match process_info_request(&state.engine, &state.spot_engine, &state.app, request).await {
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
    // Validate request first (fail-fast)
    if let Err(e) = state.validator.validate_exchange_request(&request) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Validation error: {}", e)})),
        );
    }

    match process_exchange_request(&state.engine, &state.spot_engine, &state.app, request).await {
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
    app: &Arc<RwLock<HyperCoreApp>>,
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
        // Handle state proof requests (Phase 3C)
        InfoRequest::StateInfo | InfoRequest::StateProof { .. } => {
            return process_state_proof_request(app, request).await;
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
            let address = parse_address(&user)?;
            let orders = engine.get_all_user_orders(address);
            let order_json: Vec<Value> = orders.iter().map(|o| json!({
                "coin": coin_name_from_market_id(o.market_id),
                "oid": o.id,
                "side": if o.side == hypercore_primitives::OrderSide::Buy { "B" } else { "A" },
                "limitPx": o.price.to_string_trimmed(),
                "sz": o.remaining_size.to_string_trimmed(),
                "origSz": o.original_size.to_string_trimmed(),
                "timestamp": o.timestamp,
                "cloid": o.client_order_id.clone(),
            })).collect();
            Ok(json!(order_json))
        }

        InfoRequest::UserFills { user, .. } => {
            let address = parse_address(&user)?;
            let fills = engine.get_user_fills(address, Some(100));
            let fills_json: Vec<Value> = fills.iter().map(|f| json!({
                "coin": coin_name_from_market_id(f.market_id),
                "px": Decimal::from_raw(f.price as i128, 8).to_string_trimmed(),
                "sz": Decimal::from_raw(f.size as i128, 6).to_string_trimmed(),
                "side": if f.is_taker_buy { "B" } else { "A" },
                "time": f.timestamp,
                "fee": Decimal::from_raw(f.taker_fee as i128, 6).to_string_trimmed(),
                "oid": f.taker_order_id,
            })).collect();
            Ok(json!(fills_json))
        }

        InfoRequest::UserFundingHistory { user, .. } => {
            let address = parse_address(&user)?;
            let history = engine.get_user_funding_history(address, Some(100));
            let history_json: Vec<Value> = history.iter().map(|f| json!({
                "coin": coin_name_from_market_id(f.market_id),
                "fundingRate": Decimal::from_raw(f.funding_rate, 8).to_string_trimmed(),
                "szi": Decimal::from_raw(f.position_size, 6).to_string_trimmed(),
                "usdc": Decimal::from_raw(f.payment, 6).to_string_trimmed(),
                "time": f.timestamp,
            })).collect();
            Ok(json!(history_json))
        }

        InfoRequest::FundingHistory { coin, .. } => {
            let market_id = market_id_from_coin(&coin);
            let history = engine.get_market_funding_history(market_id, Some(100));
            let history_json: Vec<Value> = history.iter().map(|(ts, rate)| json!({
                "coin": coin,
                "fundingRate": Decimal::from_raw(*rate, 8).to_string_trimmed(),
                "time": ts,
            })).collect();
            Ok(json!(history_json))
        }

        InfoRequest::RecentTrades { coin, .. } => {
            let market_id = market_id_from_coin(&coin);
            let trades = engine.get_recent_trades(market_id, Some(50));
            let trades_json: Vec<Value> = trades.iter().map(|t| json!({
                "coin": coin,
                "side": if t.is_taker_buy { "B" } else { "A" },
                "px": Decimal::from_raw(t.price as i128, 8).to_string_trimmed(),
                "sz": Decimal::from_raw(t.size as i128, 6).to_string_trimmed(),
                "time": t.timestamp,
                "tid": t.taker_order_id,
            })).collect();
            Ok(json!(trades_json))
        }

        InfoRequest::CandleSnapshot { coin, interval, .. } => {
            // Candles require aggregation from trades - return empty for now
            // A proper implementation would aggregate recent trades into OHLCV buckets
            let _market_id = market_id_from_coin(&coin);
            let _interval = interval;
            // Candle aggregation is complex and typically done by a separate indexer
            // For now, return empty array - real implementation would query indexed data
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

        // State proof requests are handled above
        InfoRequest::StateInfo | InfoRequest::StateProof { .. } => {
            unreachable!("State proof requests handled in process_state_proof_request")
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

/// Process state proof requests (Phase 3C)
///
/// Handles requests for state information and Merkle proofs:
/// - StateInfo: Returns block height, app hash, and state roots
/// - StateProof: Returns a Merkle proof for a user's balance
async fn process_state_proof_request(
    app: &Arc<RwLock<HyperCoreApp>>,
    request: InfoRequest,
) -> Result<Value, HandlerError> {
    let app_guard = app.read().await;

    match request {
        InfoRequest::StateInfo => {
            // Return current state information
            let height = app_guard.current_height();
            let unified_root = app_guard.state.get_unified_state_root();
            let nonce_root = app_guard.state.get_nonce_root();

            // Compute app hash (combination of all state roots)
            let app_hash = {
                let mut hasher = sha3::Keccak256::new();
                sha3::Digest::update(&mut hasher, &unified_root);
                sha3::Digest::update(&mut hasher, &nonce_root);
                let result: [u8; 32] = sha3::Digest::finalize(hasher).into();
                result
            };

            Ok(json!({
                "blockHeight": height,
                "appHash": format!("0x{}", hex::encode(app_hash)),
                "stateRoots": {
                    "unifiedState": format!("0x{}", hex::encode(unified_root)),
                    "nonces": format!("0x{}", hex::encode(nonce_root))
                }
            }))
        }

        InfoRequest::StateProof { user, token } => {
            let address = parse_address(&user)?;

            // Get the proof for this user's balance
            match app_guard.state.prove_balance(address, token) {
                Some(proof) => {
                    // Get the current balance for context
                    let unified_state = app_guard.unified_state();
                    let unified = unified_state.read().unwrap();
                    let balance = unified.get_balance_or_default(address, token);
                    let root = app_guard.state.get_unified_state_root();

                    // Serialize proof to hex for API response
                    let proof_bytes = proof.to_bytes();

                    Ok(json!({
                        "user": user,
                        "token": token,
                        "balance": {
                            "total": balance.total.to_string_trimmed(),
                            "coreView": balance.core_view.to_string_trimmed(),
                            "evmView": balance.evm_view.to_string_trimmed()
                        },
                        "proof": {
                            "leafHash": format!("0x{}", hex::encode(proof.leaf_hash)),
                            "leafIndex": proof.leaf_index,
                            "siblings": proof.siblings.iter()
                                .map(|s| format!("0x{}", hex::encode(s)))
                                .collect::<Vec<_>>(),
                            "directions": proof.directions.iter()
                                .map(|d| match d {
                                    hypercore_chain::merkle::ProofDirection::Left => "left",
                                    hypercore_chain::merkle::ProofDirection::Right => "right",
                                })
                                .collect::<Vec<_>>(),
                            "proofHex": format!("0x{}", hex::encode(&proof_bytes))
                        },
                        "root": format!("0x{}", hex::encode(root)),
                        "verified": proof.verify(&root)
                    }))
                }
                None => {
                    // No proof available - user might not have a balance
                    Ok(json!({
                        "user": user,
                        "token": token,
                        "error": "No balance found for this user/token combination",
                        "proof": null
                    }))
                }
            }
        }

        _ => Err(HandlerError::Internal("Unexpected request type for state proof".to_string())),
    }
}

/// Process an exchange request
///
/// Phase 2B: Perp transactions are now routed through the ABCI app layer
/// for proper transaction processing with state commitment.
async fn process_exchange_request(
    _engine: &Arc<RwLock<EngineState>>,
    spot_engine: &Arc<RwLock<SpotEngine>>,
    app: &Arc<RwLock<HyperCoreApp>>,
    request: ExchangeRequest,
) -> Result<Value, HandlerError> {
    // Verify signature and extract sender address
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

    // Route perp transactions through ABCI app layer
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Execute all perp transactions through ABCI app layer (no fallback)
    execute_perp_action_via_app(app, sender, &request, timestamp).await
}

/// Execute perp action through the ABCI app layer
async fn execute_perp_action_via_app(
    app: &Arc<RwLock<HyperCoreApp>>,
    sender: AccountAddress,
    request: &ExchangeRequest,
    timestamp: u64,
) -> Result<Value, HandlerError> {
    use hypercore_chain::tx::{Transaction, TransactionType, OrderWire, CancelWire, CancelByCloidWire};
    use alloy_primitives::B256;

    // Convert ExchangeAction to TransactionType
    let tx_type = match &request.action {
        ExchangeAction::Order { orders, grouping } => {
            let order_wires: Vec<OrderWire> = orders.iter().map(|o| OrderWire {
                a: o.a,
                b: o.b,
                p: o.p.clone(),
                s: o.s.clone(),
                r: o.r,
                t: convert_order_type_wire(&o.t),
                c: o.c.clone(),
            }).collect();
            // Convert grouping string to OrderGrouping enum
            let order_grouping = match grouping.to_lowercase().as_str() {
                "normalTpsl" | "normaltpsl" => hypercore_chain::tx::OrderGrouping::NormalTpsl,
                "positionTpsl" | "positiontpsl" => hypercore_chain::tx::OrderGrouping::PositionTpsl,
                _ => hypercore_chain::tx::OrderGrouping::Na,
            };
            TransactionType::Order { orders: order_wires, grouping: order_grouping }
        }
        ExchangeAction::Cancel { cancels } => {
            let cancel_wires: Vec<CancelWire> = cancels.iter().map(|c| CancelWire {
                a: c.a,
                o: c.o,
            }).collect();
            TransactionType::Cancel { cancels: cancel_wires }
        }
        ExchangeAction::CancelByCloid { cancels } => {
            let cancel_wires: Vec<CancelByCloidWire> = cancels.iter().map(|c| CancelByCloidWire {
                asset: c.asset,
                cloid: c.cloid.clone(),
            }).collect();
            TransactionType::CancelByCloid { cancels: cancel_wires }
        }
        ExchangeAction::CancelAll => TransactionType::CancelAll,
        ExchangeAction::UsdTransfer { destination, amount } => {
            let dest = parse_address(destination)?;
            TransactionType::UsdTransfer { destination: dest, amount: amount.clone() }
        }
        ExchangeAction::Withdraw { destination, amount } => {
            let dest = parse_address(destination)?;
            TransactionType::Withdraw { destination: dest, amount: amount.clone() }
        }
        ExchangeAction::UpdateLeverage { asset, is_cross, leverage } => {
            TransactionType::UpdateLeverage { asset: *asset, is_cross: *is_cross, leverage: *leverage }
        }
        _ => return Err(HandlerError::Internal("Unsupported action type".to_string())),
    };

    // Create transaction with signature
    let sig_wire = &request.signature;
    let r_bytes = hex::decode(sig_wire.r.strip_prefix("0x").unwrap_or(&sig_wire.r))
        .map_err(|_| HandlerError::InvalidSignature)?;
    let s_bytes = hex::decode(sig_wire.s.strip_prefix("0x").unwrap_or(&sig_wire.s))
        .map_err(|_| HandlerError::InvalidSignature)?;

    let signature = Signature::new(
        B256::from_slice(&r_bytes.get(..32).unwrap_or(&[0u8; 32])),
        B256::from_slice(&s_bytes.get(..32).unwrap_or(&[0u8; 32])),
        sig_wire.v,
    );

    let tx = Transaction {
        action: tx_type,
        nonce: request.nonce,
        signature,
        hash: None,
    };

    // Execute transaction through app
    let mut app_guard = app.write().await;
    match app_guard.execute_tx(&tx, timestamp) {
        Ok(result) => {
            // Convert result to JSON response
            Ok(json!({
                "status": "ok",
                "response": {
                    "type": format_action_type(&request.action),
                    "data": {
                        "success": result.success,
                        "events": result.events.iter().map(|e| json!({
                            "type": e.r#type.clone(),
                            "attributes": e.attributes.clone()
                        })).collect::<Vec<_>>(),
                        "gasUsed": result.gas_used
                    }
                }
            }))
        }
        Err(e) => Err(HandlerError::Internal(format!("Transaction failed: {}", e)))
    }
}

/// Format action type for response
fn format_action_type(action: &ExchangeAction) -> &'static str {
    match action {
        ExchangeAction::Order { .. } => "order",
        ExchangeAction::Cancel { .. } => "cancel",
        ExchangeAction::CancelByCloid { .. } => "cancelByCloid",
        ExchangeAction::CancelAll => "cancelAll",
        ExchangeAction::UsdTransfer { .. } => "usdTransfer",
        ExchangeAction::Withdraw { .. } => "withdraw",
        ExchangeAction::UpdateLeverage { .. } => "updateLeverage",
        _ => "unknown",
    }
}

/// Convert API order type wire to chain order type wire
fn convert_order_type_wire(wire: &crate::api::OrderTypeWire) -> hypercore_chain::tx::OrderTypeWire {
    match wire {
        crate::api::OrderTypeWire::Limit { limit } => {
            hypercore_chain::tx::OrderTypeWire::Limit {
                limit: hypercore_chain::tx::LimitTif { tif: limit.tif.clone() }
            }
        }
        crate::api::OrderTypeWire::Trigger { trigger } => {
            hypercore_chain::tx::OrderTypeWire::Trigger {
                trigger: hypercore_chain::tx::TriggerOrder {
                    is_market: trigger.is_market,
                    trigger_px: trigger.trigger_px.clone(),
                    tpsl: trigger.tpsl.clone(),
                }
            }
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

/// Verify signature and extract sender address using EIP-712
///
/// This implements proper EIP-712 signature verification:
/// 1. Computes the typed data hash using the same encoding as the SDK
/// 2. Recovers the signer address using ecrecover
/// 3. Returns the recovered address
///
/// PRODUCTION MODE: All signatures are cryptographically verified.
/// No bypass mechanisms are available.
fn verify_signature(request: &ExchangeRequest) -> Result<AccountAddress, HandlerError> {
    use alloy_primitives::B256;
    use hypercore_primitives::Signature;

    let sig = &request.signature;

    // Parse signature components
    let r_hex = sig.r.strip_prefix("0x").unwrap_or(&sig.r);
    let s_hex = sig.s.strip_prefix("0x").unwrap_or(&sig.s);

    // Validate hex lengths
    if r_hex.len() != 64 || s_hex.len() != 64 {
        tracing::warn!("Invalid signature format: r={} s={}", r_hex.len(), s_hex.len());
        return Err(HandlerError::InvalidSignature);
    }

    let r_bytes = hex::decode(r_hex).map_err(|_| HandlerError::InvalidSignature)?;
    let s_bytes = hex::decode(s_hex).map_err(|_| HandlerError::InvalidSignature)?;

    let signature = Signature::new(
        B256::from_slice(&r_bytes),
        B256::from_slice(&s_bytes),
        sig.v,
    );

    // Get chain ID from environment or use default
    let chain_id: u64 = std::env::var("CHAIN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(crate::eip712::DEFAULT_CHAIN_ID);

    // Compute EIP-712 typed data hash using proper encoding
    let message_hash = crate::eip712::compute_typed_data_hash(request, chain_id);

    // Recover signer from signature
    match signature.recover(&message_hash) {
        Ok(recovered) => {
            tracing::info!(
                "EIP-712 signature verified: action={:?}, recovered={:?}, hash={:?}",
                std::mem::discriminant(&request.action),
                recovered,
                hex::encode(&message_hash[..8])
            );
            Ok(recovered)
        }
        Err(e) => {
            tracing::warn!("Signature verification failed: {:?}", e);
            Err(HandlerError::InvalidSignature)
        }
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

/// Convert market ID to coin name
fn coin_name_from_market_id(market_id: u8) -> &'static str {
    match market_id {
        0 => "BTC",
        1 => "ETH",
        2 => "SOL",
        3 => "AVAX",
        4 => "MATIC",
        5 => "ARB",
        6 => "OP",
        7 => "DOGE",
        _ => "UNKNOWN",
    }
}

/// Convert coin name to market ID
fn market_id_from_coin(coin: &str) -> u8 {
    match coin.to_uppercase().as_str() {
        "BTC" => 0,
        "ETH" => 1,
        "SOL" => 2,
        "AVAX" => 3,
        "MATIC" => 4,
        "ARB" => 5,
        "OP" => 6,
        "DOGE" => 7,
        _ => 255, // Invalid market
    }
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
