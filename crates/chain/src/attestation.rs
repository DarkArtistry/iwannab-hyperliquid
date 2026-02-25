//! State Attestation Protocol (Option C from CONSENSUS.md)
//!
//! This module implements post-block state verification to detect divergence across validators.
//! After each block is committed, validators broadcast their computed app_hash, and the
//! attestation collector compares them to detect any state divergence.
//!
//! ## How It Works
//!
//! 1. After `commit()`, validator creates and signs a `StateAttestation`
//! 2. Attestation is broadcast to all other validators via P2P
//! 3. `AttestationCollector` receives and validates attestations
//! 4. If enough attestations are collected (quorum), check for divergence
//! 5. If divergence detected, `DivergenceHandler` takes action (halt, alert)
//!
//! ## Security Model
//!
//! - Attestations are signed with the validator's Ed25519 key
//! - Signature covers (height || app_hash || timestamp)
//! - Invalid signatures are rejected
//! - Only attestations from known validators are accepted

use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// State attestation broadcasted after each block commit
///
/// This message proves that a validator computed a specific app_hash at a given height.
/// It's used for post-block verification to detect state divergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateAttestation {
    /// Block height this attestation is for
    pub height: u64,

    /// The app_hash computed by this validator after executing the block
    pub app_hash: [u8; 32],

    /// Validator's public key (Ed25519, 32 bytes)
    pub validator_pubkey: [u8; 32],

    /// Signature over the attestation message (height || app_hash || timestamp)
    pub signature: [u8; 64],

    /// Unix timestamp (milliseconds) when the attestation was created
    /// Note: This is NOT consensus-critical - just for debugging/monitoring
    pub timestamp: u64,
}

/// Serializable version of StateAttestation for network transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateAttestationWire {
    height: u64,
    #[serde(with = "hex_array_32")]
    app_hash: [u8; 32],
    #[serde(with = "hex_array_32")]
    validator_pubkey: [u8; 32],
    #[serde(with = "hex_array_64")]
    signature: [u8; 64],
    timestamp: u64,
}

mod hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

mod hex_array_64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

impl StateAttestation {
    /// Create and sign a new attestation
    ///
    /// # Arguments
    /// * `height` - The block height
    /// * `app_hash` - The computed application state hash
    /// * `signing_key` - The validator's Ed25519 signing key
    ///
    /// # Returns
    /// A signed StateAttestation ready for broadcast
    pub fn new(height: u64, app_hash: [u8; 32], signing_key: &SigningKey) -> Self {
        let validator_pubkey = signing_key.verifying_key().to_bytes();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Construct message to sign: height || app_hash || timestamp
        let message = Self::build_signing_message(height, &app_hash, timestamp);

        // Sign the message
        let signature = signing_key.sign(&message);

        Self {
            height,
            app_hash,
            validator_pubkey,
            signature: signature.to_bytes(),
            timestamp,
        }
    }

    /// Verify the attestation signature
    ///
    /// # Returns
    /// `true` if the signature is valid, `false` otherwise
    pub fn verify(&self) -> bool {
        // Reconstruct the verifying key
        let verifying_key = match VerifyingKey::from_bytes(&self.validator_pubkey) {
            Ok(key) => key,
            Err(_) => return false,
        };

        // Reconstruct the signature (from_bytes doesn't return Result in ed25519-dalek 2.x)
        let signature = Signature::from_bytes(&self.signature);

        // Reconstruct the message
        let message = Self::build_signing_message(self.height, &self.app_hash, self.timestamp);

        // Verify
        verifying_key.verify(&message, &signature).is_ok()
    }

    /// Build the message to be signed
    ///
    /// Format: height (8 bytes LE) || app_hash (32 bytes) || timestamp (8 bytes LE)
    fn build_signing_message(height: u64, app_hash: &[u8; 32], timestamp: u64) -> Vec<u8> {
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&height.to_le_bytes());
        message.extend_from_slice(app_hash);
        message.extend_from_slice(&timestamp.to_le_bytes());
        message
    }

    /// Get the validator's public key as a hex string (for logging)
    pub fn validator_hex(&self) -> String {
        hex::encode(&self.validator_pubkey[..8])
    }

    /// Get the app_hash as a hex string (for logging)
    pub fn app_hash_hex(&self) -> String {
        hex::encode(&self.app_hash[..8])
    }

    /// Serialize to bytes for P2P transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        let wire = StateAttestationWire {
            height: self.height,
            app_hash: self.app_hash,
            validator_pubkey: self.validator_pubkey,
            signature: self.signature,
            timestamp: self.timestamp,
        };
        serde_json::to_vec(&wire).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let wire: StateAttestationWire = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            height: wire.height,
            app_hash: wire.app_hash,
            validator_pubkey: wire.validator_pubkey,
            signature: wire.signature,
            timestamp: wire.timestamp,
        })
    }
}

/// Validator key pair for attestation signing
///
/// This struct holds the Ed25519 key pair used by a validator to sign attestations.
/// It's separate from the CometBFT validator key which may use different key types.
#[derive(Clone)]
pub struct AttestationKeyPair {
    signing_key: SigningKey,
}

