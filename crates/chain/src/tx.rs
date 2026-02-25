//! Transaction types for HyperCore chain
//!
//! Note: Core EIP-712 encoding functions are imported from hypercore_primitives::eip712
//! to ensure consistency with the gateway crate's implementation.

use hypercore_primitives::{
    AccountAddress, Decimal, MarginMode, MarketId, OrderId, OrderRequest, OrderSide, OrderType,
    Signature, TimeInForce, TriggerDirection,
    eip712::{
        compute_domain_separator, encode_array, encode_bool, encode_string,
        encode_uint64, encode_uint8, encode_address_bytes, DEFAULT_CHAIN_ID,
    },
};
use serde::{Deserialize, Serialize};

/// Unique transaction identifier
pub type TxHash = [u8; 32];

/// Transaction envelope containing action and signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction action
    pub action: TransactionType,
    /// Nonce for replay protection
    pub nonce: u64,
    /// EIP-712 signature
    pub signature: Signature,
    /// Computed transaction hash (cached)
    #[serde(skip)]
    pub hash: Option<TxHash>,
}

impl Transaction {
    /// Compute transaction hash
    pub fn compute_hash(&self) -> TxHash {
        use sha3::{Digest, Keccak256};
        let encoded = serde_json::to_vec(&self.action).unwrap_or_default();
        let mut hasher = Keccak256::new();
        hasher.update(&encoded);
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }

    /// Get or compute transaction hash
    pub fn hash(&mut self) -> TxHash {
        if let Some(h) = self.hash {
            h
        } else {
            let h = self.compute_hash();
            self.hash = Some(h);
            h
        }
    }

    /// Serialize transaction to compact binary format using bincode (Phase C3)
    pub fn to_binary(&self) -> Result<Vec<u8>, TransactionError> {
        bincode::serialize(self).map_err(|e| TransactionError::SerializationError(e.to_string()))
    }

    /// Deserialize transaction from compact binary format (Phase C3)
    pub fn from_binary(bytes: &[u8]) -> Result<Self, TransactionError> {
        bincode::deserialize(bytes).map_err(|e| TransactionError::SerializationError(e.to_string()))
    }

    /// Recover signer address from signature
    pub fn recover_signer(&self) -> Result<AccountAddress, TransactionError> {
        // EIP-712 typed data hash recovery
        let typed_hash = self.compute_eip712_hash()?;
        recover_address(&typed_hash, &self.signature)
    }

    /// Get sender address using EIP-712 signature recovery
    ///
    /// Cryptographically verifies the signature and recovers the sender address.
    pub fn sender(&self) -> Result<AccountAddress, TransactionError> {
        self.recover_signer()
    }

    /// Compute EIP-712 typed data hash for signing
    fn compute_eip712_hash(&self) -> Result<[u8; 32], TransactionError> {
        use sha3::{Digest, Keccak256};

        // Domain separator (use default chain ID for now, will be configurable)
        let domain_separator = compute_domain_separator(DEFAULT_CHAIN_ID);

        // Struct hash based on action type
        let struct_hash = self.action.compute_struct_hash(self.nonce)?;

        // Final hash: keccak256("\x19\x01" || domainSeparator || structHash)
        let mut hasher = Keccak256::new();
        hasher.update(b"\x19\x01");
        hasher.update(domain_separator);
        hasher.update(struct_hash);
        Ok(hasher.finalize().into())
    }
}

