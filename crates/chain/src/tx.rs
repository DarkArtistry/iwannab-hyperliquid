//! Transaction types for HyperCore chain

use hypercore_primitives::{
    AccountAddress, Decimal, MarketId, OrderId, OrderRequest, OrderSide, OrderType, Signature,
    TimeInForce,
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

    /// Recover signer address from signature
    pub fn recover_signer(&self) -> Result<AccountAddress, TransactionError> {
        // EIP-712 typed data hash recovery
        let typed_hash = self.compute_eip712_hash()?;
        recover_address(&typed_hash, &self.signature)
    }

    /// Get sender address (alias for recover_signer)
    pub fn sender(&self) -> Result<AccountAddress, TransactionError> {
        self.recover_signer()
    }

    /// Compute EIP-712 typed data hash for signing
    fn compute_eip712_hash(&self) -> Result<[u8; 32], TransactionError> {
        use sha3::{Digest, Keccak256};

        // Domain separator
        let domain_separator = compute_domain_separator();

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
}

impl TransactionType {
    /// Compute EIP-712 struct hash for this action
    pub fn compute_struct_hash(&self, nonce: u64) -> Result<[u8; 32], TransactionError> {
        use sha3::{Digest, Keccak256};

        match self {
            TransactionType::Order { orders, grouping } => {
                // Hash each order
                let mut order_hashes = Vec::with_capacity(orders.len());
                for order in orders {
                    order_hashes.push(order.compute_hash());
                }

                // Hash the orders array
                let orders_hash: [u8; 32] = if order_hashes.is_empty() {
                    Keccak256::digest(b"").into()
                } else {
                    let mut combined = Vec::new();
                    for h in order_hashes {
                        combined.extend_from_slice(&h);
                    }
                    Keccak256::digest(&combined).into()
                };

                // Type hash
                let type_hash = Keccak256::digest(
                    b"Action(string type,Order[] orders,string grouping,uint64 nonce)Order(uint8 a,bool b,string p,string s,bool r,string t)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(Keccak256::digest(b"order"));
                hasher.update(orders_hash);
                hasher.update(Keccak256::digest(grouping.as_str().as_bytes()));
                hasher.update(&nonce.to_be_bytes());
                Ok(hasher.finalize().into())
            }
            TransactionType::Cancel { cancels } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,Cancel[] cancels,uint64 nonce)Cancel(uint8 a,uint64 o)"
                );

                let cancels_hash = {
                    let mut combined = Vec::new();
                    for c in cancels {
                        combined.extend_from_slice(&c.compute_hash());
                    }
                    Keccak256::digest(&combined)
                };

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(Keccak256::digest(b"cancel"));
                hasher.update(cancels_hash);
                hasher.update(&nonce.to_be_bytes());
                Ok(hasher.finalize().into())
            }
            TransactionType::UpdateLeverage { asset, is_cross, leverage } => {
                let type_hash = Keccak256::digest(
                    b"Action(string type,uint8 asset,bool isCross,uint8 leverage,uint64 nonce)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(Keccak256::digest(b"updateLeverage"));
                hasher.update([*asset]);
                hasher.update([if *is_cross { 1u8 } else { 0u8 }]);
                hasher.update([*leverage]);
                hasher.update(&nonce.to_be_bytes());
                Ok(hasher.finalize().into())
            }
            TransactionType::UsdTransfer { destination, amount } |
            TransactionType::Withdraw { destination, amount } => {
                let type_name: &[u8] = match self {
                    TransactionType::UsdTransfer { .. } => b"usdTransfer",
                    TransactionType::Withdraw { .. } => b"withdraw",
                    _ => unreachable!(),
                };

                let type_hash = Keccak256::digest(
                    b"Action(string type,address destination,string amount,uint64 nonce)"
                );

                let mut hasher = Keccak256::new();
                hasher.update(type_hash);
                hasher.update(Keccak256::digest(type_name));
                hasher.update(destination.as_slice());
                hasher.update(Keccak256::digest(amount.as_bytes()));
                hasher.update(&nonce.to_be_bytes());
                Ok(hasher.finalize().into())
            }
            _ => {
                // Generic hash for other types
                let encoded = serde_json::to_vec(self)
                    .map_err(|_| TransactionError::SerializationError)?;
                let mut hasher = Keccak256::new();
                hasher.update(&encoded);
                hasher.update(&nonce.to_be_bytes());
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
    pub fn compute_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(
            b"Order(uint8 a,bool b,string p,string s,bool r,string t)"
        );
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update([self.a]);
        hasher.update([if self.b { 1u8 } else { 0u8 }]);
        hasher.update(Keccak256::digest(self.p.as_bytes()));
        hasher.update(Keccak256::digest(self.s.as_bytes()));
        hasher.update([if self.r { 1u8 } else { 0u8 }]);
        hasher.update(Keccak256::digest(self.t.to_string().as_bytes()));
        hasher.finalize().into()
    }

    /// Convert to internal order request
    pub fn to_order_request(&self, _owner: AccountAddress) -> Result<OrderRequest, TransactionError> {
        let price = Decimal::from_str(&self.p)
            .map_err(|_| TransactionError::InvalidPrice)?;
        let size = Decimal::from_str(&self.s)
            .map_err(|_| TransactionError::InvalidSize)?;
        let order_type = self.t.to_order_type()?;

        Ok(OrderRequest {
            market_id: self.a,
            side: if self.b { OrderSide::Buy } else { OrderSide::Sell },
            price,
            size,
            order_type,
            reduce_only: self.r,
            client_order_id: self.c.clone(),
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
    pub fn compute_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let type_hash = Keccak256::digest(b"Cancel(uint8 a,uint64 o)");
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update([self.a]);
        hasher.update(&self.o.to_be_bytes());
        hasher.finalize().into()
    }
}

/// Wire format for cancel by client order ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelByCloidWire {
    pub asset: MarketId,
    pub cloid: String,
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
    #[error("Serialization error")]
    SerializationError,
    #[error("Unknown market: {0}")]
    UnknownMarket(MarketId),
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Signature recovery failed")]
    SignatureRecoveryFailed,
}

/// Compute EIP-712 domain separator for HyperCore
fn compute_domain_separator() -> [u8; 32] {
    use sha3::{Digest, Keccak256};

    let type_hash = Keccak256::digest(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
    );

    let name_hash = Keccak256::digest(b"HyperCore");
    let version_hash = Keccak256::digest(b"1");
    let chain_id: [u8; 32] = {
        let mut buf = [0u8; 32];
        buf[31] = 0x39; // 1337 in last bytes
        buf[30] = 0x05;
        buf
    };
    let verifying_contract = [0u8; 32]; // Zero address

    let mut hasher = Keccak256::new();
    hasher.update(type_hash);
    hasher.update(name_hash);
    hasher.update(version_hash);
    hasher.update(chain_id);
    hasher.update(verifying_contract);
    hasher.finalize().into()
}

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

fn hex_to_bytes(hex: &str) -> Result<[u8; 32], ()> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() != 64 {
        return Err(());
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let high = hex_char_to_nibble(chunk[0])?;
        let low = hex_char_to_nibble(chunk[1])?;
        bytes[i] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_char_to_nibble(c: u8) -> Result<u8, ()> {
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
        let sep = compute_domain_separator();
        assert_eq!(sep.len(), 32);
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
