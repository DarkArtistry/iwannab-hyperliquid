//! CometBFT Integration
//!
//! This module provides CometBFT/Tendermint consensus integration via the ABCI protocol.
//! It implements the `tendermint_abci::Application` trait to connect HyperCore to CometBFT.
//!
//! ## Architecture
//!
//! ```text
//! CometBFT Node                      HyperCore ABCI Server
//! ┌─────────────┐                    ┌─────────────────────┐
//! │             │   ABCI Protocol    │                     │
//! │  Consensus  │◄──────────────────►│   CometBftApp       │
//! │   Engine    │   (TCP/gRPC)       │        │            │
//! │             │                    │        ▼            │
//! └─────────────┘                    │   HyperCoreApp      │
//!                                    │   (AppState)        │
//!                                    └─────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! use hypercore_chain::cometbft::{CometBftApp, CometBftServer};
//!
//! // Create the application
//! let app = HyperCoreApp::new();
//! let cometbft_app = CometBftApp::new(app);
//!
//! // Start the ABCI server
//! let server = CometBftServer::new(cometbft_app);
//! server.start("0.0.0.0:26658").await?;
//! ```

#[cfg(feature = "cometbft")]
pub mod server;

#[cfg(feature = "cometbft")]
pub mod app;

#[cfg(feature = "cometbft")]
pub mod validators;

#[cfg(feature = "cometbft")]
pub use app::CometBftApp;

#[cfg(feature = "cometbft")]
pub use server::CometBftServer;

#[cfg(feature = "cometbft")]
pub use validators::{Validator, ValidatorSet};