/// Transaction types supported by HyperCore
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TransactionType {
    /// Place orders
    Order {
        orders: Vec<OrderWire>,
        grouping: OrderGrouping,
    },
    /// Cancel orders by ID
    Cancel { cancels: Vec<CancelWire> },
    /// Cancel orders by client order ID
    CancelByCloid { cancels: Vec<CancelByCloidWire> },
    /// Cancel all orders
    CancelAll,
    /// Update leverage for a market
    UpdateLeverage {
        asset: MarketId,
        is_cross: bool,
        leverage: u8,
    },
    /// Transfer USDC to another address
    UsdTransfer { destination: AccountAddress, amount: String },
    /// Withdraw USDC to L1
    Withdraw { destination: AccountAddress, amount: String },
    /// EVM transaction (from CoreWriter)
    EvmAction {
        action_type: u8,
        data: Vec<u8>,
    },
    /// Place spot orders (uses SpotOrder EIP-712 type, without reduce_only field)
    SpotOrder {
        orders: Vec<OrderWire>,
        grouping: OrderGrouping,
    },
    /// Cancel all orders in a specific spot market (or all spot markets if None)
    SpotCancelAll {
        market: Option<u8>,
    },
    /// View transfer between Core and EVM views
    ViewTransfer {
        token: u8,
        amount: String,
        to_evm: bool,
    },
    /// Set margin mode (isolated or cross)
    SetMarginMode {
        mode: MarginMode,
    },
    /// Cancel a trigger order (TP/SL) by order ID
    CancelTrigger {
        asset: MarketId,
        order_id: OrderId,
    },
}

// ============================================================================
// Address Encoding Helper
// ============================================================================

/// Encode an AccountAddress as 32-byte (left-padded with zeros)
/// This wraps the shared encode_address_bytes for AccountAddress type
fn encode_address(addr: &AccountAddress) -> [u8; 32] {
    let addr_bytes: [u8; 20] = addr.as_slice().try_into().unwrap_or([0u8; 20]);
    encode_address_bytes(&addr_bytes)
}

impl TransactionType {
    /// Compute EIP-712 struct hash for this action
    ///
    /// IMPORTANT: This must produce identical hashes to gateway/src/eip712.rs
    /// All values must be properly encoded according to EIP-712:
    /// - Addresses: 32 bytes, left-padded with zeros
    /// - uint8/uint64: 32 bytes, left-padded with zeros
    /// - Strings: keccak256 of string bytes
    /// - Bools: 32 bytes, 0 or 1 in last byte
    pub fn compute_struct_hash(&self, nonce: u64) -> Result<[u8; 32], TransactionError> {
        use sha3::{Digest, Keccak256};

        match self {
            TransactionType::Order { orders, grouping } => {
                // Hash each order (properly padded)
                let mut order_hashes = Vec::with_capacity(orders.len());
                for order in orders {
                    order_hashes.push(order.compute_hash_eip712());
                }

                // Hash the orders array
                let orders_hash = encode_array(&order_hashes);

                // Type hash
                let type_hash = Keccak256::digest(
                    b"Action(string type,Order[] orders,string grouping,uint64 nonce)Order(uint8 a,bool b,string p,string s,bool r,string t)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("order"));
                hasher.update(orders_hash);
                hasher.update(encode_string(grouping.as_str()));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::Cancel { cancels } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,Cancel[] cancels,uint64 nonce)Cancel(uint8 a,uint64 o)"
                );

                // Hash each cancel (properly padded)
                let cancel_hashes: Vec<[u8; 32]> = cancels.iter()
                    .map(|c| c.compute_hash_eip712())
                    .collect();
                let cancels_hash = encode_array(&cancel_hashes);

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("cancel"));
                hasher.update(cancels_hash);
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::CancelByCloid { cancels } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,CancelByCloid[] cancels,uint64 nonce)CancelByCloid(uint8 asset,string cloid)"
                );

                // Hash each cancel by cloid (properly padded)
                let cancel_hashes: Vec<[u8; 32]> = cancels.iter()
                    .map(|c| c.compute_hash_eip712())
                    .collect();
                let cancels_hash = encode_array(&cancel_hashes);

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("cancelByCloid"));
                hasher.update(cancels_hash);
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::CancelAll => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint64 nonce)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("cancelAll"));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::UpdateLeverage { asset, is_cross, leverage } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint8 asset,bool isCross,uint8 leverage,uint64 nonce)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("updateLeverage"));
                hasher.update(encode_uint8(*asset));
                hasher.update(encode_bool(*is_cross));
                hasher.update(encode_uint8(*leverage));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::UsdTransfer { destination, amount } |
            TransactionType::Withdraw { destination, amount } => {
                let type_name = match self {
                    TransactionType::UsdTransfer { .. } => "usdTransfer",
                    TransactionType::Withdraw { .. } => "withdraw",
                    _ => unreachable!(),
                };

                let type_hash = Keccak256::digest(
                    b"Action(string type,address destination,string amount,uint64 nonce)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string(type_name));
                hasher.update(encode_address(destination));  // Properly padded to 32 bytes
                hasher.update(encode_string(amount));
                hasher.update(encode_uint64(nonce));         // Properly padded to 32 bytes
                Ok(hasher.finalize().into())
            }
            TransactionType::EvmAction { action_type, data } => {
                // EVM actions use a simple hash for now
                let mut hasher = Keccak256::new();
                hasher.update(encode_uint8(*action_type));
                hasher.update(data);
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::SpotOrder { orders, grouping } => {
                // SpotOrder EIP-712 uses different type than perp Order:
                // - Type string: "spotOrder" (not "order")
                // - SpotOrder struct: no 'r' (reduce_only) field
                let mut order_hashes = Vec::with_capacity(orders.len());
                for order in orders {
                    order_hashes.push(order.compute_hash_eip712_spot());
                }

                let orders_hash = encode_array(&order_hashes);

                let type_hash = Keccak256::digest(
                    b"Action(string type,SpotOrder[] orders,string grouping,uint64 nonce)SpotOrder(uint8 a,bool b,string p,string s,string t)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("spotOrder"));
                hasher.update(orders_hash);
                hasher.update(encode_string(grouping.as_str()));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::SpotCancelAll { market: _ } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint64 nonce)"
                );
                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("spotCancelAll"));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::ViewTransfer { token, amount, to_evm } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint8 token,string amount,bool toEvm,uint64 nonce)"
                );
                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("viewTransfer"));
                hasher.update(encode_uint8(*token));
                hasher.update(encode_string(amount));
                hasher.update(encode_bool(*to_evm));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::SetMarginMode { mode } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,bool isCross,uint64 nonce)"
                );
                let is_cross = matches!(mode, MarginMode::Cross);
                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("setMarginMode"));
                hasher.update(encode_bool(is_cross));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
            TransactionType::CancelTrigger { asset, order_id } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint8 asset,uint64 orderId,uint64 nonce)"
                );
                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(encode_string("cancelTrigger"));
                hasher.update(encode_uint8(*asset));
                hasher.update(encode_uint64(*order_id));
                hasher.update(encode_uint64(nonce));
                Ok(hasher.finalize().into())
            }
        }
    }
}

