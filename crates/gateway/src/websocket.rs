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
use hypercore_primitives::{AccountAddress, MarketId};
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
    /// Ping
    Ping,
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
    let (mut sender, mut receiver) = socket.split();

    // Track subscriptions
    let mut subscriptions: HashSet<String> = HashSet::new();
    let mut broadcast_receivers: Vec<broadcast::Receiver<WsMessage>> = Vec::new();

    // Spawn task to forward broadcast messages to client
    let (client_tx, mut client_rx) = mpsc::channel::<WsMessage>(100);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if sender.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(data)) => {
                // Respond to ping with pong
                let _ = client_tx.send(WsMessage::Pong).await;
                continue;
            }
            _ => continue,
        };

        // Parse request
        let request: WsRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                let _ = client_tx
                    .send(WsMessage::Error {
                        message: format!("Invalid request: {}", e),
                    })
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
                tokio::spawn(async move {
                    let mut rx = rx;
                    while let Ok(msg) = rx.recv().await {
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                });

                subscriptions.insert(sub_key.clone());

                let _ = client_tx
                    .send(WsMessage::Subscribed { sub_type: sub_key })
                    .await;
            }

            WsRequest::Unsubscribe { subscription } => {
                let sub_key = subscription_key(&subscription);
                subscriptions.remove(&sub_key);
                // Note: We don't actually stop the broadcast receiver here
                // In production, we'd need more sophisticated tracking
            }

            WsRequest::Ping => {
                let _ = client_tx.send(WsMessage::Pong).await;
            }
        }
    }

    // Clean up
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
}
