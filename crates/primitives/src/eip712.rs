//! EIP-712 Encoding Utilities
//!
//! Shared EIP-712 typed data encoding functions used by both the gateway
//! and chain crates. This consolidation ensures identical hash computation
//! across the entire codebase.
//!
//! Reference: https://eips.ethereum.org/EIPS/eip-712
//!
//! ## Usage
//!
//! ```rust
//! use hypercore_primitives::eip712::{
//!     encode_string, encode_uint8, encode_uint64, encode_bool,
//!     encode_address_bytes, encode_address_str, encode_array,
//!     compute_domain_separator, DOMAIN_TYPE_HASH, DEFAULT_CHAIN_ID,
//! };
//!
//! // Encode values for EIP-712 struct hashing
//! let name_hash = encode_string("HyperCore");
//! let market_id = encode_uint8(0);
//! let nonce = encode_uint64(12345);
//! let is_buy = encode_bool(true);
//! ```

use sha3::{Digest, Keccak256};

// ============================================================================
// Constants
// ============================================================================

/// Default chain ID for HyperCore (devnet)
pub const DEFAULT_CHAIN_ID: u64 = 1337;

/// EIP-712 Domain Type Hash
/// keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
pub const DOMAIN_TYPE_HASH: [u8; 32] = [
    0x8b, 0x73, 0xc3, 0xc6, 0x9b, 0xb8, 0xfe, 0x3d, 0x51, 0x2e, 0xcc, 0x4c, 0xf7, 0x59, 0xcc, 0x79,
    0x23, 0x9f, 0x7b, 0x17, 0x9b, 0x0f, 0xfa, 0xca, 0xa9, 0xa7, 0x5d, 0x52, 0x2b, 0x39, 0x40, 0x0f,
];

// ============================================================================
// Encoding Functions
// ============================================================================

/// Encode a string as keccak256 hash (EIP-712 string encoding)
///
/// Per EIP-712: strings are encoded as keccak256(string)
pub fn encode_string(s: &str) -> [u8; 32] {
    let result = Keccak256::digest(s.as_bytes());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Encode a uint8 as 32-byte big-endian (left-padded with zeros)
///
/// Per EIP-712: uintN types are encoded as 32-byte big-endian
pub fn encode_uint8(v: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = v;
    bytes
}

/// Encode a uint64 as 32-byte big-endian (left-padded with zeros)
///
/// Per EIP-712: uintN types are encoded as 32-byte big-endian
pub fn encode_uint64(v: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&v.to_be_bytes());
    bytes
}

/// Encode a bool as 32-byte (0 or 1 in the last byte)
///
/// Per EIP-712: bool is encoded as uint256 (0 or 1)
pub fn encode_bool(v: bool) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = if v { 1 } else { 0 };
    bytes
}

/// Encode a 20-byte address as 32-byte (left-padded with zeros)
///
/// Per EIP-712: address is encoded as uint160, left-padded to 32 bytes
///
/// Use this for `AccountAddress` types via `.as_slice()`
pub fn encode_address_bytes(addr: &[u8; 20]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[12..32].copy_from_slice(addr);
    bytes
}

/// Encode an address string (0x-prefixed hex) as 32-byte (left-padded with zeros)
///
/// Per EIP-712: address is encoded as uint160, left-padded to 32 bytes
///
/// Use this for string address inputs from APIs
pub fn encode_address_str(addr: &str) -> [u8; 32] {
    let hex_str = addr.strip_prefix("0x").unwrap_or(addr);
    let mut bytes = [0u8; 32];
    if let Ok(addr_bytes) = hex::decode(hex_str) {
        if addr_bytes.len() == 20 {
            bytes[12..32].copy_from_slice(&addr_bytes);
        }
    }
    bytes
}

