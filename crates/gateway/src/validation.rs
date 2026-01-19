//! Input validation for the Gateway API
//!
//! Provides fail-fast validation at the gateway layer before requests
//! reach the engine. This improves:
//! - Security: Reject malformed inputs early
//! - Performance: Don't waste engine cycles on invalid requests
//! - User experience: Better error messages
//!
//! # Validation Layers
//!
//! ```text
//! Request ──► Gateway Validation ──► Engine Validation ──► Execution
//!             (this module)          (margin, tick size)
//! ```

use crate::api::{ExchangeAction, ExchangeRequest, InfoRequest, OrderWire, SpotOrderWire};
use hypercore_primitives::Decimal;

/// Validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum orders per request
    pub max_orders_per_request: usize,
    /// Maximum cancels per request
    pub max_cancels_per_request: usize,
    /// Maximum request body size in bytes
    pub max_request_body_size: usize,
    /// Maximum address length (with 0x prefix)
    pub max_address_length: usize,
    /// Maximum decimal string length
    pub max_decimal_length: usize,
    /// Maximum price value (sanity check)
    pub max_price: Decimal,
    /// Maximum size value (sanity check)
    pub max_size: Decimal,
    /// Minimum nonce value (0 is suspicious)
    pub min_nonce: u64,
    /// Enable validation
    pub enabled: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_orders_per_request: 100,
            max_cancels_per_request: 100,
            max_request_body_size: 1024 * 1024, // 1MB
            max_address_length: 42,              // 0x + 40 hex chars
            max_decimal_length: 50,              // Reasonable precision
            // Max price: 1 trillion (with 8 decimals) - covers any realistic asset price
            max_price: Decimal::from_raw(1_000_000_000_000_00000000i128, 8),
            // Max size: 1 billion (with 8 decimals) - covers any realistic order size
            max_size: Decimal::from_raw(1_000_000_000_00000000i128, 8),
            min_nonce: 1,
            enabled: true,
        }
    }
}

impl ValidationConfig {
    /// Disable validation (for testing)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Strict validation for production
    pub fn strict() -> Self {
        Self {
            max_orders_per_request: 50,
            max_cancels_per_request: 50,
            max_request_body_size: 512 * 1024, // 512KB
            ..Default::default()
        }
    }
}

/// Validation error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("Too many orders in request: {0} (max: {1})")]
    TooManyOrders(usize, usize),

    #[error("Too many cancels in request: {0} (max: {1})")]
    TooManyCancels(usize, usize),

    #[error("Request body too large: {0} bytes (max: {1})")]
    RequestTooLarge(usize, usize),

    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    #[error("Invalid price: {0}")]
    InvalidPrice(String),

    #[error("Invalid size: {0}")]
    InvalidSize(String),

    #[error("Invalid market ID: {0}")]
    InvalidMarketId(String),

    #[error("Invalid time-in-force: {0}")]
    InvalidTimeInForce(String),

    #[error("Invalid nonce: {0}")]
    InvalidNonce(String),

    #[error("Invalid order type: {0}")]
    InvalidOrderType(String),

    #[error("Invalid leverage: {0} (must be 1-{1})")]
    InvalidLeverage(u8, u8),

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Invalid grouping: {0}")]
    InvalidGrouping(String),

    #[error("Price must be positive")]
    PriceMustBePositive,

    #[error("Size must be positive")]
    SizeMustBePositive,

    #[error("Price exceeds maximum: {0}")]
    PriceExceedsMax(String),

    #[error("Size exceeds maximum: {0}")]
    SizeExceedsMax(String),

    #[error("Empty orders list")]
    EmptyOrdersList,

    #[error("Empty cancels list")]
    EmptyCancelsList,
}

/// Validator for gateway requests
pub struct Validator {
    config: ValidationConfig,
}

