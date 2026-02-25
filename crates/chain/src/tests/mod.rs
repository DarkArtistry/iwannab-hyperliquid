//! Comprehensive Integration Tests for HyperCore Chain
//!
//! This module contains integration tests that verify:
//! - Multi-node consensus simulation
//! - Blockchain growth and state transitions
//! - P2P attestation gossip (simulated)
//! - Re-org and rollback handling
//! - Determinism across nodes
//! - Security properties

pub mod multi_node;
pub mod blockchain_growth;
pub mod consensus;
pub mod security;
pub mod stress;
pub mod multi_node_integration;
pub mod evm_integration;
pub mod chain_integration;
