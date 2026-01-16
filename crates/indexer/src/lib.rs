//! Indexer crate - Block ingestion and persistence
//!
//! This crate handles indexing blockchain data into a PostgreSQL database
//! for efficient querying by the API gateway.

pub mod candles;
pub mod db;
pub mod ingest;
pub mod models;
pub mod queries;

pub use candles::CandleAggregator;
pub use db::Database;
pub use ingest::Indexer;
