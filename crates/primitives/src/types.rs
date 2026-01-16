//! Core type definitions

use alloy_primitives::{Address, B256, U256};
use k256::ecdsa::{RecoveryId, Signature as K256Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// Account address (20 bytes, Ethereum-compatible)
pub type AccountAddress = Address;

/// Transaction hash
pub type TxHash = B256;

/// Block height
pub type BlockHeight = u64;

/// Timestamp in milliseconds since Unix epoch
pub type Timestamp = u64;

/// Nonce for replay protection
pub type Nonce = u64;

/// Raw balance/amount in smallest unit (6 decimals for USDC)
pub type RawAmount = u128;

/// Signed amount (for PnL, balance changes)
pub type SignedAmount = i128;

/// Price in smallest unit (8 decimals typically)
pub type RawPrice = u128;

/// Signature components
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub r: B256,
    pub s: B256,
    pub v: u8,
}

impl Signature {
    /// Create a new signature from components
    pub fn new(r: B256, s: B256, v: u8) -> Self {
        Self { r, s, v }
    }

    /// Create a zero/null signature (for testing)
    pub fn zero() -> Self {
        Self {
            r: B256::ZERO,
            s: B256::ZERO,
            v: 27,
        }
    }

    /// Convert to bytes (65 bytes: r || s || v)
    pub fn to_bytes(&self) -> [u8; 65] {
        let mut bytes = [0u8; 65];
        bytes[0..32].copy_from_slice(self.r.as_slice());
        bytes[32..64].copy_from_slice(self.s.as_slice());
        bytes[64] = self.v;
        bytes
    }

    /// Parse from 65-byte array
    pub fn from_bytes(bytes: &[u8; 65]) -> Self {
        Self {
            r: B256::from_slice(&bytes[0..32]),
            s: B256::from_slice(&bytes[32..64]),
            v: bytes[64],
        }
    }

    /// Recover the signer address from a message hash
    ///
    /// This implements EIP-191 and EIP-712 signature recovery using ecrecover.
    /// The message_hash should be the Keccak256 hash of the signed data.
    pub fn recover(&self, message_hash: &[u8; 32]) -> Result<AccountAddress, SignatureError> {
        // Combine r and s into a single 64-byte signature
        let mut sig_bytes = [0u8; 64];
        sig_bytes[0..32].copy_from_slice(self.r.as_slice());
        sig_bytes[32..64].copy_from_slice(self.s.as_slice());

        // Parse the k256 signature
        let signature = K256Signature::from_slice(&sig_bytes)
            .map_err(|_| SignatureError::InvalidSignature)?;

        // Compute recovery ID from v
        // v = 27 or 28 in traditional Ethereum, or 0/1 in EIP-155
        let recovery_id = match self.v {
            0 | 27 => RecoveryId::new(false, false),
            1 | 28 => RecoveryId::new(true, false),
            v if v >= 35 => {
                // EIP-155: v = chain_id * 2 + 35 + recovery_id
                let is_odd = (v - 35) % 2 == 1;
                RecoveryId::new(is_odd, false)
            }
            _ => return Err(SignatureError::InvalidRecoveryId),
        };

        // Recover the verifying key (public key)
        let verifying_key = VerifyingKey::recover_from_prehash(message_hash, &signature, recovery_id)
            .map_err(|_| SignatureError::RecoveryFailed)?;

        // Convert public key to Ethereum address
        let public_key_bytes = verifying_key.to_encoded_point(false);
        let public_key_slice = &public_key_bytes.as_bytes()[1..]; // Skip the 0x04 prefix

        // Keccak256 hash of public key, take last 20 bytes
        let hash = Keccak256::digest(public_key_slice);
        let mut address_bytes = [0u8; 20];
        address_bytes.copy_from_slice(&hash[12..32]);

        Ok(Address::from(address_bytes))
    }

    /// Verify that this signature was created by the expected signer
    pub fn verify(&self, message_hash: &[u8; 32], expected_signer: &AccountAddress) -> bool {
        match self.recover(message_hash) {
            Ok(recovered) => &recovered == expected_signer,
            Err(_) => false,
        }
    }
}

/// Errors that can occur during signature operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum SignatureError {
    #[error("Invalid signature format")]
    InvalidSignature,
    #[error("Invalid recovery ID")]
    InvalidRecoveryId,
    #[error("Failed to recover public key")]
    RecoveryFailed,
}

/// Signed action wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedAction<T> {
    pub action: T,
    pub nonce: Nonce,
    pub signature: Signature,
}

/// Fill event from matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub market_id: u8,
    pub maker: AccountAddress,
    pub taker: AccountAddress,
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: RawPrice,
    pub size: RawAmount,
    pub maker_fee: SignedAmount,
    pub taker_fee: RawAmount,
    pub timestamp: Timestamp,
    pub is_taker_buy: bool,
}

/// Liquidation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Liquidation {
    pub market_id: u8,
    pub account: AccountAddress,
    pub liquidated_size: RawAmount,
    pub liquidation_price: RawPrice,
    pub bankruptcy_price: RawPrice,
    pub insurance_fund_delta: SignedAmount,
    pub timestamp: Timestamp,
}

/// Funding payment event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingPayment {
    pub market_id: u8,
    pub account: AccountAddress,
    pub payment: SignedAmount,
    pub position_size: SignedAmount,
    pub funding_rate: SignedAmount,
    pub timestamp: Timestamp,
}

/// Account state snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: SignedAmount,
    pub nonce: Nonce,
    pub last_timestamp_nonce: Nonce,
}

/// Margin summary for an account
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarginSummary {
    pub account_value: SignedAmount,
    pub total_position_value: RawAmount,
    pub total_initial_margin: RawAmount,
    pub total_maintenance_margin: RawAmount,
    pub free_collateral: SignedAmount,
    pub withdrawable: RawAmount,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_roundtrip() {
        let sig = Signature {
            r: B256::repeat_byte(0x11),
            s: B256::repeat_byte(0x22),
            v: 27,
        };
        let bytes = sig.to_bytes();
        let recovered = Signature::from_bytes(&bytes);
        assert_eq!(sig, recovered);
    }
}
