//! Rate limiting middleware for the Gateway API
//!
//! Provides protection against DoS attacks by limiting request rates:
//! - Per-IP rate limiting (all requests from an IP)
//! - Per-endpoint rate limiting (stricter for write operations)
//!
//! # Example
//!
//! ```ignore
//! use hypercore_gateway::rate_limit::{RateLimiter, RateLimitConfig};
//!
//! let config = RateLimitConfig::default();
//! let limiter = RateLimiter::new(config);
//!
//! // Use with Axum router
//! let app = Router::new()
//!     .route("/exchange", post(handler))
//!     .layer(limiter.layer());
//! ```

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, Response, StatusCode},
};
use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovernorRateLimiter,
};
use tower::{Layer, Service};

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per minute per IP (global)
    pub requests_per_minute: u32,
    /// Maximum exchange (write) requests per minute per IP
    pub exchange_requests_per_minute: u32,
    /// Maximum info (read) requests per minute per IP
    pub info_requests_per_minute: u32,
    /// Enable rate limiting
    pub enabled: bool,
    /// Burst size multiplier (allows short bursts above the rate)
    pub burst_multiplier: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            // 100 requests per minute global
            requests_per_minute: 100,
            // 50 write requests per minute (stricter for orders/cancels)
            exchange_requests_per_minute: 50,
            // 200 read requests per minute (more permissive for info queries)
            info_requests_per_minute: 200,
            // Enabled by default
            enabled: true,
            // Allow 2x burst
            burst_multiplier: 2,
        }
    }
}

impl RateLimitConfig {
    /// Create a new config with custom settings
    pub fn new(requests_per_minute: u32, exchange_requests_per_minute: u32) -> Self {
        Self {
            requests_per_minute,
            exchange_requests_per_minute,
            ..Default::default()
        }
    }

    /// Disable rate limiting (for testing)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Development mode with relaxed limits
    pub fn development() -> Self {
        Self {
            requests_per_minute: 1000,
            exchange_requests_per_minute: 500,
            info_requests_per_minute: 2000,
            enabled: true,
            burst_multiplier: 5,
        }
    }

    /// Production mode with strict limits
    pub fn production() -> Self {
        Self {
            requests_per_minute: 60,
            exchange_requests_per_minute: 30,
            info_requests_per_minute: 120,
            enabled: true,
            burst_multiplier: 2,
        }
    }
}

/// Per-IP rate limiter using governor
type IpRateLimiter = GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Rate limiter state
pub struct RateLimiterState {
    config: RateLimitConfig,
    /// Global rate limiters per IP
    global_limiters: DashMap<IpAddr, Arc<IpRateLimiter>>,
    /// Exchange endpoint rate limiters per IP
    exchange_limiters: DashMap<IpAddr, Arc<IpRateLimiter>>,
    /// Info endpoint rate limiters per IP
    info_limiters: DashMap<IpAddr, Arc<IpRateLimiter>>,
}

