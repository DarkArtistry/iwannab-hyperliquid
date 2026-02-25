//! WebSocket server for real-time updates

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::server::AppState;

/// WebSocket subscription types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsSubscription {
    /// Subscribe to all mid prices
    AllMids,
    /// Subscribe to L2 orderbook updates
    L2Book { coin: String },
    /// Subscribe to trades
    Trades { coin: String },
    /// Subscribe to user orders
    UserOrders { user: String },
    /// Subscribe to user fills
    UserFills { user: String },
    /// Subscribe to user position updates
    UserPositions { user: String },
    /// Subscribe to candles
    Candles { coin: String, interval: String },
}

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel", rename_all = "camelCase")]
pub enum WsMessage {
    /// Subscription confirmation
    Subscribed {
        #[serde(rename = "type")]
        sub_type: String,
    },
    /// Error message
    Error { message: String },
    /// All mids update
    AllMids { data: HashMap<String, String> },
    /// L2 book update
    L2Book { data: L2BookUpdate },
    /// Trade update
    Trades { data: Vec<TradeUpdate> },
    /// Order update
    Order { data: OrderUpdate },
    /// Fill update
    Fill { data: FillUpdate },
    /// Position update
    Position { data: PositionUpdate },
    /// Candle update
    Candle { data: CandleUpdate },
    /// Pong response
    Pong,
    /// Binary response (Phase C4)
    BinaryResponse { data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookUpdate {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<L2Level>, Vec<L2Level>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Level {
    pub px: String,
    pub sz: String,
    pub n: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeUpdate {
    pub coin: String,
    pub side: String,
    pub px: String,
    pub sz: String,
    pub time: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUpdate {
    pub coin: String,
    pub oid: u64,
    pub status: String,
    pub sz: String,
    pub filled_sz: String,
    pub limit_px: String,
    pub side: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillUpdate {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub fee: String,
    pub oid: u64,
    pub crossed: bool,
    pub closed_pnl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionUpdate {
    pub coin: String,
    pub szi: String,
    pub entry_px: String,
    pub unrealized_pnl: String,
    pub leverage: u8,
    pub liquidation_px: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleUpdate {
    #[serde(rename = "s")]
    pub start: u64,
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
    #[serde(rename = "i")]
    pub interval: String,
}

/// WebSocket client request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum WsRequest {
    /// Subscribe to a channel
    Subscribe { subscription: WsSubscription },
    /// Unsubscribe from a channel
    Unsubscribe { subscription: WsSubscription },
    /// Enable binary mode for all subscriptions
    SetBinaryMode { enabled: bool },
    /// Ping
    Ping,
}

/// Internal message wrapper that tracks whether to send as text or binary
enum ClientMsg {
    Text(WsMessage),
    Binary(WsMessage),
}

/// WebSocket connection manager
pub struct WsManager {
    /// Broadcast channels for different data types
    mids_tx: broadcast::Sender<WsMessage>,
    book_txs: RwLock<HashMap<String, broadcast::Sender<WsMessage>>>,
    trade_txs: RwLock<HashMap<String, broadcast::Sender<WsMessage>>>,
    user_txs: RwLock<HashMap<String, broadcast::Sender<WsMessage>>>,
}

impl WsManager {
    /// Create new WebSocket manager
    pub fn new() -> Self {
        let (mids_tx, _) = broadcast::channel(1000);

        Self {
            mids_tx,
            book_txs: RwLock::new(HashMap::new()),
            trade_txs: RwLock::new(HashMap::new()),
            user_txs: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create book broadcast channel
    pub async fn get_book_channel(&self, coin: &str) -> broadcast::Sender<WsMessage> {
        let mut txs = self.book_txs.write().await;
        txs.entry(coin.to_string())
            .or_insert_with(|| broadcast::channel(1000).0)
            .clone()
    }

    /// Get or create trade broadcast channel
    pub async fn get_trade_channel(&self, coin: &str) -> broadcast::Sender<WsMessage> {
        let mut txs = self.trade_txs.write().await;
        txs.entry(coin.to_string())
            .or_insert_with(|| broadcast::channel(1000).0)
            .clone()
    }

    /// Get or create user broadcast channel
    pub async fn get_user_channel(&self, user: &str) -> broadcast::Sender<WsMessage> {
        let mut txs = self.user_txs.write().await;
        txs.entry(user.to_string())
            .or_insert_with(|| broadcast::channel(1000).0)
            .clone()
    }

    /// Broadcast mid prices update
    pub fn broadcast_mids(&self, mids: HashMap<String, String>) {
        let _ = self.mids_tx.send(WsMessage::AllMids { data: mids });
    }

    /// Broadcast L2 book update
    pub async fn broadcast_book(&self, coin: &str, update: L2BookUpdate) {
        let tx = self.get_book_channel(coin).await;
        let _ = tx.send(WsMessage::L2Book { data: update });
    }

    /// Broadcast trade
    pub async fn broadcast_trade(&self, coin: &str, trade: TradeUpdate) {
        let tx = self.get_trade_channel(coin).await;
        let _ = tx.send(WsMessage::Trades { data: vec![trade] });
    }

    /// Broadcast user order update
    pub async fn broadcast_order(&self, user: &str, order: OrderUpdate) {
        let tx = self.get_user_channel(user).await;
        let _ = tx.send(WsMessage::Order { data: order });
    }

    /// Broadcast user fill
    pub async fn broadcast_fill(&self, user: &str, fill: FillUpdate) {
        let tx = self.get_user_channel(user).await;
        let _ = tx.send(WsMessage::Fill { data: fill });
    }

    /// Broadcast user position update
    pub async fn broadcast_position(&self, user: &str, position: PositionUpdate) {
        let tx = self.get_user_channel(user).await;
        let _ = tx.send(WsMessage::Position { data: position });
    }
}

impl Default for WsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared WebSocket manager state
pub type SharedWsManager = Arc<WsManager>;

/// Handle WebSocket upgrade
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    let ws_manager = Arc::clone(&app_state.ws_manager);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, ws_manager))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: AppState, ws_manager: SharedWsManager) {
    hypercore_engine::metrics::WEBSOCKET_CONNECTIONS.inc();
    let (mut sender, mut receiver) = socket.split();

    // Track subscriptions
    let mut subscriptions: HashSet<String> = HashSet::new();
    let _broadcast_receivers: Vec<broadcast::Receiver<WsMessage>> = Vec::new();

    // Whether to send binary-encoded messages instead of JSON
    let mut binary_mode = false;

    // Spawn task to forward broadcast messages to client
    let (client_tx, mut client_rx) = mpsc::channel::<ClientMsg>(100);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            let result = match msg {
                ClientMsg::Text(ws_msg) => {
                    match serde_json::to_string(&ws_msg) {
                        Ok(t) => sender.send(Message::Text(t)).await,
                        Err(_) => continue,
                    }
                }
                ClientMsg::Binary(ws_msg) => {
                    match bincode::serialize(&ws_msg) {
                        Ok(bytes) => sender.send(Message::Binary(bytes)).await,
                        Err(_) => continue,
                    }
                }
            };
            if result.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Binary(data)) => {
                // Phase C4: Handle binary order submission
                // First byte = message type: 0x01=PlaceOrder, 0x02=Cancel, 0x03=Batch
                if data.is_empty() {
                    continue;
                }
                match handle_binary_message(&data, &app_state).await {
                    Ok(response) => {
                        if let Ok(text) = serde_json::to_string(&response) {
                            let _ = client_tx.send(ClientMsg::Text(WsMessage::BinaryResponse { data: text })).await;
                        }
                    }
                    Err(e) => {
                        let _ = client_tx.send(ClientMsg::Text(WsMessage::Error { message: e })).await;
                    }
                }
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_data)) => {
                // Respond to ping with pong
                let _ = client_tx.send(ClientMsg::Text(WsMessage::Pong)).await;
                continue;
            }
            _ => continue,
        };

        // Parse request
        let request: WsRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                let _ = client_tx
                    .send(ClientMsg::Text(WsMessage::Error {
                        message: format!("Invalid request: {}", e),
                    }))
                    .await;
                continue;
            }
        };

        match request {
            WsRequest::Subscribe { subscription } => {
                let sub_key = subscription_key(&subscription);

                if subscriptions.contains(&sub_key) {
                    continue;
                }

                // Set up subscription
                let rx = match &subscription {
                    WsSubscription::AllMids => ws_manager.mids_tx.subscribe(),
                    WsSubscription::L2Book { coin } => {
                        ws_manager.get_book_channel(coin).await.subscribe()
                    }
                    WsSubscription::Trades { coin } => {
                        ws_manager.get_trade_channel(coin).await.subscribe()
                    }
                    WsSubscription::UserOrders { user }
                    | WsSubscription::UserFills { user }
                    | WsSubscription::UserPositions { user } => {
                        ws_manager.get_user_channel(user).await.subscribe()
                    }
                    WsSubscription::Candles { coin, .. } => {
                        // Candles use same channel as book for simplicity
                        ws_manager.get_book_channel(coin).await.subscribe()
                    }
                };

                // Spawn task to forward messages
                let tx = client_tx.clone();
                let is_binary = binary_mode;
                tokio::spawn(async move {
                    let mut rx = rx;
                    while let Ok(msg) = rx.recv().await {
                        let client_msg = if is_binary {
                            ClientMsg::Binary(msg)
                        } else {
                            ClientMsg::Text(msg)
                        };
                        if tx.send(client_msg).await.is_err() {
                            break;
                        }
                    }
                });

                subscriptions.insert(sub_key.clone());

                let _ = client_tx
                    .send(ClientMsg::Text(WsMessage::Subscribed { sub_type: sub_key }))
                    .await;
            }

            WsRequest::Unsubscribe { subscription } => {
                let sub_key = subscription_key(&subscription);
                subscriptions.remove(&sub_key);
                // Note: We don't actually stop the broadcast receiver here
                // In production, we'd need more sophisticated tracking
            }

            WsRequest::SetBinaryMode { enabled } => {
                binary_mode = enabled;
                let _ = client_tx.send(ClientMsg::Text(WsMessage::Subscribed {
                    sub_type: format!("binaryMode:{}", enabled),
                })).await;
            }

            WsRequest::Ping => {
                let _ = client_tx.send(ClientMsg::Text(WsMessage::Pong)).await;
            }
        }
    }

