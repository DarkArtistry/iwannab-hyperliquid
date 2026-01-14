//! Gateway crate - HTTP API and WebSocket server
//!
//! This crate provides the API gateway for HyperCore, including:
//! - REST-like POST API for info and exchange endpoints
//! - WebSocket server for real-time updates

pub mod api;
pub mod handlers;
pub mod server;
pub mod websocket;

pub use api::{InfoRequest, ExchangeRequest, ApiResponse};
pub use server::{GatewayConfig, GatewayServer};
pub use websocket::{WsMessage, WsSubscription};