impl AttestationKeyPair {
    /// Generate a new random key pair
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        Self { signing_key }
    }

    /// Create from an existing secret key (32 bytes)
    pub fn from_secret_key(secret: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(secret);
        Self { signing_key }
    }

    /// Get the public key bytes
    pub fn public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Get the public key as a hex string
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key())
    }

    /// Sign an attestation
    pub fn sign_attestation(&self, height: u64, app_hash: [u8; 32]) -> StateAttestation {
        StateAttestation::new(height, app_hash, &self.signing_key)
    }

    /// Get the signing key (for internal use)
    #[allow(dead_code)]
    pub(crate) fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }
}

impl std::fmt::Debug for AttestationKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttestationKeyPair")
            .field("public_key", &self.public_key_hex())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attestation_creation_and_verification() {
        let key_pair = AttestationKeyPair::generate();
        let height = 100;
        let app_hash = [0x42u8; 32];

        let attestation = key_pair.sign_attestation(height, app_hash);

        assert_eq!(attestation.height, height);
        assert_eq!(attestation.app_hash, app_hash);
        assert_eq!(attestation.validator_pubkey, key_pair.public_key());
        assert!(attestation.verify(), "Valid attestation should verify");
    }

    #[test]
    fn test_attestation_invalid_signature() {
        let key_pair = AttestationKeyPair::generate();
        let mut attestation = key_pair.sign_attestation(100, [0x42u8; 32]);

        // Tamper with the app_hash
        attestation.app_hash[0] ^= 0xFF;

        assert!(!attestation.verify(), "Tampered attestation should not verify");
    }

    #[test]
    fn test_attestation_invalid_signature_bytes() {
        let key_pair = AttestationKeyPair::generate();
        let mut attestation = key_pair.sign_attestation(100, [0x42u8; 32]);

        // Tamper with the signature
        attestation.signature[0] ^= 0xFF;

        assert!(!attestation.verify(), "Invalid signature should not verify");
    }

    #[test]
    fn test_attestation_wrong_pubkey() {
        let key_pair1 = AttestationKeyPair::generate();
        let key_pair2 = AttestationKeyPair::generate();

        let mut attestation = key_pair1.sign_attestation(100, [0x42u8; 32]);

        // Replace with different validator's pubkey
        attestation.validator_pubkey = key_pair2.public_key();

        assert!(!attestation.verify(), "Wrong pubkey should not verify");
    }

    #[test]
    fn test_attestation_serialization() {
        let key_pair = AttestationKeyPair::generate();
        let attestation = key_pair.sign_attestation(100, [0x42u8; 32]);

        // Serialize and deserialize
        let bytes = attestation.to_bytes();
        let restored = StateAttestation::from_bytes(&bytes).expect("Should deserialize");

        assert_eq!(attestation, restored);
        assert!(restored.verify(), "Restored attestation should verify");
    }

    #[test]
    fn test_key_pair_from_secret() {
        let secret = [0x01u8; 32];
        let key_pair1 = AttestationKeyPair::from_secret_key(&secret);
        let key_pair2 = AttestationKeyPair::from_secret_key(&secret);

        assert_eq!(key_pair1.public_key(), key_pair2.public_key());
    }

    #[test]
    fn test_different_heights_different_attestations() {
        let key_pair = AttestationKeyPair::generate();
        let app_hash = [0x42u8; 32];

        let att1 = key_pair.sign_attestation(100, app_hash);
        let att2 = key_pair.sign_attestation(101, app_hash);

        assert_ne!(att1.signature, att2.signature);
        assert!(att1.verify());
        assert!(att2.verify());
    }

    #[test]
    fn test_different_hashes_different_attestations() {
        let key_pair = AttestationKeyPair::generate();

        let att1 = key_pair.sign_attestation(100, [0x42u8; 32]);
        let att2 = key_pair.sign_attestation(100, [0x43u8; 32]);

        assert_ne!(att1.signature, att2.signature);
        assert!(att1.verify());
        assert!(att2.verify());
    }

    #[test]
    fn test_attestation_message_format() {
        // Verify the message format is deterministic
        let msg1 = StateAttestation::build_signing_message(100, &[0x42u8; 32], 1000);
        let msg2 = StateAttestation::build_signing_message(100, &[0x42u8; 32], 1000);
        assert_eq!(msg1, msg2);

        // Length should be 8 + 32 + 8 = 48 bytes
        assert_eq!(msg1.len(), 48);
    }

    #[test]
    fn test_attestation_hex_helpers() {
        let key_pair = AttestationKeyPair::generate();
        let attestation = key_pair.sign_attestation(100, [0xABu8; 32]);

        let validator_hex = attestation.validator_hex();
        let app_hash_hex = attestation.app_hash_hex();

        // Should be 16 chars (8 bytes * 2)
        assert_eq!(validator_hex.len(), 16);
        assert_eq!(app_hash_hex.len(), 16);
        assert!(app_hash_hex.starts_with("abababab"));
    }
}
