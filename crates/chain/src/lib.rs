//! Chain crate - ABCI application for CometBFT integration
//!
//! This crate implements the Application BlockChain Interface (ABCI) that connects
//! the HyperCore engine to CometBFT for Byzantine Fault Tolerant consensus.
//!
//! ## Phase 2B: Unified State Integration
//!
//! The chain crate now integrates with the unified state model:
//! - `AppState` holds references to `SharedUnifiedState`, `SharedEngineState`, and `SharedSpotEngineState`
//! - `HyperCoreApp` implements transaction handlers connected to these shared components
//! - Both perpetual and spot trading go through the same app state
//!
//! ## CometBFT Integration (Phase 2B Complete)
//!
//! When the `cometbft` feature is enabled, this crate provides full CometBFT integration:
//! - `CometBftApp` - Implements the tendermint_abci::Application trait
//! - `CometBftServer` - TCP-based ABCI server for CometBFT to connect to
//! - `ValidatorSet` - Validator set management for consensus
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │    CometBFT     │
//! │   (optional)    │
//! └────────┬────────┘
//!          │ ABCI Protocol
//!          ▼
//! ┌─────────────────┐     ┌─────────────────┐
//! │  CometBftApp    │ OR  │  BlockProducer  │
//! │ (multi-node)    │     │ (single-node)   │
//! └────────┬────────┘     └────────┬────────┘
//!          │                       │
//!          └──────────┬────────────┘
//!                     ▼
//!              ┌──────────────┐
//!              │ HyperCoreApp │
//!              │  execute_tx  │
//!              └──────┬───────┘
//!                     │
//!       ┌─────────────┼─────────────┐
//!       │             │             │
//!       ▼             ▼             ▼
//! ┌───────────┐ ┌───────────┐ ┌───────────┐
//! │EngineState│ │SpotEngine │ │UnifiedState│
//! │  (Perps)  │ │  (HIP-1)  │ │ (Balances) │
//! └───────────┘ └───────────┘ └───────────┘
//! ```

pub mod abci;
pub mod app;
pub mod block_producer;
pub mod mempool;
pub mod merkle;
pub mod state;
pub mod tx;

// CometBFT integration (optional feature)
#[cfg(feature = "cometbft")]
pub mod cometbft;

// Persistence integration (optional feature)
#[cfg(feature = "persistence")]
pub mod persistence_integration;

pub use abci::AbciService;
pub use app::HyperCoreApp;
pub use block_producer::{BlockProducer, BlockProducerConfig, BlockResult, PostCommitHandler};
pub use mempool::{Mempool, SharedMempool};
pub use merkle::{MerkleProof, MerkleTree, verify_proof, compute_leaf_hash, hash_entry};
pub use state::{AppState, SharedEngine, SharedEngineState, SharedSpotEngine};
pub use tx::{Transaction, TransactionType};

// Re-export CometBFT types when feature is enabled
#[cfg(feature = "cometbft")]
pub use cometbft::{CometBftApp, CometBftServer, Validator, ValidatorSet};

// Re-export persistence integration types when feature is enabled
#[cfg(feature = "persistence")]
pub use persistence_integration::{extract_state, restore_state, restore_chain_state, RestoreError};
