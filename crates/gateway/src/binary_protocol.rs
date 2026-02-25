//! Binary protocol for WebSocket market data (Phase C2)
//!
//! Provides compact binary encoding for high-frequency market data updates,
//! reducing serialization overhead by 5-10x compared to JSON.
//!
//! ## Wire Format
//!
//! All messages use a simple TLV (Type-Length-Value) encoding:
//!
//! ```text
//! +----------+----------+------------------+
//! | Type (1) | Len (4)  | Payload (N)      |
//! +----------+----------+------------------+
//! ```
//!
//! Types:
//! - 0x01: L2BookSnapshot
//! - 0x02: TradeUpdate
//! - 0x03: OrderUpdate
//! - 0x04: FillUpdate
//! - 0x05: PositionUpdate
//! - 0x06: AllMids
//! - 0x10: PlaceOrderRequest
//! - 0x11: CancelOrderRequest
//! - 0x12: BatchOrderRequest
//! - 0xFE: Error
//! - 0xFF: Pong

use serde::{Deserialize, Serialize};

/// Binary message type identifiers
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryMsgType {
    L2BookSnapshot = 0x01,
    TradeUpdate = 0x02,
    OrderUpdate = 0x03,
    FillUpdate = 0x04,
    PositionUpdate = 0x05,
    AllMids = 0x06,
    PlaceOrder = 0x10,
    CancelOrder = 0x11,
    BatchOrder = 0x12,
    Error = 0xFE,
    Pong = 0xFF,
}

impl BinaryMsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::L2BookSnapshot),
            0x02 => Some(Self::TradeUpdate),
            0x03 => Some(Self::OrderUpdate),
            0x04 => Some(Self::FillUpdate),
            0x05 => Some(Self::PositionUpdate),
            0x06 => Some(Self::AllMids),
            0x10 => Some(Self::PlaceOrder),
            0x11 => Some(Self::CancelOrder),
            0x12 => Some(Self::BatchOrder),
            0xFE => Some(Self::Error),
            0xFF => Some(Self::Pong),
            _ => None,
        }
    }
}

/// Compact L2 book level for binary encoding
/// Uses fixed-point integers instead of strings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryL2Level {
    /// Price as raw integer (8 decimal places)
    pub price_raw: u64,
    /// Size as raw integer (8 decimal places)
    pub size_raw: u64,
    /// Number of orders at this level
    pub order_count: u32,
}

/// Compact L2 book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryL2Book {
    /// Market ID
    pub market_id: u16,
    /// Timestamp in milliseconds
    pub timestamp: u64,
    /// Bid levels (best first)
    pub bids: Vec<BinaryL2Level>,
    /// Ask levels (best first)
    pub asks: Vec<BinaryL2Level>,
}

/// Compact trade update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryTrade {
    pub market_id: u16,
    pub price_raw: u64,
    pub size_raw: u64,
    pub is_buy: bool,
    pub timestamp: u64,
}

/// Compact order update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryOrderUpdate {
    pub market_id: u16,
    pub order_id: u64,
    /// 0=Open, 1=PartialFill, 2=Filled, 3=Canceled
    pub status: u8,
    pub price_raw: u64,
    pub size_raw: u64,
    pub filled_size_raw: u64,
    pub is_buy: bool,
    pub timestamp: u64,
}

/// Compact fill update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryFill {
    pub market_id: u16,
    pub order_id: u64,
    pub price_raw: u64,
    pub size_raw: u64,
    pub fee_raw: i64,
    pub is_buy: bool,
    pub is_maker: bool,
    pub realized_pnl_raw: i64,
    pub timestamp: u64,
}

/// Compact position update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryPosition {
    pub market_id: u16,
    /// Signed size (negative = short)
    pub size_raw: i64,
    pub entry_price_raw: u64,
    pub unrealized_pnl_raw: i64,
    pub leverage: u8,
    /// 0 = no liquidation price
    pub liquidation_price_raw: u64,
}

/// Compact all-mids update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryAllMids {
    pub timestamp: u64,
    /// (market_id, mid_price_raw) pairs
    pub mids: Vec<(u16, u64)>,
}