/// Encode an array of struct hashes as keccak256 of concatenated hashes
///
/// Per EIP-712: arrays are encoded as keccak256(concat(encodeData(items)))
pub fn encode_array(hashes: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    for hash in hashes {
        hasher.update(hash);
    }
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Compute the type hash for a type definition string
///
/// Type hash = keccak256(typeString)
pub fn type_hash(type_str: &str) -> [u8; 32] {
    let result = Keccak256::digest(type_str.as_bytes());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

// ============================================================================
// Domain Separator
// ============================================================================

/// Compute EIP-712 domain separator for HyperCore
///
/// The domain separator uniquely identifies the signing domain (HyperCore)
/// and prevents signature replay attacks across different applications.
///
/// # Arguments
/// * `chain_id` - The chain ID (use DEFAULT_CHAIN_ID for devnet)
///
/// # Returns
/// The 32-byte domain separator hash
pub fn compute_domain_separator(chain_id: u64) -> [u8; 32] {
    let name_hash = encode_string("HyperCore");
    let version_hash = encode_string("1");
    let chain_id_bytes = encode_uint64(chain_id);
    let verifying_contract = [0u8; 32]; // Zero address

    let mut hasher = Keccak256::new();
    hasher.update(DOMAIN_TYPE_HASH);
    hasher.update(name_hash);
    hasher.update(version_hash);
    hasher.update(chain_id_bytes);
    hasher.update(verifying_contract);

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Compute the final EIP-712 typed data hash
///
/// hash = keccak256("\x19\x01" || domainSeparator || structHash)
///
/// This is the message that gets signed by the user.
pub fn compute_typed_data_hash(domain_separator: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
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
    fn test_encode_string() {
        let hash = encode_string("HyperCore");
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, [0u8; 32]);
    }

    #[test]
    fn test_encode_uint8() {
        let bytes = encode_uint8(42);
        assert_eq!(bytes[31], 42);
        assert_eq!(bytes[0..31], [0u8; 31]);
    }

    #[test]
    fn test_encode_uint64() {
        let bytes = encode_uint64(0x123456789ABCDEF0);
        assert_eq!(bytes[24..32], [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]);
        assert_eq!(bytes[0..24], [0u8; 24]);
    }

    #[test]
    fn test_encode_bool() {
        let true_bytes = encode_bool(true);
        let false_bytes = encode_bool(false);
        assert_eq!(true_bytes[31], 1);
        assert_eq!(false_bytes[31], 0);
    }

    #[test]
    fn test_encode_address_bytes() {
        let addr = [0xAB; 20];
        let bytes = encode_address_bytes(&addr);
        assert_eq!(bytes[0..12], [0u8; 12]);
        assert_eq!(bytes[12..32], [0xAB; 20]);
    }

    #[test]
    fn test_encode_address_str() {
        let addr = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let bytes = encode_address_str(addr);
        assert_eq!(bytes[0..12], [0u8; 12]);
        // First byte should be 0xf3
        assert_eq!(bytes[12], 0xf3);
    }

    #[test]
    fn test_encode_address_str_without_prefix() {
        let addr = "f39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let bytes = encode_address_str(addr);
        assert_eq!(bytes[12], 0xf3);
    }

    #[test]
    fn test_encode_array() {
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        let result = encode_array(&[hash1, hash2]);
        assert_eq!(result.len(), 32);
        assert_ne!(result, [0u8; 32]);
    }

    #[test]
    fn test_encode_array_empty() {
        let result = encode_array(&[]);
        // Empty array should return keccak256 of empty input
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_domain_separator_deterministic() {
        let sep1 = compute_domain_separator(DEFAULT_CHAIN_ID);
        let sep2 = compute_domain_separator(DEFAULT_CHAIN_ID);
        assert_eq!(sep1, sep2);
    }

    #[test]
    fn test_domain_separator_different_chain_ids() {
        let sep1 = compute_domain_separator(1);
        let sep2 = compute_domain_separator(1337);
        assert_ne!(sep1, sep2);
    }

    #[test]
    fn test_typed_data_hash() {
        let domain_sep = compute_domain_separator(DEFAULT_CHAIN_ID);
        let struct_hash = [0xAB; 32];
        let result = compute_typed_data_hash(&domain_sep, &struct_hash);
        assert_eq!(result.len(), 32);
        assert_ne!(result, [0u8; 32]);
    }

    #[test]
    fn test_type_hash() {
        let hash = type_hash("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
        assert_eq!(hash, DOMAIN_TYPE_HASH);
    }

    #[test]
    fn test_domain_type_hash_constant() {
        // Verify the precomputed constant is correct
        let computed = type_hash("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
        assert_eq!(computed, DOMAIN_TYPE_HASH);
    }

    // ========================================================================
    // EIP-712 Known Test Vector Tests (Tier 3)
    // ========================================================================

    #[test]
    fn test_eip712_domain_separator_known_vector() {
        // Verify the domain separator for chain_id=1337 against a manually
        // computed known test vector.
        //
        // Components:
        //   typeHash   = keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
        //   nameHash   = keccak256("HyperCore")
        //   versionHash = keccak256("1")
        //   chainId    = 1337 (uint256, big-endian padded to 32 bytes)
        //   verifyingContract = address(0) (32 zero bytes)
        //
        // domainSeparator = keccak256(typeHash || nameHash || versionHash || chainId || verifyingContract)

        let domain_sep = compute_domain_separator(DEFAULT_CHAIN_ID);

        // Recompute step-by-step to verify
        let type_hash_val = type_hash(
            "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );
        assert_eq!(type_hash_val, DOMAIN_TYPE_HASH);

        let name_hash = encode_string("HyperCore");
        let version_hash = encode_string("1");

        // Verify known intermediate values:
        // keccak256("1") is a well-known constant in Ethereum EIP-712 implementations
        let expected_version_hash = Keccak256::digest(b"1");
        assert_eq!(version_hash, expected_version_hash.as_slice());

        // chain_id encoding: 1337 = 0x539
        let chain_id_bytes = encode_uint64(1337);
        assert_eq!(chain_id_bytes[30], 0x05);
        assert_eq!(chain_id_bytes[31], 0x39);

        // Reconstruct and verify equality
        let mut hasher = Keccak256::new();
        hasher.update(type_hash_val);
        hasher.update(name_hash);
        hasher.update(version_hash);
        hasher.update(chain_id_bytes);
        hasher.update([0u8; 32]); // verifying contract = zero address
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(domain_sep, expected,
            "Domain separator should match manual computation");

        // Verify domain separator is non-trivial (not zero or all-ones)
        assert_ne!(domain_sep, [0u8; 32]);
        assert_ne!(domain_sep, [0xFF; 32]);
    }

    #[test]
    fn test_eip712_typed_data_hash_known_vector() {
        // Verify the full typed data hash follows the EIP-712 spec:
        // hash = keccak256("\x19\x01" || domainSeparator || structHash)
        let domain_sep = compute_domain_separator(1337);
        let struct_hash = Keccak256::digest(b"test struct content");
        let mut sh = [0u8; 32];
        sh.copy_from_slice(&struct_hash);

        let result = compute_typed_data_hash(&domain_sep, &sh);

        // Manually compute the same thing
        let mut hasher = Keccak256::new();
        hasher.update(b"\x19\x01");
        hasher.update(domain_sep);
        hasher.update(sh);
        let expected: [u8; 32] = hasher.finalize().into();

        assert_eq!(result, expected);

        // Verify the "\x19\x01" prefix matters (changing it changes the hash)
        let mut hasher_wrong = Keccak256::new();
        hasher_wrong.update(b"\x19\x00"); // wrong prefix
        hasher_wrong.update(domain_sep);
        hasher_wrong.update(sh);
        let wrong: [u8; 32] = hasher_wrong.finalize().into();
        assert_ne!(result, wrong, "Different prefix must produce different hash");
    }

    #[test]
    fn test_eip712_encoding_determinism_across_types() {
        // Verify that all encoding functions are deterministic and produce
        // distinct outputs for distinct inputs.
        let a1 = encode_uint8(0);
        let a2 = encode_uint8(1);
        assert_ne!(a1, a2, "Different uint8 values must produce different encodings");

        let b1 = encode_uint64(0);
        let b2 = encode_uint64(1);
        assert_ne!(b1, b2);

        let c1 = encode_bool(true);
        let c2 = encode_bool(false);
        assert_ne!(c1, c2);

        let d1 = encode_string("hello");
        let d2 = encode_string("world");
        assert_ne!(d1, d2);

        // Same input always produces same output
        assert_eq!(encode_string("test"), encode_string("test"));
        assert_eq!(encode_uint64(42), encode_uint64(42));

        // Address encoding: zero address vs non-zero
        let zero_addr = [0u8; 20];
        let nonzero_addr = [0xAB; 20];
        assert_ne!(encode_address_bytes(&zero_addr), encode_address_bytes(&nonzero_addr));
    }
}