/// Wire format for order in transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderWire {
    /// Asset/market ID
    pub a: MarketId,
    /// Is buy
    pub b: bool,
    /// Price (string for precision)
    pub p: String,
    /// Size (string for precision)
    pub s: String,
    /// Reduce only
    pub r: bool,
    /// Time-in-force / order type
    pub t: OrderTypeWire,
    /// Client order ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

impl OrderWire {
    /// Compute properly padded EIP-712 struct hash
    pub fn compute_hash_eip712(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(
            b"Order(uint8 a,bool b,string p,string s,bool r,string t)"
        );
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update(encode_uint8(self.a));
        hasher.update(encode_bool(self.b));
        hasher.update(encode_string(&self.p));
        hasher.update(encode_string(&self.s));
        hasher.update(encode_bool(self.r));
        hasher.update(encode_string(&self.t.to_tif_string()));
        hasher.finalize().into()
    }

    /// Compute EIP-712 struct hash for SpotOrder (no reduce_only field)
    ///
    /// SpotOrder uses a different type definition than perp Order:
    /// `SpotOrder(uint8 a,bool b,string p,string s,string t)` — no `bool r`
    pub fn compute_hash_eip712_spot(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(
            b"SpotOrder(uint8 a,bool b,string p,string s,string t)"
        );
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update(encode_uint8(self.a));
        hasher.update(encode_bool(self.b));
        hasher.update(encode_string(&self.p));
        hasher.update(encode_string(&self.s));
        hasher.update(encode_string(&self.t.to_tif_string()));
        hasher.finalize().into()
    }

