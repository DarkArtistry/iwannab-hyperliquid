//! EIP-712 Typed Data Encoding
//!
//! This module implements proper EIP-712 typed structured data hashing
//! that matches the TypeScript SDK's viem implementation exactly.
//!
//! Reference: https://eips.ethereum.org/EIPS/eip-712
//!
//! Note: Core encoding functions are imported from hypercore_primitives::eip712
//! to ensure consistency with the chain crate's implementation.

use sha3::{Digest, Keccak256};

use crate::api::{
    CancelByCloidWire, CancelWire, ExchangeAction, ExchangeRequest, OrderTypeWire, OrderWire,
    SpotCancelWire, SpotOrderWire,
};

// Re-export shared constants and functions from primitives
pub use hypercore_primitives::eip712::{
    compute_domain_separator, DEFAULT_CHAIN_ID, DOMAIN_TYPE_HASH,
};

// Import shared encoding functions (used internally)
use hypercore_primitives::eip712::{
    encode_array, encode_bool, encode_string, encode_uint64, encode_uint8,
    type_hash, encode_address_str as encode_address,
};

// ============================================================================
// Type Hashes for Action Types
// ============================================================================

/// Get the type hash for Order action
/// Action(string type,Order[] orders,string grouping,uint64 nonce)Order(uint8 a,bool b,string p,string s,bool r,string t)
fn order_action_type_hash() -> [u8; 32] {
    type_hash(
        "Action(string type,Order[] orders,string grouping,uint64 nonce)Order(uint8 a,bool b,string p,string s,bool r,string t)"
    )
}

/// Get the type hash for Order struct
/// Order(uint8 a,bool b,string p,string s,bool r,string t)
fn order_struct_type_hash() -> [u8; 32] {
    type_hash("Order(uint8 a,bool b,string p,string s,bool r,string t)")
}

/// Get the type hash for Cancel action
/// Action(string type,Cancel[] cancels,uint64 nonce)Cancel(uint8 a,uint64 o)
fn cancel_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,Cancel[] cancels,uint64 nonce)Cancel(uint8 a,uint64 o)")
}

/// Get the type hash for Cancel struct
/// Cancel(uint8 a,uint64 o)
fn cancel_struct_type_hash() -> [u8; 32] {
    type_hash("Cancel(uint8 a,uint64 o)")
}

/// Get the type hash for CancelByCloid action
/// Action(string type,CancelByCloid[] cancels,uint64 nonce)CancelByCloid(uint8 asset,string cloid)
fn cancel_by_cloid_action_type_hash() -> [u8; 32] {
    type_hash(
        "Action(string type,CancelByCloid[] cancels,uint64 nonce)CancelByCloid(uint8 asset,string cloid)"
    )
}

/// Get the type hash for CancelByCloid struct
fn cancel_by_cloid_struct_type_hash() -> [u8; 32] {
    type_hash("CancelByCloid(uint8 asset,string cloid)")
}

/// Get the type hash for CancelAll action
/// Action(string type,uint64 nonce)
fn cancel_all_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,uint64 nonce)")
}

/// Get the type hash for UpdateLeverage action
/// Action(string type,uint8 asset,bool isCross,uint8 leverage,uint64 nonce)
fn update_leverage_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,uint8 asset,bool isCross,uint8 leverage,uint64 nonce)")
}

/// Get the type hash for UsdTransfer/Withdraw action
/// Action(string type,address destination,string amount,uint64 nonce)
fn transfer_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,address destination,string amount,uint64 nonce)")
}

/// Get the type hash for SpotOrder action
/// Action(string type,SpotOrder[] orders,string grouping,uint64 nonce)SpotOrder(...)
fn spot_order_action_type_hash() -> [u8; 32] {
    type_hash(
        "Action(string type,SpotOrder[] orders,string grouping,uint64 nonce)SpotOrder(uint8 a,bool b,string p,string s,string t)"
    )
}