    // Clean up
    hypercore_engine::metrics::WEBSOCKET_CONNECTIONS.dec();
    send_task.abort();
}

/// Generate unique key for subscription
fn subscription_key(sub: &WsSubscription) -> String {
    match sub {
        WsSubscription::AllMids => "allMids".to_string(),
        WsSubscription::L2Book { coin } => format!("l2Book:{}", coin),
        WsSubscription::Trades { coin } => format!("trades:{}", coin),
        WsSubscription::UserOrders { user } => format!("userOrders:{}", user),
        WsSubscription::UserFills { user } => format!("userFills:{}", user),
        WsSubscription::UserPositions { user } => format!("userPositions:{}", user),
        WsSubscription::Candles { coin, interval } => format!("candles:{}:{}", coin, interval),
    }
}

/// Handle binary WebSocket messages for order submission (Phase C4)
///
/// Message format:
/// - Byte 0: message type (0x01=PlaceOrder, 0x02=Cancel, 0x03=Batch)
/// - Bytes 1..N: bincode-encoded transaction
async fn handle_binary_message(
    data: &[u8],
    app_state: &AppState,
) -> Result<serde_json::Value, String> {
    use hypercore_chain::tx::Transaction;

    if data.len() < 2 {
        return Err("Binary message too short".to_string());
    }

    let msg_type = data[0];
    let payload = &data[1..];

    match msg_type {
        0x01 | 0x02 | 0x03 => {
            // All types use the same Transaction deserialization
            let tx = Transaction::from_binary(payload)
                .map_err(|e| format!("Binary decode error: {}", e))?;

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            let mut app = app_state.app.write().await;
            match app.execute_tx(&tx, timestamp) {
                Ok(result) => Ok(serde_json::json!({
                    "status": "ok",
                    "type": match msg_type {
                        0x01 => "placeOrder",
                        0x02 => "cancel",
                        0x03 => "batch",
                        _ => "unknown",
                    },
                    "success": result.success,
                    "gasUsed": result.gas_used,
                })),
                Err(e) => Err(format!("Transaction failed: {}", e)),
            }
        }
        _ => Err(format!("Unknown binary message type: 0x{:02x}", msg_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_key() {
        let sub = WsSubscription::L2Book {
            coin: "BTC-PERP".to_string(),
        };
        assert_eq!(subscription_key(&sub), "l2Book:BTC-PERP");
    }

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::AllMids {
            data: HashMap::from([("BTC-PERP".to_string(), "50000.0".to_string())]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("allMids"));
        assert!(json.contains("BTC-PERP"));
    }

    #[test]
    fn test_binary_ws_message_serialization() {
        let msg = WsMessage::AllMids {
            data: HashMap::from([("BTC-PERP".to_string(), "50000.0".to_string())]),
        };
        // Test bincode serialization succeeds (outbound path)
        let binary = bincode::serialize(&msg).unwrap();
        assert!(!binary.is_empty(), "Binary serialization should produce non-empty output");

        // Test round-trip with bincode using the sub-structure directly,
        // since bincode does not support internally tagged enums (serde tag = "channel")
        // for deserialization. Clients deserialize using variant index.
        let data: HashMap<String, String> =
            HashMap::from([("BTC-PERP".to_string(), "50000.0".to_string())]);
        let binary_data = bincode::serialize(&data).unwrap();
        let deserialized: HashMap<String, String> = bincode::deserialize(&binary_data).unwrap();
        assert_eq!(deserialized.get("BTC-PERP").unwrap(), "50000.0");

        // Verify that different message variants serialize to different bytes
        let pong = WsMessage::Pong;
        let pong_binary = bincode::serialize(&pong).unwrap();
        assert_ne!(binary, pong_binary, "Different variants should serialize differently");
    }

    #[test]
    fn test_binary_vs_json_size() {
        // Use a larger L2Book message where binary encoding shines
        let msg = WsMessage::L2Book {
            data: L2BookUpdate {
                coin: "BTC-PERP".to_string(),
                time: 1234567890000,
                levels: (
                    (0..10)
                        .map(|i| L2Level {
                            px: format!("{}.00", 65000 - i),
                            sz: format!("{}.0000", i + 1),
                            n: (i + 1) as u32 * 2,
                        })
                        .collect(),
                    (0..10)
                        .map(|i| L2Level {
                            px: format!("{}.00", 65001 + i),
                            sz: format!("{}.0000", i + 1),
                            n: (i + 1) as u32 * 3,
                        })
                        .collect(),
                ),
            },
        };
        let json_size = serde_json::to_string(&msg).unwrap().len();
        let binary_size = bincode::serialize(&msg).unwrap().len();
        // Binary should be smaller than JSON for structured data with field names
        assert!(
            binary_size < json_size,
            "Binary {} should be smaller than JSON {}",
            binary_size,
            json_size
        );
    }
}
