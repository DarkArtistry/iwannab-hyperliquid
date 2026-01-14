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
//! ## Architecture
//!
//! ```text
//! Gateway API ──────────────┐
//!                           │
//!                           ▼
//!                    ┌──────────────┐
//!                    │ HyperCoreApp │
//!                    │  execute_tx  │
//!                    └──────┬───────┘
//!                           │
//!         ┌─────────────────┼─────────────────┐
//!         │                 │                 │
//!         ▼                 ▼                 ▼
//!  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
//!  │ EngineState │  │ SpotEngine  │  │ UnifiedState│
//!  │   (Perps)   │  │  (HIP-1)    │  │  (Balances) │
//!  └─────────────┘  └─────────────┘  └─────────────┘
//! ```

pub mod abci;
pub mod app;
pub mod block_producer;
pub mod mempool;
pub mod state;
pub mod tx;

pub use abci::AbciService;
pub use app::HyperCoreApp;
pub use block_producer::{BlockProducer, BlockProducerConfig, BlockResult};
pub use mempool::{Mempool, SharedMempool};
pub use state::{AppState, SharedEngineState, SharedSpotEngine};
pub use tx::{Transaction, TransactionType};