impl Validator {
    /// Create a new validator with the given config
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }

    /// Validate an exchange request
    pub fn validate_exchange_request(&self, request: &ExchangeRequest) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Validate nonce
        self.validate_nonce(request.nonce)?;

        // Validate action
        self.validate_exchange_action(&request.action)?;

        Ok(())
    }

    /// Validate an info request
    pub fn validate_info_request(&self, request: &InfoRequest) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        match request {
            InfoRequest::ClearinghouseState { user }
            | InfoRequest::OpenOrders { user }
            | InfoRequest::SpotBalances { user }
            | InfoRequest::UnifiedBalances { user } => {
                self.validate_address(user)?;
            }
            InfoRequest::SpotOpenOrders { user, .. } => {
                self.validate_address(user)?;
            }
            InfoRequest::UserFills { user, .. }
            | InfoRequest::UserFundingHistory { user, .. } => {
                self.validate_address(user)?;
            }
            InfoRequest::L2Book { coin, .. }
            | InfoRequest::SpotL2Book { coin, .. }
            | InfoRequest::RecentTrades { coin, .. }
            | InfoRequest::CandleSnapshot { coin, .. }
            | InfoRequest::FundingHistory { coin, .. } => {
                self.validate_market_name(coin)?;
            }
            // Meta requests don't need validation
            InfoRequest::Meta
            | InfoRequest::AllMids
            | InfoRequest::SpotMeta
            | InfoRequest::SpotAllMids
            | InfoRequest::SpotTokenInfo { .. }
            | InfoRequest::StateInfo => {}
            // State proof requires address validation
            InfoRequest::StateProof { user, .. } => {
                self.validate_address(user)?;
            }
        }

        Ok(())
    }

    /// Validate request body size
    pub fn validate_body_size(&self, size: usize) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        if size > self.config.max_request_body_size {
            return Err(ValidationError::RequestTooLarge(
                size,
                self.config.max_request_body_size,
            ));
        }

        Ok(())
    }

    /// Validate exchange action
    fn validate_exchange_action(&self, action: &ExchangeAction) -> Result<(), ValidationError> {
        match action {
            ExchangeAction::Order { orders, grouping } => {
                self.validate_orders_count(orders.len())?;
                self.validate_grouping(grouping)?;
                for order in orders {
                    self.validate_order_wire(order)?;
                }
            }
            ExchangeAction::Cancel { cancels } => {
                self.validate_cancels_count(cancels.len())?;
                if cancels.is_empty() {
                    return Err(ValidationError::EmptyCancelsList);
                }
            }
            ExchangeAction::CancelByCloid { cancels } => {
                self.validate_cancels_count(cancels.len())?;
                if cancels.is_empty() {
                    return Err(ValidationError::EmptyCancelsList);
                }
            }
            ExchangeAction::CancelAll => {
                // No validation needed
            }
            ExchangeAction::UpdateLeverage { leverage, .. } => {
                self.validate_leverage(*leverage)?;
            }
            ExchangeAction::UsdTransfer { destination, amount } => {
                self.validate_address(destination)?;
                self.validate_amount(amount)?;
            }
            ExchangeAction::Withdraw { destination, amount } => {
                self.validate_address(destination)?;
                self.validate_amount(amount)?;
            }
            ExchangeAction::SpotOrder { orders, grouping } => {
                self.validate_orders_count(orders.len())?;
                self.validate_grouping(grouping)?;
                for order in orders {
                    self.validate_spot_order_wire(order)?;
                }
            }
            ExchangeAction::SpotCancel { cancels } => {
                self.validate_cancels_count(cancels.len())?;
                if cancels.is_empty() {
                    return Err(ValidationError::EmptyCancelsList);
                }
            }
            ExchangeAction::SpotCancelAll { .. } => {
                // No validation needed
            }
            ExchangeAction::ViewTransfer { amount, .. } => {
                self.validate_amount(amount)?;
            }
        }

        Ok(())
    }

    /// Validate order wire
    fn validate_order_wire(&self, order: &OrderWire) -> Result<(), ValidationError> {
        // Validate price
        self.validate_price(&order.p)?;

        // Validate size
        self.validate_size(&order.s)?;

        // Validate order type (TIF)
        self.validate_order_type(&order.t)?;

        // Validate client order ID if present
        if let Some(ref cloid) = order.c {
            self.validate_client_order_id(cloid)?;
        }

        Ok(())
    }

    /// Validate spot order wire
    fn validate_spot_order_wire(&self, order: &SpotOrderWire) -> Result<(), ValidationError> {
        // Validate price
        self.validate_price(&order.p)?;

        // Validate size
        self.validate_size(&order.s)?;

        // Validate order type (TIF)
        self.validate_order_type(&order.t)?;

        // Validate client order ID if present
        if let Some(ref cloid) = order.c {
            self.validate_client_order_id(cloid)?;
        }

        Ok(())
    }

    /// Validate price string
    fn validate_price(&self, price: &str) -> Result<(), ValidationError> {
        // Check length
        if price.len() > self.config.max_decimal_length {
            return Err(ValidationError::InvalidPrice(format!(
                "too long: {} chars",
                price.len()
            )));
        }

        // Parse decimal
        let decimal = Decimal::from_str(price)
            .map_err(|_| ValidationError::InvalidPrice(price.to_string()))?;

        // Check positive
        if decimal <= Decimal::ZERO {
            return Err(ValidationError::PriceMustBePositive);
        }

        // Check max
        if decimal > self.config.max_price {
            return Err(ValidationError::PriceExceedsMax(price.to_string()));
        }

        Ok(())
    }

    /// Validate size string
    fn validate_size(&self, size: &str) -> Result<(), ValidationError> {
        // Check length
        if size.len() > self.config.max_decimal_length {
            return Err(ValidationError::InvalidSize(format!(
                "too long: {} chars",
                size.len()
            )));
        }

        // Parse decimal
        let decimal = Decimal::from_str(size)
            .map_err(|_| ValidationError::InvalidSize(size.to_string()))?;

        // Check positive
        if decimal <= Decimal::ZERO {
            return Err(ValidationError::SizeMustBePositive);
        }

        // Check max
        if decimal > self.config.max_size {
            return Err(ValidationError::SizeExceedsMax(size.to_string()));
        }

        Ok(())
    }

    /// Validate amount string (for transfers)
    fn validate_amount(&self, amount: &str) -> Result<(), ValidationError> {
        // Check length
        if amount.len() > self.config.max_decimal_length {
            return Err(ValidationError::InvalidAmount(format!(
                "too long: {} chars",
                amount.len()
            )));
        }

        // Parse decimal
        let decimal = Decimal::from_str(amount)
            .map_err(|_| ValidationError::InvalidAmount(amount.to_string()))?;

        // Check positive
        if decimal <= Decimal::ZERO {
            return Err(ValidationError::InvalidAmount(
                "amount must be positive".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate address format
    pub fn validate_address(&self, address: &str) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check length
        if address.len() > self.config.max_address_length {
            return Err(ValidationError::InvalidAddress(format!(
                "too long: {} chars",
                address.len()
            )));
        }

        // Must start with 0x
        let hex_part = if let Some(stripped) = address.strip_prefix("0x") {
            stripped
        } else if let Some(stripped) = address.strip_prefix("0X") {
            stripped
        } else {
            return Err(ValidationError::InvalidAddress(
                "must start with 0x".to_string(),
            ));
        };

        // Must be exactly 40 hex characters
        if hex_part.len() != 40 {
            return Err(ValidationError::InvalidAddress(format!(
                "expected 40 hex chars, got {}",
                hex_part.len()
            )));
        }

        // Must be valid hex
        if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ValidationError::InvalidAddress(
                "contains invalid hex characters".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate nonce
    pub fn validate_nonce(&self, nonce: u64) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        if nonce < self.config.min_nonce {
            return Err(ValidationError::InvalidNonce(format!(
                "nonce {} is below minimum {}",
                nonce, self.config.min_nonce
            )));
        }

        Ok(())
    }

    /// Validate leverage
    fn validate_leverage(&self, leverage: u8) -> Result<(), ValidationError> {
        const MAX_LEVERAGE: u8 = 50;
        if leverage < 1 || leverage > MAX_LEVERAGE {
            return Err(ValidationError::InvalidLeverage(leverage, MAX_LEVERAGE));
        }

        Ok(())
    }

    /// Validate order type (TIF)
    fn validate_order_type(&self, order_type: &crate::api::OrderTypeWire) -> Result<(), ValidationError> {
        match order_type {
            crate::api::OrderTypeWire::Limit { limit } => {
                // Valid TIF values
                let valid_tifs = ["Gtc", "Ioc", "Alo", "Fok", "gtc", "ioc", "alo", "fok"];
                if !valid_tifs.contains(&limit.tif.as_str()) {
                    return Err(ValidationError::InvalidTimeInForce(limit.tif.clone()));
                }
            }
            crate::api::OrderTypeWire::Trigger { trigger } => {
                // Validate trigger price
                self.validate_price(&trigger.trigger_px)?;

                // Valid TPSL values
                let valid_tpsl = ["tp", "sl", "TP", "SL"];
                if !valid_tpsl.contains(&trigger.tpsl.as_str()) {
                    return Err(ValidationError::InvalidOrderType(format!(
                        "invalid tpsl: {}",
                        trigger.tpsl
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate client order ID
    fn validate_client_order_id(&self, cloid: &str) -> Result<(), ValidationError> {
        // Max length 64 characters
        if cloid.len() > 64 {
            return Err(ValidationError::InvalidOrderType(format!(
                "client order ID too long: {} chars (max 64)",
                cloid.len()
            )));
        }

        // Must be alphanumeric with dashes/underscores
        if !cloid.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(ValidationError::InvalidOrderType(
                "client order ID contains invalid characters".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate market name
    fn validate_market_name(&self, name: &str) -> Result<(), ValidationError> {
        // Max length 32 characters
        if name.len() > 32 {
            return Err(ValidationError::InvalidMarketId(format!(
                "market name too long: {} chars",
                name.len()
            )));
        }

        // Must not be empty
        if name.is_empty() {
            return Err(ValidationError::InvalidMarketId(
                "market name cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate grouping
    fn validate_grouping(&self, grouping: &str) -> Result<(), ValidationError> {
        let valid_groupings = ["na", "Na", "NA", "normalTpsl", "positionTpsl"];
        if !valid_groupings.contains(&grouping) {
            return Err(ValidationError::InvalidGrouping(grouping.to_string()));
        }

        Ok(())
    }

    /// Validate orders count
    fn validate_orders_count(&self, count: usize) -> Result<(), ValidationError> {
        if count == 0 {
            return Err(ValidationError::EmptyOrdersList);
        }

        if count > self.config.max_orders_per_request {
            return Err(ValidationError::TooManyOrders(
                count,
                self.config.max_orders_per_request,
            ));
        }

        Ok(())
    }

    /// Validate cancels count
    fn validate_cancels_count(&self, count: usize) -> Result<(), ValidationError> {
        if count > self.config.max_cancels_per_request {
            return Err(ValidationError::TooManyCancels(
                count,
                self.config.max_cancels_per_request,
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{LimitTif, OrderTypeWire, SignatureWire};

    fn make_validator() -> Validator {
        Validator::new(ValidationConfig::default())
    }

    #[test]
    fn test_valid_address() {
        let validator = make_validator();
        assert!(validator.validate_address("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").is_ok());
        assert!(validator.validate_address("0x0000000000000000000000000000000000000000").is_ok());
    }

    #[test]
    fn test_invalid_address_no_prefix() {
        let validator = make_validator();
        let result = validator.validate_address("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert!(matches!(result, Err(ValidationError::InvalidAddress(_))));
    }

    #[test]
    fn test_invalid_address_wrong_length() {
        let validator = make_validator();
        let result = validator.validate_address("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb922"); // 39 chars
        assert!(matches!(result, Err(ValidationError::InvalidAddress(_))));
    }

    #[test]
    fn test_invalid_address_bad_hex() {
        let validator = make_validator();
        let result = validator.validate_address("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb9226G"); // G is invalid
        assert!(matches!(result, Err(ValidationError::InvalidAddress(_))));
    }

    #[test]
    fn test_valid_price() {
        let validator = make_validator();
        assert!(validator.validate_price("65000.5").is_ok());
        assert!(validator.validate_price("0.001").is_ok());
        assert!(validator.validate_price("100000000").is_ok());
    }

    #[test]
    fn test_invalid_price_negative() {
        let validator = make_validator();
        let result = validator.validate_price("-100");
        assert!(matches!(result, Err(ValidationError::PriceMustBePositive)));
    }

    #[test]
    fn test_invalid_price_zero() {
        let validator = make_validator();
        let result = validator.validate_price("0");
        assert!(matches!(result, Err(ValidationError::PriceMustBePositive)));
    }

    #[test]
    fn test_invalid_price_nan() {
        let validator = make_validator();
        let result = validator.validate_price("NaN");
        assert!(matches!(result, Err(ValidationError::InvalidPrice(_))));
    }

    #[test]
    fn test_valid_size() {
        let validator = make_validator();
        assert!(validator.validate_size("0.1").is_ok());
        assert!(validator.validate_size("100").is_ok());
        assert!(validator.validate_size("0.00001").is_ok());
    }

    #[test]
    fn test_invalid_size_zero() {
        let validator = make_validator();
        let result = validator.validate_size("0");
        assert!(matches!(result, Err(ValidationError::SizeMustBePositive)));
    }

    #[test]
    fn test_valid_nonce() {
        let validator = make_validator();
        assert!(validator.validate_nonce(1).is_ok());
        assert!(validator.validate_nonce(1234567890).is_ok());
    }

    #[test]
    fn test_invalid_nonce_zero() {
        let validator = make_validator();
        let result = validator.validate_nonce(0);
        assert!(matches!(result, Err(ValidationError::InvalidNonce(_))));
    }

    #[test]
    fn test_valid_leverage() {
        let validator = make_validator();
        assert!(validator.validate_leverage(1).is_ok());
        assert!(validator.validate_leverage(10).is_ok());
        assert!(validator.validate_leverage(50).is_ok());
    }

    #[test]
    fn test_invalid_leverage() {
        let validator = make_validator();
        assert!(matches!(validator.validate_leverage(0), Err(ValidationError::InvalidLeverage(0, 50))));
        assert!(matches!(validator.validate_leverage(51), Err(ValidationError::InvalidLeverage(51, 50))));
    }

    #[test]
    fn test_valid_tif() {
        let validator = make_validator();
        let gtc = OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } };
        let ioc = OrderTypeWire::Limit { limit: LimitTif { tif: "Ioc".to_string() } };
        let alo = OrderTypeWire::Limit { limit: LimitTif { tif: "Alo".to_string() } };
        let fok = OrderTypeWire::Limit { limit: LimitTif { tif: "Fok".to_string() } };

        assert!(validator.validate_order_type(&gtc).is_ok());
        assert!(validator.validate_order_type(&ioc).is_ok());
        assert!(validator.validate_order_type(&alo).is_ok());
        assert!(validator.validate_order_type(&fok).is_ok());
    }

    #[test]
    fn test_invalid_tif() {
        let validator = make_validator();
        let invalid = OrderTypeWire::Limit { limit: LimitTif { tif: "invalid".to_string() } };
        assert!(matches!(validator.validate_order_type(&invalid), Err(ValidationError::InvalidTimeInForce(_))));
    }

    #[test]
    fn test_valid_grouping() {
        let validator = make_validator();
        assert!(validator.validate_grouping("na").is_ok());
        assert!(validator.validate_grouping("normalTpsl").is_ok());
        assert!(validator.validate_grouping("positionTpsl").is_ok());
    }

    #[test]
    fn test_invalid_grouping() {
        let validator = make_validator();
        let result = validator.validate_grouping("invalid");
        assert!(matches!(result, Err(ValidationError::InvalidGrouping(_))));
    }

    #[test]
    fn test_orders_count_limit() {
        let validator = make_validator();
        assert!(validator.validate_orders_count(1).is_ok());
        assert!(validator.validate_orders_count(100).is_ok());
        assert!(matches!(validator.validate_orders_count(0), Err(ValidationError::EmptyOrdersList)));
        assert!(matches!(validator.validate_orders_count(101), Err(ValidationError::TooManyOrders(101, 100))));
    }

    #[test]
    fn test_valid_client_order_id() {
        let validator = make_validator();
        assert!(validator.validate_client_order_id("order-123").is_ok());
        assert!(validator.validate_client_order_id("my_order_456").is_ok());
        assert!(validator.validate_client_order_id("abc123").is_ok());
    }

    #[test]
    fn test_invalid_client_order_id() {
        let validator = make_validator();
        // Too long
        let long_id = "a".repeat(65);
        assert!(validator.validate_client_order_id(&long_id).is_err());

        // Invalid characters
        assert!(validator.validate_client_order_id("order@123").is_err());
        assert!(validator.validate_client_order_id("order 123").is_err());
    }

    #[test]
    fn test_valid_order_wire() {
        let validator = make_validator();
        let order = OrderWire {
            a: 0,
            b: true,
            p: "65000.0".to_string(),
            s: "0.1".to_string(),
            r: false,
            t: OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } },
            c: Some("my-order-123".to_string()),
        };
        assert!(validator.validate_order_wire(&order).is_ok());
    }

    #[test]
    fn test_invalid_order_wire_bad_price() {
        let validator = make_validator();
        let order = OrderWire {
            a: 0,
            b: true,
            p: "-100".to_string(),
            s: "0.1".to_string(),
            r: false,
            t: OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } },
            c: None,
        };
        assert!(matches!(validator.validate_order_wire(&order), Err(ValidationError::PriceMustBePositive)));
    }

    #[test]
    fn test_invalid_order_wire_bad_size() {
        let validator = make_validator();
        let order = OrderWire {
            a: 0,
            b: true,
            p: "65000.0".to_string(),
            s: "0".to_string(),
            r: false,
            t: OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } },
            c: None,
        };
        assert!(matches!(validator.validate_order_wire(&order), Err(ValidationError::SizeMustBePositive)));
    }

    #[test]
    fn test_body_size_limit() {
        let validator = make_validator();
        assert!(validator.validate_body_size(1000).is_ok());
        assert!(validator.validate_body_size(1024 * 1024).is_ok()); // Exactly max
        assert!(matches!(
            validator.validate_body_size(1024 * 1024 + 1),
            Err(ValidationError::RequestTooLarge(_, _))
        ));
    }

    #[test]
    fn test_disabled_validation() {
        let validator = Validator::new(ValidationConfig::disabled());
        // Should pass even with invalid data when disabled
        assert!(validator.validate_address("invalid").is_ok());
        assert!(validator.validate_nonce(0).is_ok());
        assert!(validator.validate_body_size(usize::MAX).is_ok());
    }

    #[test]
    fn test_strict_config() {
        let config = ValidationConfig::strict();
        assert_eq!(config.max_orders_per_request, 50);
        assert_eq!(config.max_request_body_size, 512 * 1024);
    }

    #[test]
    fn test_validate_exchange_request_order() {
        let validator = make_validator();
        let request = ExchangeRequest {
            action: ExchangeAction::Order {
                orders: vec![OrderWire {
                    a: 0,
                    b: true,
                    p: "65000.0".to_string(),
                    s: "0.1".to_string(),
                    r: false,
                    t: OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } },
                    c: None,
                }],
                grouping: "na".to_string(),
            },
            nonce: 1234567890,
            signature: SignatureWire {
                r: "0x123".to_string(),
                s: "0x456".to_string(),
                v: 27,
            },
        };
        assert!(validator.validate_exchange_request(&request).is_ok());
    }

    #[test]
    fn test_validate_exchange_request_too_many_orders() {
        let validator = make_validator();
        let orders: Vec<OrderWire> = (0..101).map(|_| OrderWire {
            a: 0,
            b: true,
            p: "65000.0".to_string(),
            s: "0.1".to_string(),
            r: false,
            t: OrderTypeWire::Limit { limit: LimitTif { tif: "Gtc".to_string() } },
            c: None,
        }).collect();

        let request = ExchangeRequest {
            action: ExchangeAction::Order {
                orders,
                grouping: "na".to_string(),
            },
            nonce: 1234567890,
            signature: SignatureWire {
                r: "0x123".to_string(),
                s: "0x456".to_string(),
                v: 27,
            },
        };
        assert!(matches!(
            validator.validate_exchange_request(&request),
            Err(ValidationError::TooManyOrders(101, 100))
        ));
    }

    #[test]
    fn test_validate_info_request() {
        let validator = make_validator();

        // Valid requests
        assert!(validator.validate_info_request(&InfoRequest::Meta).is_ok());
        assert!(validator.validate_info_request(&InfoRequest::AllMids).is_ok());
        assert!(validator.validate_info_request(&InfoRequest::ClearinghouseState {
            user: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_string(),
        }).is_ok());

        // Invalid address
        assert!(validator.validate_info_request(&InfoRequest::ClearinghouseState {
            user: "invalid".to_string(),
        }).is_err());
    }
}