/// Get the type hash for SpotOrder struct
fn spot_order_struct_type_hash() -> [u8; 32] {
    type_hash("SpotOrder(uint8 a,bool b,string p,string s,string t)")
}

/// Get the type hash for SpotCancel action
fn spot_cancel_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,SpotCancel[] cancels,uint64 nonce)SpotCancel(uint8 a,uint64 o)")
}

/// Get the type hash for SpotCancel struct
fn spot_cancel_struct_type_hash() -> [u8; 32] {
    type_hash("SpotCancel(uint8 a,uint64 o)")
}

/// Get the type hash for SpotCancelAll action
fn spot_cancel_all_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,uint64 nonce)")
}

/// Get the type hash for ViewTransfer action
/// Action(string type,uint8 token,string amount,bool toEvm,uint64 nonce)
fn view_transfer_action_type_hash() -> [u8; 32] {
    type_hash("Action(string type,uint8 token,string amount,bool toEvm,uint64 nonce)")
}

// ============================================================================
// Action-Specific Helpers
// ============================================================================

/// Convert OrderTypeWire to TIF string (lowercase for EIP-712 consistency)
fn order_type_to_tif(t: &OrderTypeWire) -> String {
    match t {
        OrderTypeWire::Limit { limit } => limit.tif.to_lowercase(),
        OrderTypeWire::Trigger { trigger: _ } => "trigger".to_string(),
    }
}

// ============================================================================
// Struct Encoding
// ============================================================================

/// Hash an Order struct
/// Order(uint8 a, bool b, string p, string s, bool r, string t)
fn hash_order(order: &OrderWire) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(order_struct_type_hash());
    hasher.update(encode_uint8(order.a));
    hasher.update(encode_bool(order.b));
    hasher.update(encode_string(&order.p));
    hasher.update(encode_string(&order.s));
    hasher.update(encode_bool(order.r));
    hasher.update(encode_string(&order_type_to_tif(&order.t)));

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Hash a Cancel struct
/// Cancel(uint8 a, uint64 o)
fn hash_cancel(cancel: &CancelWire) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(cancel_struct_type_hash());
    hasher.update(encode_uint8(cancel.a));
    hasher.update(encode_uint64(cancel.o));

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Hash a CancelByCloid struct
/// CancelByCloid(uint8 asset, string cloid)
fn hash_cancel_by_cloid(cancel: &CancelByCloidWire) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(cancel_by_cloid_struct_type_hash());
    hasher.update(encode_uint8(cancel.asset));
    hasher.update(encode_string(&cancel.cloid));

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Hash a SpotOrder struct
/// SpotOrder(uint8 a, bool b, string p, string s, string t)
fn hash_spot_order(order: &SpotOrderWire) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(spot_order_struct_type_hash());
    hasher.update(encode_uint8(order.a));
    hasher.update(encode_bool(order.b));
    hasher.update(encode_string(&order.p));
    hasher.update(encode_string(&order.s));
    hasher.update(encode_string(&order_type_to_tif(&order.t)));

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Hash a SpotCancel struct
/// SpotCancel(uint8 a, uint64 o)
fn hash_spot_cancel(cancel: &SpotCancelWire) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(spot_cancel_struct_type_hash());
    hasher.update(encode_uint8(cancel.a));
    hasher.update(encode_uint64(cancel.o));

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

// ============================================================================
// Action Encoding
// ============================================================================

