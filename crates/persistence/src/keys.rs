//! Key Encoding for RocksDB
//!
//! This module provides efficient binary key encoding for different
//! data types. Keys are designed for:
//! - Deterministic ordering (big-endian for integers)
//! - Efficient prefix scans
//! - Minimal size
//!
//! ## Type Sizes
//! - TokenIndex: u8 (1 byte)
//! - MarketId: u8 (1 byte)
//! - OrderId: u64 (8 bytes)
//! - AccountAddress: 20 bytes

use hypercore_primitives::{AccountAddress, MarketId, OrderId, TokenIndex};

/// Key prefix constants for different data types
#[derive(Debug, Clone, Copy)]
pub enum KeyPrefix {
    /// Balance key prefix: account + token
    Balance,
    /// Position key prefix: account + market
    Position,
    /// Order key prefix: market + order_id
    Order,
    /// Account order index prefix: account + market
    AccountOrder,
    /// Nonce prefix: account
    Nonce,
    /// Block metadata prefix: height
    Block,
    /// CLOID index prefix: account + cloid
    Cloid,
    /// EVM storage prefix: address + slot
    EvmStorage,
}

/// Key encoder for building binary keys
pub struct KeyEncoder;

impl KeyEncoder {
    // ==================== BALANCE KEYS ====================