impl RateLimiterState {
    /// Create new rate limiter state
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            global_limiters: DashMap::new(),
            exchange_limiters: DashMap::new(),
            info_limiters: DashMap::new(),
        }
    }

    /// Get or create a rate limiter for the given IP
    fn get_or_create_limiter(
        map: &DashMap<IpAddr, Arc<IpRateLimiter>>,
        ip: IpAddr,
        requests_per_minute: u32,
        burst_multiplier: u32,
    ) -> Arc<IpRateLimiter> {
        map.entry(ip)
            .or_insert_with(|| {
                let burst = NonZeroU32::new(requests_per_minute * burst_multiplier)
                    .unwrap_or(NonZeroU32::new(1).unwrap());
                let quota = Quota::per_minute(burst)
                    .allow_burst(burst);
                Arc::new(GovernorRateLimiter::direct(quota))
            })
            .clone()
    }

    /// Check if request should be allowed (global limit)
    pub fn check_global(&self, ip: IpAddr) -> bool {
        if !self.config.enabled {
            return true;
        }

        let limiter = Self::get_or_create_limiter(
            &self.global_limiters,
            ip,
            self.config.requests_per_minute,
            self.config.burst_multiplier,
        );
        limiter.check().is_ok()
    }

    /// Check if exchange request should be allowed
    pub fn check_exchange(&self, ip: IpAddr) -> bool {
        if !self.config.enabled {
            return true;
        }

        let limiter = Self::get_or_create_limiter(
            &self.exchange_limiters,
            ip,
            self.config.exchange_requests_per_minute,
            self.config.burst_multiplier,
        );
        limiter.check().is_ok()
    }

    /// Check if info request should be allowed
    pub fn check_info(&self, ip: IpAddr) -> bool {
        if !self.config.enabled {
            return true;
        }

        let limiter = Self::get_or_create_limiter(
            &self.info_limiters,
            ip,
            self.config.info_requests_per_minute,
            self.config.burst_multiplier,
        );
        limiter.check().is_ok()
    }

    /// Clean up expired limiters (call periodically)
    pub fn cleanup(&self) {
        // Remove limiters that haven't been used recently
        // This is a simple implementation; for production, consider
        // using TTL-based eviction
        const MAX_ENTRIES: usize = 10000;

        if self.global_limiters.len() > MAX_ENTRIES {
            // Remove half the entries (simple eviction)
            let keys: Vec<_> = self.global_limiters.iter()
                .take(MAX_ENTRIES / 2)
                .map(|r| *r.key())
                .collect();
            for key in keys {
                self.global_limiters.remove(&key);
            }
        }

        if self.exchange_limiters.len() > MAX_ENTRIES {
            let keys: Vec<_> = self.exchange_limiters.iter()
                .take(MAX_ENTRIES / 2)
                .map(|r| *r.key())
                .collect();
            for key in keys {
                self.exchange_limiters.remove(&key);
            }
        }

        if self.info_limiters.len() > MAX_ENTRIES {
            let keys: Vec<_> = self.info_limiters.iter()
                .take(MAX_ENTRIES / 2)
                .map(|r| *r.key())
                .collect();
            for key in keys {
                self.info_limiters.remove(&key);
            }
        }
    }

    /// Get current config
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Get number of tracked IPs (for monitoring)
    pub fn tracked_ips(&self) -> usize {
        self.global_limiters.len()
    }
}

/// Rate limiter that can be used as a tower Layer
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<RateLimiterState>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given config
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            state: Arc::new(RateLimiterState::new(config)),
        }
    }

    /// Create a rate limiter layer for use with Axum
    pub fn layer(&self) -> RateLimitLayer {
        RateLimitLayer {
            state: Arc::clone(&self.state),
        }
    }

    /// Get the internal state for monitoring
    pub fn state(&self) -> Arc<RateLimiterState> {
        Arc::clone(&self.state)
    }

    /// Start a background task to clean up expired limiters
    pub fn start_cleanup_task(&self) {
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes
            loop {
                interval.tick().await;
                state.cleanup();
                tracing::debug!("Rate limiter cleanup: {} tracked IPs", state.tracked_ips());
            }
        });
    }
}

/// Tower Layer for rate limiting
#[derive(Clone)]
pub struct RateLimitLayer {
    state: Arc<RateLimiterState>,
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            state: Arc::clone(&self.state),
        }
    }
}

/// Tower Service for rate limiting
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    state: Arc<RateLimiterState>,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let state = Arc::clone(&self.state);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract client IP
            let ip = extract_client_ip(&req);

            // Check global rate limit first
            if !state.check_global(ip) {
                tracing::warn!("Rate limit exceeded for IP: {}", ip);
                return Ok(rate_limit_response());
            }

            // Check endpoint-specific rate limits
            let path = req.uri().path();
            let allowed = if path == "/exchange" {
                state.check_exchange(ip)
            } else if path == "/info" {
                state.check_info(ip)
            } else {
                true // Other endpoints only use global limit
            };

            if !allowed {
                tracing::warn!("Endpoint rate limit exceeded for IP: {} on {}", ip, path);
                return Ok(rate_limit_response());
            }

            // Request allowed, forward to inner service
            inner.call(req).await
        })
    }
}

/// Extract client IP from request
fn extract_client_ip(req: &Request<Body>) -> IpAddr {
    // Try to get from X-Forwarded-For header (for proxied requests)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = forwarded.to_str() {
            // Take the first IP in the list
            if let Some(ip_str) = s.split(',').next() {
                if let Ok(ip) = ip_str.trim().parse() {
                    return ip;
                }
            }
        }
    }

    // Try X-Real-IP header
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(s) = real_ip.to_str() {
            if let Ok(ip) = s.trim().parse() {
                return ip;
            }
        }
    }

    // Fall back to ConnectInfo if available (set by Axum)
    if let Some(connect_info) = req.extensions().get::<ConnectInfo<std::net::SocketAddr>>() {
        return connect_info.0.ip();
    }

    // Default to localhost if nothing else works
    "127.0.0.1".parse().unwrap()
}