/// Compute the struct hash for an exchange action
pub fn compute_action_struct_hash(action: &ExchangeAction, nonce: u64) -> [u8; 32] {
    match action {
        ExchangeAction::Order { orders, grouping } => {
            // Action(string type, Order[] orders, string grouping, uint64 nonce)
            let order_hashes: Vec<[u8; 32]> = orders.iter().map(hash_order).collect();

            let mut hasher = Keccak256::new();
            hasher.update(order_action_type_hash());
            hasher.update(encode_string("order"));
            hasher.update(encode_array(&order_hashes));
            hasher.update(encode_string(grouping));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::Cancel { cancels } => {
            // Action(string type, Cancel[] cancels, uint64 nonce)
            let cancel_hashes: Vec<[u8; 32]> = cancels.iter().map(hash_cancel).collect();

            let mut hasher = Keccak256::new();
            hasher.update(cancel_action_type_hash());
            hasher.update(encode_string("cancel"));
            hasher.update(encode_array(&cancel_hashes));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::CancelByCloid { cancels } => {
            // Action(string type, CancelByCloid[] cancels, uint64 nonce)
            let cancel_hashes: Vec<[u8; 32]> = cancels.iter().map(hash_cancel_by_cloid).collect();

            let mut hasher = Keccak256::new();
            hasher.update(cancel_by_cloid_action_type_hash());
            hasher.update(encode_string("cancelByCloid"));
            hasher.update(encode_array(&cancel_hashes));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::CancelAll => {
            // Action(string type, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(cancel_all_action_type_hash());
            hasher.update(encode_string("cancelAll"));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::UpdateLeverage {
            asset,
            is_cross,
            leverage,
        } => {
            // Action(string type, uint8 asset, bool isCross, uint8 leverage, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(update_leverage_action_type_hash());
            hasher.update(encode_string("updateLeverage"));
            hasher.update(encode_uint8(*asset));
            hasher.update(encode_bool(*is_cross));
            hasher.update(encode_uint8(*leverage));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::UsdTransfer {
            destination,
            amount,
        } => {
            // Action(string type, address destination, string amount, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(transfer_action_type_hash());
            hasher.update(encode_string("usdTransfer"));
            hasher.update(encode_address(destination));
            hasher.update(encode_string(amount));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::Withdraw {
            destination,
            amount,
        } => {
            // Action(string type, address destination, string amount, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(transfer_action_type_hash());
            hasher.update(encode_string("withdraw"));
            hasher.update(encode_address(destination));
            hasher.update(encode_string(amount));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::SpotOrder { orders, grouping } => {
            // Action(string type, SpotOrder[] orders, string grouping, uint64 nonce)
            let order_hashes: Vec<[u8; 32]> = orders.iter().map(hash_spot_order).collect();

            let mut hasher = Keccak256::new();
            hasher.update(spot_order_action_type_hash());
            hasher.update(encode_string("spotOrder"));
            hasher.update(encode_array(&order_hashes));
            hasher.update(encode_string(grouping));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::SpotCancel { cancels } => {
            // Action(string type, SpotCancel[] cancels, uint64 nonce)
            let cancel_hashes: Vec<[u8; 32]> = cancels.iter().map(hash_spot_cancel).collect();

            let mut hasher = Keccak256::new();
            hasher.update(spot_cancel_action_type_hash());
            hasher.update(encode_string("spotCancel"));
            hasher.update(encode_array(&cancel_hashes));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::SpotCancelAll { market: _ } => {
            // Action(string type, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(spot_cancel_all_action_type_hash());
            hasher.update(encode_string("spotCancelAll"));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }

        ExchangeAction::ViewTransfer {
            token,
            amount,
            to_evm,
        } => {
            // Action(string type, uint8 token, string amount, bool toEvm, uint64 nonce)
            let mut hasher = Keccak256::new();
            hasher.update(view_transfer_action_type_hash());
            hasher.update(encode_string("viewTransfer"));
            hasher.update(encode_uint8(*token));
            hasher.update(encode_string(amount));
            hasher.update(encode_bool(*to_evm));
            hasher.update(encode_uint64(nonce));

            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        }
    }
}

/// Compute the full EIP-712 typed data hash
/// hash = keccak256("\x19\x01" || domainSeparator || structHash)
pub fn compute_typed_data_hash(request: &ExchangeRequest, chain_id: u64) -> [u8; 32] {
    let domain_separator = compute_domain_separator(chain_id);
    let struct_hash = compute_action_struct_hash(&request.action, request.nonce);

    let mut hasher = Keccak256::new();
    hasher.update(b"\x19\x01");
    hasher.update(domain_separator);
    hasher.update(struct_hash);

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_type_hash() {
        // Verify the domain type hash constant is correct
        let computed = type_hash(
            "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );
        assert_eq!(computed, DOMAIN_TYPE_HASH);
    }

    #[test]
    fn test_encode_string() {
        // Test string encoding (keccak256 of string)
        let hash = encode_string("HyperCore");
        // Should be deterministic
        let hash2 = encode_string("HyperCore");
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_encode_uint8() {
        let encoded = encode_uint8(5);
        assert_eq!(encoded[31], 5);
        for i in 0..31 {
            assert_eq!(encoded[i], 0);
        }
    }

    #[test]
    fn test_encode_uint64() {
        let encoded = encode_uint64(0x1234567890abcdef);
        assert_eq!(&encoded[24..32], &0x1234567890abcdefu64.to_be_bytes());
    }

    #[test]
    fn test_encode_bool() {
        let true_encoded = encode_bool(true);
        let false_encoded = encode_bool(false);
        assert_eq!(true_encoded[31], 1);
        assert_eq!(false_encoded[31], 0);
    }

    #[test]
    fn test_encode_address() {
        let addr = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let encoded = encode_address(addr);
        // First 12 bytes should be zero (left padding)
        for i in 0..12 {
            assert_eq!(encoded[i], 0);
        }
        // Last 20 bytes should be the address
        assert_eq!(encoded[12], 0xf3);
    }

    #[test]
    fn test_cancel_all_struct_hash() {
        // Test CancelAll action struct hash
        let action = ExchangeAction::CancelAll;
        let hash = compute_action_struct_hash(&action, 12345);
        // Hash should be non-zero and deterministic
        assert_ne!(hash, [0u8; 32]);
        let hash2 = compute_action_struct_hash(&action, 12345);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_different_nonces_different_hashes() {
        let action = ExchangeAction::CancelAll;
        let hash1 = compute_action_struct_hash(&action, 1);
        let hash2 = compute_action_struct_hash(&action, 2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_domain_separator_deterministic() {
        let sep1 = compute_domain_separator(1337);
        let sep2 = compute_domain_separator(1337);
        assert_eq!(sep1, sep2);
    }

    #[test]
    fn test_different_chain_ids() {
        let sep1 = compute_domain_separator(1);
        let sep2 = compute_domain_separator(1337);
        assert_ne!(sep1, sep2);
    }

    #[test]
    fn test_usd_transfer_struct_hash() {
        // Test UsdTransfer action struct hash computation
        // This should match what viem computes in TypeScript
        let action = ExchangeAction::UsdTransfer {
            destination: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            amount: "10".to_string(),
        };
        let nonce = 1234567890u64;
        let hash = compute_action_struct_hash(&action, nonce);

        // Print hash for debugging/comparison with TypeScript
        println!("UsdTransfer struct hash: 0x{}", hex::encode(hash));
        println!("Type hash: 0x{}", hex::encode(transfer_action_type_hash()));
        println!("Encoded 'usdTransfer': 0x{}", hex::encode(encode_string("usdTransfer")));
        println!("Encoded destination: 0x{}", hex::encode(encode_address("0x70997970C51812dc3A010C7d01b50e0d17dc79C8")));
        println!("Encoded amount '10': 0x{}", hex::encode(encode_string("10")));
        println!("Encoded nonce: 0x{}", hex::encode(encode_uint64(nonce)));

        // Hash should be non-zero
        assert_ne!(hash, [0u8; 32]);

        // Domain separator for chain 1337
        let domain_sep = compute_domain_separator(1337);
        println!("Domain separator (chain 1337): 0x{}", hex::encode(domain_sep));
    }

    #[test]
    fn test_transfer_type_hash_string() {
        // Verify the type hash string matches what viem expects
        let type_str = "Action(string type,address destination,string amount,uint64 nonce)";
        let hash = type_hash(type_str);
        assert_eq!(hash, transfer_action_type_hash());
        println!("Transfer type hash for '{}': 0x{}", type_str, hex::encode(hash));
    }
}