    /// Legacy hash method (deprecated, use compute_hash_eip712)
    #[deprecated(note = "Use compute_hash_eip712 for proper EIP-712 encoding")]
    pub fn compute_hash(&self) -> [u8; 32] {
        self.compute_hash_eip712()
    }

    /// Convert to internal order request
    pub fn to_order_request(&self, _owner: AccountAddress) -> Result<OrderRequest, TransactionError> {
        let price = Decimal::from_str(&self.p)
            .map_err(|_| TransactionError::InvalidPrice)?;
        let size = Decimal::from_str(&self.s)
            .map_err(|_| TransactionError::InvalidSize)?;
        let order_type = self.t.to_order_type()?;

        // Extract trigger fields from OrderTypeWire::Trigger variant
        let (trigger_price, trigger_direction) = match &self.t {
            OrderTypeWire::Trigger { trigger } => {
                let tp = Decimal::from_str(&trigger.trigger_px)
                    .map_err(|_| TransactionError::InvalidPrice)?;
                let dir = match trigger.tpsl.as_str() {
                    "tp" => TriggerDirection::Above,
                    "sl" => TriggerDirection::Below,
                    _ => {
                        // Infer direction from side: buy triggers below, sell triggers above
                        if self.b { TriggerDirection::Below } else { TriggerDirection::Above }
                    }
                };
                (Some(tp), Some(dir))
            }
            _ => (None, None),
        };

        Ok(OrderRequest {
            market_id: self.a,
            side: if self.b { OrderSide::Buy } else { OrderSide::Sell },
            price,
            size,
            order_type,
            reduce_only: self.r,
            client_order_id: self.c.clone(),
            trigger_price,
            trigger_direction,
        })
    }
}

/// Wire format for order type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OrderTypeWire {
    Limit { limit: LimitTif },
    Trigger { trigger: TriggerOrder },
}

impl OrderTypeWire {
    /// Convert to TIF string for EIP-712 hashing (lowercase)
    /// This must match the TypeScript SDK's transformation
    pub fn to_tif_string(&self) -> String {
        match self {
            OrderTypeWire::Limit { limit } => limit.tif.to_lowercase(),
            OrderTypeWire::Trigger { .. } => "trigger".to_string(),
        }
    }

    /// Legacy method for serialization display
    pub fn to_string(&self) -> String {
        match self {
            OrderTypeWire::Limit { limit } => format!("limit:{}", limit.tif),
            OrderTypeWire::Trigger { trigger } => {
                format!("trigger:{}:{}", trigger.trigger_px, trigger.tpsl)
            }
        }
    }

