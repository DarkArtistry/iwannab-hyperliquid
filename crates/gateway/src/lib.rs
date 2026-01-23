//! Gateway crate - HTTP API and WebSocket server
//!
//! This crate provides the API gateway for HyperCore, including:
//! - REST-like POST API for info and exchange endpoints
//! - WebSocket server for real-time updates
//! - EIP-712 typed data signing and verification
//! - Rate limiting middleware for DoS protection
//! - Input validation for fail-fast error handling
//! - Event broadcasting for real-time WebSocket updates

pub mod api;
pub mod eip712;
pub mod event_broadcaster;
pub mod handlers;
pub mod rate_limit;
pub mod server;
pub mod validation;
pub mod websocket;

pub use api::{InfoRequest, ExchangeRequest, ApiResponse};
pub use event_broadcaster::{EventBroadcaster, SharedEventBroadcaster};
pub use rate_limit::{RateLimiter, RateLimitConfig};
pub use server::{GatewayConfig, GatewayServer};
pub use validation::{ValidationConfig, ValidationError, Validator};
pub use websocket::{WsManager, WsMessage, WsSubscription};
