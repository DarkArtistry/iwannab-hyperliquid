//! Error types for HyperCore

use thiserror::Error;

/// Result type alias using HyperCore error
pub type Result<T> = std::result::Result<T, Error>;

/// HyperCore error codes (for API responses)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    // Authentication errors (1000-1099)
    InvalidSignature = 1001,
    InvalidNonce = 1002,

    // Margin errors (1100-1199)
    InsufficientMargin = 1003,
    InsufficientBalance = 1100,
    MarginWouldBeNegative = 1101,

    // Order errors (1200-1299)
    InvalidPrice = 1004,
    InvalidSize = 1005,
    OrderNotFound = 1006,
    MaxOrdersExceeded = 1009,
    PostOnlyWouldCross = 1012,
    ReduceOnlyViolation = 1011,
    FokNotFilled = 1200,
    InvalidOrderType = 1201,
    DuplicateClientOrderId = 1202,

    // Market errors (1300-1399)
    MarketNotFound = 1007,
    MarketPaused = 1013,
    MarketClosed = 1300,

    // Position errors (1400-1499)
    PositionNotFound = 1008,
    MaxPositionExceeded = 1010,
    InvalidLeverage = 1400,
    PositionWouldBeLiquidated = 1401,

    // Rate limiting (1500-1599)
    RateLimited = 1014,

    // Internal errors (1900-1999)
    InternalError = 1015,
    StateCorruption = 1900,
    SerializationError = 1901,
    DatabaseError = 1902,
}

impl ErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// Main error type for HyperCore operations
#[derive(Debug, Error)]
pub enum Error {
    // Authentication
    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid nonce: expected > {expected}, got {actual}")]
    InvalidNonce { expected: u64, actual: u64 },

    #[error("Nonce too old: {0} is more than 1 hour in the past")]
    NonceTooOld(u64),

    // Margin
    #[error("Insufficient margin: required {required}, available {available}")]
    InsufficientMargin { required: String, available: String },

    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: String, available: String },

    // Orders
    #[error("Invalid price: {price} is not a multiple of tick size {tick_size}")]
    InvalidPrice { price: String, tick_size: String },

    #[error("Invalid size: {size} is not a multiple of lot size {lot_size}")]
    InvalidSize { size: String, lot_size: String },

    #[error("Size too small: minimum is {min_size}")]
    SizeTooSmall { size: String, min_size: String },

    #[error("Size too large: maximum is {max_size}")]
    SizeTooLarge { size: String, max_size: String },

    #[error("Order not found: {0}")]
    OrderNotFound(u64),

    #[error("Order not owned by sender")]
    OrderNotOwned,

    #[error("Max open orders exceeded: limit is {limit}")]
    MaxOrdersExceeded { limit: u32 },

    #[error("Post-only order would cross the spread")]
    PostOnlyWouldCross,

    #[error("Reduce-only order would increase position")]
    ReduceOnlyViolation,

    #[error("Fill-or-kill order could not be fully filled")]
    FokNotFilled,

    #[error("Invalid order type for this operation")]
    InvalidOrderType,

    #[error("Duplicate client order ID: {0}")]
    DuplicateClientOrderId(String),

    // Markets
    #[error("Market not found: {0}")]
    MarketNotFound(u8),

    #[error("Market {0} is paused")]
    MarketPaused(String),

    #[error("Market {0} is closed")]
    MarketClosed(String),

    // Positions
    #[error("No position in market {0}")]
    PositionNotFound(u8),

    #[error("Position would exceed maximum: limit is {max_notional}")]
    MaxPositionExceeded { max_notional: String },

    #[error("Invalid leverage: {leverage}, must be between 1 and {max_leverage}")]
    InvalidLeverage { leverage: u8, max_leverage: u8 },

    #[error("Leverage change would cause liquidation")]
    LeverageWouldCauseLiquidation,

    // Liquidation
    #[error("Account is being liquidated")]
    AccountBeingLiquidated,

    #[error("Position is underwater and must be liquidated")]
    PositionUnderwater,

    // Rate limiting
    #[error("Rate limited: too many requests")]
    RateLimited,

    // Internal
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("State corruption detected: {0}")]
    StateCorruption(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Orderbook error: {0}")]
    Orderbook(String),
}

impl Error {
    /// Get the error code
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::InvalidSignature => ErrorCode::InvalidSignature,
            Error::InvalidNonce { .. } => ErrorCode::InvalidNonce,
            Error::NonceTooOld(_) => ErrorCode::InvalidNonce,
            Error::InsufficientMargin { .. } => ErrorCode::InsufficientMargin,
            Error::InsufficientBalance { .. } => ErrorCode::InsufficientBalance,
            Error::InvalidPrice { .. } => ErrorCode::InvalidPrice,
            Error::InvalidSize { .. } => ErrorCode::InvalidSize,
            Error::SizeTooSmall { .. } => ErrorCode::InvalidSize,
            Error::SizeTooLarge { .. } => ErrorCode::InvalidSize,
            Error::OrderNotFound(_) => ErrorCode::OrderNotFound,
            Error::OrderNotOwned => ErrorCode::OrderNotFound,
            Error::MaxOrdersExceeded { .. } => ErrorCode::MaxOrdersExceeded,
            Error::PostOnlyWouldCross => ErrorCode::PostOnlyWouldCross,
            Error::ReduceOnlyViolation => ErrorCode::ReduceOnlyViolation,
            Error::FokNotFilled => ErrorCode::FokNotFilled,
            Error::InvalidOrderType => ErrorCode::InvalidOrderType,
            Error::DuplicateClientOrderId(_) => ErrorCode::DuplicateClientOrderId,
            Error::MarketNotFound(_) => ErrorCode::MarketNotFound,
            Error::MarketPaused(_) => ErrorCode::MarketPaused,
            Error::MarketClosed(_) => ErrorCode::MarketClosed,
            Error::PositionNotFound(_) => ErrorCode::PositionNotFound,
            Error::MaxPositionExceeded { .. } => ErrorCode::MaxPositionExceeded,
            Error::InvalidLeverage { .. } => ErrorCode::InvalidLeverage,
            Error::LeverageWouldCauseLiquidation => ErrorCode::PositionWouldBeLiquidated,
            Error::AccountBeingLiquidated => ErrorCode::PositionWouldBeLiquidated,
            Error::PositionUnderwater => ErrorCode::PositionWouldBeLiquidated,
            Error::RateLimited => ErrorCode::RateLimited,
            Error::Internal(_) => ErrorCode::InternalError,
            Error::StateCorruption(_) => ErrorCode::StateCorruption,
            Error::Serialization(_) => ErrorCode::SerializationError,
            Error::Database(_) => ErrorCode::DatabaseError,
            Error::Orderbook(_) => ErrorCode::InternalError,
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::RateLimited | Error::Internal(_) | Error::Database(_)
        )
    }
}

/// API error response
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiError {
    pub status: String,
    pub code: u16,
    pub message: String,
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        Self {
            status: "err".to_string(),
            code: err.code().as_u16(),
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = Error::InsufficientMargin {
            required: "1000".to_string(),
            available: "500".to_string(),
        };
        assert_eq!(err.code(), ErrorCode::InsufficientMargin);
        assert_eq!(err.code().as_u16(), 1003);
    }

    #[test]
    fn test_api_error() {
        let err = Error::OrderNotFound(12345);
        let api_err: ApiError = err.into();
        assert_eq!(api_err.status, "err");
        assert_eq!(api_err.code, 1006);
    }
}
