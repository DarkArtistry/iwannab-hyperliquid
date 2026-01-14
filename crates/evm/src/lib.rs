//! EVM crate - HyperEVM execution environment
//!
//! This crate implements the EVM execution environment with custom precompiles
//! for reading HyperCore state and queued writes via CoreWriter.
//!
//! ## Components
//!
//! - `executor` - EVM transaction executor with revm integration
//! - `precompiles` - HyperCore-specific precompiles for state reads
//! - `state` - EVM state management (accounts, storage, code)
//! - `core_writer` - CoreWriter action queue for MEV prevention
//! - `rpc` - Ethereum JSON-RPC server

pub mod executor;
pub mod precompiles;
pub mod state;
pub mod core_writer;
pub mod rpc;

pub use executor::{EvmExecutor, EvmTransaction, EvmExecutionResult, EvmLog, EvmExecutorError};
pub use precompiles::{HyperCorePrecompiles, PrecompileAddress};
pub use state::EvmState;
pub use core_writer::{CoreWriterAction, ActionQueue};
pub use rpc::EvmRpcServer;