    pub fn to_order_type(&self) -> Result<OrderType, TransactionError> {
        match self {
            OrderTypeWire::Limit { limit } => {
                let tif = match limit.tif.as_str() {
                    "Gtc" => TimeInForce::Gtc,
                    "Ioc" => TimeInForce::Ioc,
                    "Fok" => TimeInForce::Fok,
                    "Alo" => TimeInForce::Alo,
                    _ => return Err(TransactionError::InvalidTimeInForce),
                };
                Ok(OrderType::Limit { tif })
            }
            OrderTypeWire::Trigger { trigger } => {
                if trigger.is_market {
                    Ok(OrderType::Market)
                } else {
                    // Stop limit orders treated as limit GTC for now
                    Ok(OrderType::Limit { tif: TimeInForce::Gtc })
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitTif {
    pub tif: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerOrder {
    pub is_market: bool,
    pub trigger_px: String,
    pub tpsl: String,
}

/// Order grouping for batch orders
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderGrouping {
    /// No grouping
    Na,
    /// All or none
    NormalTpsl,
    /// Position TP/SL
    PositionTpsl,
}

impl OrderGrouping {
    pub fn as_str(&self) -> &str {
        match self {
            OrderGrouping::Na => "na",
            OrderGrouping::NormalTpsl => "normalTpsl",
            OrderGrouping::PositionTpsl => "positionTpsl",
        }
    }
}

/// Wire format for cancel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelWire {
    /// Asset/market ID
    pub a: MarketId,
    /// Order ID
    pub o: OrderId,
}

impl CancelWire {
    /// Compute properly padded EIP-712 struct hash
    pub fn compute_hash_eip712(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(b"Cancel(uint8 a,uint64 o)");
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update(encode_uint8(self.a));
        hasher.update(encode_uint64(self.o));
        hasher.finalize().into()
    }

    /// Legacy hash method (deprecated)
    #[deprecated(note = "Use compute_hash_eip712 for proper EIP-712 encoding")]
    pub fn compute_hash(&self) -> [u8; 32] {
        self.compute_hash_eip712()
    }
}

/// Wire format for cancel by client order ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelByCloidWire {
    pub asset: MarketId,
    pub cloid: String,
}

impl CancelByCloidWire {
    /// Compute properly padded EIP-712 struct hash
    pub fn compute_hash_eip712(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(b"CancelByCloid(uint8 asset,string cloid)");
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update(encode_uint8(self.asset));
        hasher.update(encode_string(&self.cloid));
        hasher.finalize().into()
    }
}

/// Transaction errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum TransactionError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("Invalid price")]
    InvalidPrice,
    #[error("Invalid size")]
    InvalidSize,
    #[error("Invalid time in force")]
    InvalidTimeInForce,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Unknown market: {0}")]
    UnknownMarket(MarketId),
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Signature recovery failed")]
    SignatureRecoveryFailed,
}

// DEFAULT_CHAIN_ID and compute_domain_separator are now imported from hypercore_primitives::eip712

/// Recover address from message hash and signature
fn recover_address(message_hash: &[u8; 32], signature: &Signature) -> Result<AccountAddress, TransactionError> {
    use k256::ecdsa::{RecoveryId, Signature as K256Signature, VerifyingKey};

    // r and s are already B256 (32 bytes)
    let r_bytes: [u8; 32] = signature.r.into();
    let s_bytes: [u8; 32] = signature.s.into();

    // Create signature bytes
    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(&r_bytes);
    sig_bytes[32..].copy_from_slice(&s_bytes);

    let sig = K256Signature::from_slice(&sig_bytes)
        .map_err(|_| TransactionError::InvalidSignature)?;

    // Recovery ID from v
    let recovery_id = RecoveryId::try_from((signature.v - 27) as u8)
        .map_err(|_| TransactionError::InvalidSignature)?;

    // Recover public key
    let verifying_key = VerifyingKey::recover_from_prehash(message_hash, &sig, recovery_id)
        .map_err(|_| TransactionError::SignatureRecoveryFailed)?;

    // Compute address from public key
    let public_key = verifying_key.to_encoded_point(false);
    let public_key_bytes = &public_key.as_bytes()[1..]; // Skip 0x04 prefix

    use sha3::{Digest, Keccak256};
    let hash = Keccak256::digest(public_key_bytes);
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);

    Ok(AccountAddress::from(address))
}

fn _hex_to_bytes(hex: &str) -> Result<[u8; 32], ()> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() != 64 {
        return Err(());
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let high = _hex_char_to_nibble(chunk[0])?;
        let low = _hex_char_to_nibble(chunk[1])?;
        bytes[i] = (high << 4) | low;
    }
    Ok(bytes)
}

fn _hex_char_to_nibble(c: u8) -> Result<u8, ()> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_separator() {
        let sep = compute_domain_separator(DEFAULT_CHAIN_ID);
        assert_eq!(sep.len(), 32);
    }

    #[test]
    fn test_different_chain_ids_produce_different_separators() {
        let sep1 = compute_domain_separator(1);
        let sep2 = compute_domain_separator(1337);
        assert_ne!(sep1, sep2);
    }

    #[test]
    fn test_order_wire_hash() {
        let order = OrderWire {
            a: 0,
            b: true,
            p: "50000.0".to_string(),
            s: "0.1".to_string(),
            r: false,
            t: OrderTypeWire::Limit {
                limit: LimitTif { tif: "Gtc".to_string() },
            },
            c: None,
        };
        let hash = order.compute_hash();
        assert_eq!(hash.len(), 32);
    }
}