/// Create a 429 Too Many Requests response
fn rate_limit_response() -> Response<Body> {
    let body = serde_json::json!({
        "error": "Too Many Requests",
        "message": "Rate limit exceeded. Please slow down your requests.",
        "retry_after_seconds": 60
    });

    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("Content-Type", "application/json")
        .header("Retry-After", "60")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_default_config() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_minute, 100);
        assert_eq!(config.exchange_requests_per_minute, 50);
        assert_eq!(config.info_requests_per_minute, 200);
        assert!(config.enabled);
    }

    #[test]
    fn test_disabled_config() {
        let config = RateLimitConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_development_config() {
        let config = RateLimitConfig::development();
        assert_eq!(config.requests_per_minute, 1000);
        assert!(config.enabled);
    }

    #[test]
    fn test_production_config() {
        let config = RateLimitConfig::production();
        assert_eq!(config.requests_per_minute, 60);
        assert!(config.enabled);
    }

    #[test]
    fn test_rate_limiter_allows_requests_when_disabled() {
        let config = RateLimitConfig::disabled();
        let state = RateLimiterState::new(config);
        let ip: IpAddr = Ipv4Addr::new(192, 168, 1, 1).into();

        // Should always allow when disabled
        for _ in 0..1000 {
            assert!(state.check_global(ip));
            assert!(state.check_exchange(ip));
            assert!(state.check_info(ip));
        }
    }

    #[test]
    fn test_rate_limiter_tracks_ips() {
        let config = RateLimitConfig::default();
        let state = RateLimiterState::new(config);

        let ip1: IpAddr = Ipv4Addr::new(192, 168, 1, 1).into();
        let ip2: IpAddr = Ipv4Addr::new(192, 168, 1, 2).into();
        let ip3: IpAddr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into();

        state.check_global(ip1);
        state.check_global(ip2);
        state.check_global(ip3);

        assert_eq!(state.tracked_ips(), 3);
    }

    #[test]
    fn test_rate_limiter_allows_burst() {
        let config = RateLimitConfig {
            requests_per_minute: 10,
            exchange_requests_per_minute: 5,
            info_requests_per_minute: 20,
            enabled: true,
            burst_multiplier: 2,
        };
        let state = RateLimiterState::new(config);
        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();

        // Should allow burst of requests (10 * 2 = 20 burst)
        let mut allowed = 0;
        for _ in 0..30 {
            if state.check_global(ip) {
                allowed += 1;
            }
        }

        // Should have allowed the burst amount
        assert!(allowed >= 10, "Expected at least 10 requests to be allowed, got {}", allowed);
        assert!(allowed <= 25, "Expected at most 25 requests to be allowed, got {}", allowed);
    }

    #[test]
    fn test_rate_limiter_creation() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let state = limiter.state();
        assert_eq!(state.tracked_ips(), 0);
    }

    #[test]
    fn test_cleanup() {
        let config = RateLimitConfig::default();
        let state = RateLimiterState::new(config);

        // Add many IPs
        for i in 0..100u8 {
            let ip: IpAddr = Ipv4Addr::new(192, 168, 1, i).into();
            state.check_global(ip);
        }

        assert_eq!(state.tracked_ips(), 100);

        // Cleanup should run without error
        state.cleanup();
    }

    #[test]
    fn test_different_endpoints_have_different_limits() {
        let config = RateLimitConfig {
            requests_per_minute: 100,
            exchange_requests_per_minute: 5,  // Very strict
            info_requests_per_minute: 100,
            enabled: true,
            burst_multiplier: 1,
        };
        let state = RateLimiterState::new(config);
        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();

        // Exchange should be stricter
        let mut exchange_allowed = 0;
        for _ in 0..20 {
            if state.check_exchange(ip) {
                exchange_allowed += 1;
            }
        }

        // Info should be more permissive (different IP to reset)
        let ip2: IpAddr = Ipv4Addr::new(10, 0, 0, 2).into();
        let mut info_allowed = 0;
        for _ in 0..20 {
            if state.check_info(ip2) {
                info_allowed += 1;
            }
        }

        assert!(exchange_allowed < info_allowed,
            "Exchange ({}) should be stricter than info ({})",
            exchange_allowed, info_allowed);
    }

    #[test]
    fn test_rate_limit_response_format() {
        let response = rate_limit_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/json"
        );
        assert_eq!(
            response.headers().get("Retry-After").unwrap(),
            "60"
        );
    }
}