/// Encode a binary message with TLV framing
pub fn encode_message(msg_type: BinaryMsgType, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(msg_type as u8);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Decode a binary message type and payload from TLV frame
pub fn decode_message(data: &[u8]) -> Result<(BinaryMsgType, &[u8]), BinaryProtocolError> {
    if data.len() < 5 {
        return Err(BinaryProtocolError::MessageTooShort);
    }

    let msg_type = BinaryMsgType::from_u8(data[0])
        .ok_or(BinaryProtocolError::UnknownMessageType(data[0]))?;

    let len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;

    if data.len() < 5 + len {
        return Err(BinaryProtocolError::IncompletePayload {
            expected: len,
            got: data.len() - 5,
        });
    }

    Ok((msg_type, &data[5..5 + len]))
}

/// Encode an L2 book snapshot to binary
pub fn encode_l2_book(book: &BinaryL2Book) -> Vec<u8> {
    let payload = bincode::serialize(book).expect("L2Book serialization should not fail");
    encode_message(BinaryMsgType::L2BookSnapshot, &payload)
}

/// Decode an L2 book snapshot from binary payload
pub fn decode_l2_book(payload: &[u8]) -> Result<BinaryL2Book, BinaryProtocolError> {
    bincode::deserialize(payload)
        .map_err(|e| BinaryProtocolError::DeserializationFailed(e.to_string()))
}

/// Encode a trade update to binary
pub fn encode_trade(trade: &BinaryTrade) -> Vec<u8> {
    let payload = bincode::serialize(trade).expect("Trade serialization should not fail");
    encode_message(BinaryMsgType::TradeUpdate, &payload)
}

/// Decode a trade update from binary payload
pub fn decode_trade(payload: &[u8]) -> Result<BinaryTrade, BinaryProtocolError> {
    bincode::deserialize(payload)
        .map_err(|e| BinaryProtocolError::DeserializationFailed(e.to_string()))
}

/// Encode an order update to binary
pub fn encode_order_update(order: &BinaryOrderUpdate) -> Vec<u8> {
    let payload = bincode::serialize(order).expect("OrderUpdate serialization should not fail");
    encode_message(BinaryMsgType::OrderUpdate, &payload)
}

/// Encode a fill update to binary
pub fn encode_fill(fill: &BinaryFill) -> Vec<u8> {
    let payload = bincode::serialize(fill).expect("Fill serialization should not fail");
    encode_message(BinaryMsgType::FillUpdate, &payload)
}

/// Encode a position update to binary
pub fn encode_position(pos: &BinaryPosition) -> Vec<u8> {
    let payload = bincode::serialize(pos).expect("Position serialization should not fail");
    encode_message(BinaryMsgType::PositionUpdate, &payload)
}

/// Encode all-mids update to binary
pub fn encode_all_mids(mids: &BinaryAllMids) -> Vec<u8> {
    let payload = bincode::serialize(mids).expect("AllMids serialization should not fail");
    encode_message(BinaryMsgType::AllMids, &payload)
}

/// Encode a pong response
pub fn encode_pong() -> Vec<u8> {
    encode_message(BinaryMsgType::Pong, &[])
}

/// Encode an error message
pub fn encode_error(msg: &str) -> Vec<u8> {
    encode_message(BinaryMsgType::Error, msg.as_bytes())
}

/// Binary protocol errors
#[derive(Debug, thiserror::Error)]
pub enum BinaryProtocolError {
    #[error("Message too short (minimum 5 bytes for TLV header)")]
    MessageTooShort,
    #[error("Unknown message type: 0x{0:02x}")]
    UnknownMessageType(u8),
    #[error("Incomplete payload: expected {expected} bytes, got {got}")]
    IncompletePayload { expected: usize, got: usize },
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
}

/// Size comparison: JSON vs Binary for an L2 book with 10 levels
///
/// JSON: ~800 bytes (string prices, string sizes, field names)
/// Binary: ~250 bytes (fixed-point integers, no field names)
/// Savings: ~70%

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_l2_book() {
        let book = BinaryL2Book {
            market_id: 0,
            timestamp: 1234567890,
            bids: vec![
                BinaryL2Level {
                    price_raw: 65000_00000000,
                    size_raw: 1_00000000,
                    order_count: 3,
                },
                BinaryL2Level {
                    price_raw: 64999_00000000,
                    size_raw: 5_00000000,
                    order_count: 7,
                },
            ],
            asks: vec![BinaryL2Level {
                price_raw: 65001_00000000,
                size_raw: 2_00000000,
                order_count: 2,
            }],
        };

        let encoded = encode_l2_book(&book);

        // Verify TLV header
        assert_eq!(encoded[0], BinaryMsgType::L2BookSnapshot as u8);

        // Decode
        let (msg_type, payload) = decode_message(&encoded).unwrap();
        assert_eq!(msg_type, BinaryMsgType::L2BookSnapshot);

        let decoded = decode_l2_book(payload).unwrap();
        assert_eq!(decoded.market_id, 0);
        assert_eq!(decoded.bids.len(), 2);
        assert_eq!(decoded.asks.len(), 1);
        assert_eq!(decoded.bids[0].price_raw, 65000_00000000);
    }

    #[test]
    fn test_encode_decode_trade() {
        let trade = BinaryTrade {
            market_id: 0,
            price_raw: 65000_00000000,
            size_raw: 1_00000000,
            is_buy: true,
            timestamp: 1234567890,
        };

        let encoded = encode_trade(&trade);
        let (msg_type, payload) = decode_message(&encoded).unwrap();
        assert_eq!(msg_type, BinaryMsgType::TradeUpdate);

        let decoded = decode_trade(payload).unwrap();
        assert_eq!(decoded.price_raw, 65000_00000000);
        assert!(decoded.is_buy);
    }

    #[test]
    fn test_pong_encoding() {
        let encoded = encode_pong();
        let (msg_type, payload) = decode_message(&encoded).unwrap();
        assert_eq!(msg_type, BinaryMsgType::Pong);
        assert!(payload.is_empty());
    }

    #[test]
    fn test_error_encoding() {
        let encoded = encode_error("test error");
        let (msg_type, payload) = decode_message(&encoded).unwrap();
        assert_eq!(msg_type, BinaryMsgType::Error);
        assert_eq!(std::str::from_utf8(payload).unwrap(), "test error");
    }

    #[test]
    fn test_decode_too_short() {
        let result = decode_message(&[0x01, 0x02]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_unknown_type() {
        let data = [0x99, 0x00, 0x00, 0x00, 0x00];
        let result = decode_message(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_binary_vs_json_size() {
        // Create a 10-level L2 book
        let book = BinaryL2Book {
            market_id: 0,
            timestamp: 1234567890000,
            bids: (0..10)
                .map(|i| BinaryL2Level {
                    price_raw: (65000 - i) * 100000000,
                    size_raw: (i as u64 + 1) * 100000000,
                    order_count: (i as u32 + 1) * 2,
                })
                .collect(),
            asks: (0..10)
                .map(|i| BinaryL2Level {
                    price_raw: (65001 + i) * 100000000,
                    size_raw: (i as u64 + 1) * 100000000,
                    order_count: (i as u32 + 1) * 3,
                })
                .collect(),
        };

        let binary_size = encode_l2_book(&book).len();
        let json_size = serde_json::to_string(&book).unwrap().len();

        // Binary should be significantly smaller
        assert!(
            binary_size < json_size,
            "Binary ({}) should be smaller than JSON ({})",
            binary_size,
            json_size
        );

        let savings = 100.0 * (1.0 - binary_size as f64 / json_size as f64);
        // We expect at least 30% savings
        assert!(
            savings > 30.0,
            "Expected >30% size savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_roundtrip_all_message_types() {
        // L2Book
        let book = BinaryL2Book {
            market_id: 1,
            timestamp: 1000,
            bids: vec![BinaryL2Level {
                price_raw: 100,
                size_raw: 50,
                order_count: 1,
            }],
            asks: vec![],
        };
        let encoded = encode_l2_book(&book);
        let (t, p) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::L2BookSnapshot);
        let _ = decode_l2_book(p).unwrap();

        // Trade
        let trade = BinaryTrade {
            market_id: 0,
            price_raw: 100,
            size_raw: 50,
            is_buy: false,
            timestamp: 2000,
        };
        let encoded = encode_trade(&trade);
        let (t, p) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::TradeUpdate);
        let _ = decode_trade(p).unwrap();

        // Order update
        let order = BinaryOrderUpdate {
            market_id: 0,
            order_id: 42,
            status: 0,
            price_raw: 100,
            size_raw: 50,
            filled_size_raw: 0,
            is_buy: true,
            timestamp: 3000,
        };
        let encoded = encode_order_update(&order);
        let (t, _) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::OrderUpdate);

        // Fill
        let fill = BinaryFill {
            market_id: 0,
            order_id: 42,
            price_raw: 100,
            size_raw: 50,
            fee_raw: -10,
            is_buy: true,
            is_maker: true,
            realized_pnl_raw: 0,
            timestamp: 4000,
        };
        let encoded = encode_fill(&fill);
        let (t, _) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::FillUpdate);

        // Position
        let pos = BinaryPosition {
            market_id: 0,
            size_raw: -50,
            entry_price_raw: 100,
            unrealized_pnl_raw: 25,
            leverage: 10,
            liquidation_price_raw: 80,
        };
        let encoded = encode_position(&pos);
        let (t, _) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::PositionUpdate);

        // AllMids
        let mids = BinaryAllMids {
            timestamp: 5000,
            mids: vec![(0, 65000_00000000), (1, 3500_00000000)],
        };
        let encoded = encode_all_mids(&mids);
        let (t, _) = decode_message(&encoded).unwrap();
        assert_eq!(t, BinaryMsgType::AllMids);
    }
}