    /// Encode a balance key: account (20 bytes) + token (1 byte)
    pub fn balance_key(account: &AccountAddress, token: TokenIndex) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.extend_from_slice(account.as_slice());
        key.push(token); // TokenIndex is u8
        key
    }

    /// Decode a balance key
    pub fn decode_balance_key(key: &[u8]) -> Option<(AccountAddress, TokenIndex)> {
        if key.len() != 21 {
            return None;
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&key[0..20]);
        let account = AccountAddress::from(addr_bytes);
        let token = key[20]; // TokenIndex is u8
        Some((account, token))
    }

    /// Get prefix for all balances of an account
    pub fn balance_prefix(account: &AccountAddress) -> Vec<u8> {
        account.as_slice().to_vec()
    }

    // ==================== POSITION KEYS ====================

    /// Encode a position key: account (20 bytes) + market (1 byte)
    pub fn position_key(account: &AccountAddress, market: MarketId) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.extend_from_slice(account.as_slice());
        key.push(market); // MarketId is u8
        key
    }

    /// Decode a position key
    pub fn decode_position_key(key: &[u8]) -> Option<(AccountAddress, MarketId)> {
        if key.len() != 21 {
            return None;
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&key[0..20]);
        let account = AccountAddress::from(addr_bytes);
        let market = key[20]; // MarketId is u8
        Some((account, market))
    }

    /// Get prefix for all positions of an account
    pub fn position_prefix(account: &AccountAddress) -> Vec<u8> {
        account.as_slice().to_vec()
    }

    // ==================== ORDER KEYS ====================

    /// Encode an order key: market (1 byte) + order_id (8 bytes)
    pub fn order_key(market: MarketId, order_id: OrderId) -> Vec<u8> {
        let mut key = Vec::with_capacity(9);
        key.push(market); // MarketId is u8
        key.extend_from_slice(&order_id.to_be_bytes());
        key
    }

    /// Decode an order key
    pub fn decode_order_key(key: &[u8]) -> Option<(MarketId, OrderId)> {
        if key.len() != 9 {
            return None;
        }
        let market = key[0]; // MarketId is u8
        let order_id = OrderId::from_be_bytes([
            key[1], key[2], key[3], key[4], key[5], key[6], key[7], key[8],
        ]);
        Some((market, order_id))
    }

    /// Get prefix for all orders in a market
    pub fn order_market_prefix(market: MarketId) -> Vec<u8> {
        vec![market] // MarketId is u8
    }

    // ==================== ACCOUNT ORDER INDEX KEYS ====================

    /// Encode an account order index key: account (20 bytes) + market (1 byte)
    pub fn account_order_key(account: &AccountAddress, market: MarketId) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.extend_from_slice(account.as_slice());
        key.push(market); // MarketId is u8
        key
    }

    // ==================== NONCE KEYS ====================

    /// Encode a nonce key: account (20 bytes)
    pub fn nonce_key(account: &AccountAddress) -> Vec<u8> {
        account.as_slice().to_vec()
    }

    // ==================== BLOCK KEYS ====================

    /// Encode a block key: height (8 bytes, big-endian for ordering)
    pub fn block_key(height: u64) -> Vec<u8> {
        height.to_be_bytes().to_vec()
    }

    /// Decode a block key
    pub fn decode_block_key(key: &[u8]) -> Option<u64> {
        if key.len() != 8 {
            return None;
        }
        Some(u64::from_be_bytes([
            key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7],
        ]))
    }

    // ==================== CLOID KEYS ====================

    /// Encode a CLOID index key: account (20 bytes) + cloid (variable, max 64 bytes)
    pub fn cloid_key(account: &AccountAddress, cloid: &str) -> Vec<u8> {
        let cloid_bytes = cloid.as_bytes();
        let mut key = Vec::with_capacity(20 + cloid_bytes.len());
        key.extend_from_slice(account.as_slice());
        key.extend_from_slice(cloid_bytes);
        key
    }

    /// Get prefix for all CLOIDs of an account
    pub fn cloid_prefix(account: &AccountAddress) -> Vec<u8> {
        account.as_slice().to_vec()
    }

    // ==================== EVM STORAGE KEYS ====================

    /// Encode an EVM storage key: address (20 bytes) + slot (32 bytes)
    pub fn evm_storage_key(address: &[u8; 20], slot: &[u8; 32]) -> Vec<u8> {
        let mut key = Vec::with_capacity(52);
        key.extend_from_slice(address);
        key.extend_from_slice(slot);
        key
    }

    /// Decode an EVM storage key
    pub fn decode_evm_storage_key(key: &[u8]) -> Option<([u8; 20], [u8; 32])> {
        if key.len() != 52 {
            return None;
        }
        let mut address = [0u8; 20];
        let mut slot = [0u8; 32];
        address.copy_from_slice(&key[0..20]);
        slot.copy_from_slice(&key[20..52]);
        Some((address, slot))
    }

    /// Get prefix for all storage of a contract
    pub fn evm_storage_prefix(address: &[u8; 20]) -> Vec<u8> {
        address.to_vec()
    }

    // ==================== TOKEN KEYS ====================

    /// Encode a token key: token_index (1 byte)
    pub fn token_key(index: TokenIndex) -> Vec<u8> {
        vec![index] // TokenIndex is u8
    }

    /// Decode a token key
    pub fn decode_token_key(key: &[u8]) -> Option<TokenIndex> {
        if key.len() != 1 {
            return None;
        }
        Some(key[0]) // TokenIndex is u8
    }

    // ==================== MARKET KEYS ====================

    /// Encode a market key: market_id (1 byte)
    pub fn market_key(market: MarketId) -> Vec<u8> {
        vec![market] // MarketId is u8
    }

    /// Decode a market key
    pub fn decode_market_key(key: &[u8]) -> Option<MarketId> {
        if key.len() != 1 {
            return None;
        }
        Some(key[0]) // MarketId is u8
    }

    // ==================== METADATA KEYS ====================

    /// Well-known metadata keys
    pub const METADATA_HEIGHT: &'static [u8] = b"height";
    pub const METADATA_TIMESTAMP: &'static [u8] = b"timestamp";
    pub const METADATA_NEXT_ORDER_ID: &'static [u8] = b"next_order_id";
    pub const METADATA_NEXT_TOKEN_INDEX: &'static [u8] = b"next_token_index";
    pub const METADATA_NEXT_MARKET_ID: &'static [u8] = b"next_market_id";
    pub const METADATA_INSURANCE_FUND: &'static [u8] = b"insurance_fund";
    pub const METADATA_SCHEMA_VERSION: &'static [u8] = b"schema_version";

    // ==================== FILL/TRADE KEYS ====================

    /// Encode a fill key: account (20 bytes) + timestamp (8 bytes) + fill_id (8 bytes)
    pub fn fill_key(account: &AccountAddress, timestamp: u64, fill_id: u64) -> Vec<u8> {
        let mut key = Vec::with_capacity(36);
        key.extend_from_slice(account.as_slice());
        key.extend_from_slice(&timestamp.to_be_bytes());
        key.extend_from_slice(&fill_id.to_be_bytes());
        key
    }

    /// Encode a trade key: market (8 bytes) + timestamp (8 bytes) + trade_id (8 bytes)
    pub fn trade_key(market: MarketId, timestamp: u64, trade_id: u64) -> Vec<u8> {
        let mut key = Vec::with_capacity(24);
        key.extend_from_slice(&market.to_be_bytes());
        key.extend_from_slice(&timestamp.to_be_bytes());
        key.extend_from_slice(&trade_id.to_be_bytes());
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_key_roundtrip() {
        let account = AccountAddress::from([1u8; 20]);
        let token: TokenIndex = 42;

        let key = KeyEncoder::balance_key(&account, token);
        assert_eq!(key.len(), 21); // 20 bytes address + 1 byte token

        let (decoded_account, decoded_token) =
            KeyEncoder::decode_balance_key(&key).unwrap();
        assert_eq!(decoded_account, account);
        assert_eq!(decoded_token, token);
    }

    #[test]
    fn test_order_key_roundtrip() {
        let market: MarketId = 5; // u8
        let order_id: OrderId = 456;

        let key = KeyEncoder::order_key(market, order_id);
        assert_eq!(key.len(), 9); // 1 byte market + 8 bytes order_id

        let (decoded_market, decoded_order_id) =
            KeyEncoder::decode_order_key(&key).unwrap();
        assert_eq!(decoded_market, market);
        assert_eq!(decoded_order_id, order_id);
    }

    #[test]
    fn test_block_key_ordering() {
        // Keys should be ordered by height (big-endian)
        let key1 = KeyEncoder::block_key(100);
        let key2 = KeyEncoder::block_key(200);
        let key3 = KeyEncoder::block_key(1);

        assert!(key3 < key1);
        assert!(key1 < key2);
    }

    #[test]
    fn test_evm_storage_key_roundtrip() {
        let address = [1u8; 20];
        let slot = [2u8; 32];

        let key = KeyEncoder::evm_storage_key(&address, &slot);
        assert_eq!(key.len(), 52);

        let (decoded_addr, decoded_slot) =
            KeyEncoder::decode_evm_storage_key(&key).unwrap();
        assert_eq!(decoded_addr, address);
        assert_eq!(decoded_slot, slot);
    }
}
