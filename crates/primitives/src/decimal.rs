//! Fixed-point decimal arithmetic for financial calculations
//!
//! All amounts are stored as integers with implicit decimal places.
//! This avoids floating-point precision issues.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Fixed-point decimal with configurable precision
///
/// Internal representation: value * 10^decimals
///
/// Serializes to/from JSON as a string with decimal notation (e.g., "123.456")
/// to avoid precision loss and support values larger than JSON's number type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Decimal {
    /// Raw integer value (scaled)
    value: i128,
    /// Number of decimal places
    decimals: u8,
}

impl Serialize for Decimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // JSON: serialize as string for API compatibility
            serializer.serialize_str(&self.to_string_trimmed())
        } else {
            // Binary format (bincode): preserve exact (value, decimals) representation
            use serde::ser::SerializeTuple;
            let mut tup = serializer.serialize_tuple(2)?;
            tup.serialize_element(&self.value)?;
            tup.serialize_element(&self.decimals)?;
            tup.end()
        }
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            // JSON: parse from string with default precision
            let s = String::deserialize(deserializer)?;
            Self::from_str_exact(&s, Self::PRICE_DECIMALS)
                .ok_or_else(|| serde::de::Error::custom(format!("Invalid decimal: {}", s)))
        } else {
            // Binary format (bincode): read exact (value, decimals) pair
            let (value, decimals) = <(i128, u8)>::deserialize(deserializer)?;
            Ok(Decimal::from_raw(value, decimals))
        }
    }
}

impl Decimal {
    /// Standard decimals for different use cases
    pub const PRICE_DECIMALS: u8 = 8;
    pub const SIZE_DECIMALS: u8 = 8;
    pub const USDC_DECIMALS: u8 = 6;
    pub const RATE_DECIMALS: u8 = 10;

    /// Zero value
    pub const ZERO: Decimal = Decimal { value: 0, decimals: 8 };

    /// One (1.0)
    pub const ONE: Decimal = Decimal { value: 100_000_000, decimals: 8 };

    /// Create from raw integer value with specified decimals
    pub const fn from_raw(value: i128, decimals: u8) -> Self {
        Self { value, decimals }
    }

    /// Get the raw integer value (scaled by 10^decimals)
    pub const fn raw_value(&self) -> i128 {
        self.value
    }

    /// Get the number of decimal places
    pub const fn decimal_places(&self) -> u8 {
        self.decimals
    }

    /// Create from integer (no decimals)
    pub fn from_int(value: i64, decimals: u8) -> Self {
        let scale = 10i128.pow(decimals as u32);
        Self {
            value: (value as i128) * scale,
            decimals,
        }
    }

    /// Parse from string representation
    pub fn from_str_exact(s: &str, decimals: u8) -> Option<Self> {
        let s = s.trim();
        let negative = s.starts_with('-');
        let s = if negative { &s[1..] } else { s };

        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() > 2 {
            return None;
        }

        let integer_part: i128 = parts[0].parse().ok()?;
        let scale = 10i128.pow(decimals as u32);

        let decimal_part = if parts.len() == 2 {
            let decimal_str = parts[1];
            if decimal_str.len() > decimals as usize {
                return None; // Too many decimal places
            }
            let padding = decimals as usize - decimal_str.len();
            let padded = format!("{}{}", decimal_str, "0".repeat(padding));
            padded.parse::<i128>().ok()?
        } else {
            0
        };

        let value = integer_part.checked_mul(scale)?.checked_add(decimal_part)?;
        Some(Self {
            value: if negative { -value } else { value },
            decimals,
        })
    }

    /// Get the raw integer value
    pub const fn raw(&self) -> i128 {
        self.value
    }

    /// Get the number of decimals
    pub const fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Convert to different decimal precision
    pub fn to_decimals(&self, target_decimals: u8) -> Self {
        if target_decimals == self.decimals {
            return *self;
        }

        let value = if target_decimals > self.decimals {
            let diff = target_decimals - self.decimals;
            self.value * 10i128.pow(diff as u32)
        } else {
            let diff = self.decimals - target_decimals;
            self.value / 10i128.pow(diff as u32)
        };

        Self {
            value,
            decimals: target_decimals,
        }
    }

    /// Check if value is zero
    pub const fn is_zero(&self) -> bool {
        self.value == 0
    }

    /// Check if value is positive
    pub const fn is_positive(&self) -> bool {
        self.value > 0
    }

    /// Check if value is negative
    pub const fn is_negative(&self) -> bool {
        self.value < 0
    }

    /// Absolute value
    pub fn abs(&self) -> Self {
        Self {
            value: self.value.abs(),
            decimals: self.decimals,
        }
    }

    /// Minimum of two values (must have same decimals)
    pub fn min(self, other: Self) -> Self {
        debug_assert_eq!(self.decimals, other.decimals);
        if self.value <= other.value {
            self
        } else {
            other
        }
    }

    /// Maximum of two values (must have same decimals)
    pub fn max(self, other: Self) -> Self {
        debug_assert_eq!(self.decimals, other.decimals);
        if self.value >= other.value {
            self
        } else {
            other
        }
    }

    /// Checked addition
    pub fn checked_add(self, other: Self) -> Option<Self> {
        debug_assert_eq!(self.decimals, other.decimals);
        Some(Self {
            value: self.value.checked_add(other.value)?,
            decimals: self.decimals,
        })
    }

    /// Checked subtraction
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        debug_assert_eq!(self.decimals, other.decimals);
        Some(Self {
            value: self.value.checked_sub(other.value)?,
            decimals: self.decimals,
        })
    }

    /// Checked multiplication (result has sum of decimals, then normalized)
    pub fn checked_mul(self, other: Self) -> Option<Self> {
        let raw_product = self.value.checked_mul(other.value)?;
        // Normalize: divide by one scale factor
        let divisor = 10i128.pow(other.decimals as u32);
        Some(Self {
            value: raw_product / divisor,
            decimals: self.decimals,
        })
    }

    /// Checked division
    pub fn checked_div(self, other: Self) -> Option<Self> {
        if other.value == 0 {
            return None;
        }
        // Scale up numerator first to maintain precision
        let scale = 10i128.pow(other.decimals as u32);
        let scaled_value = self.value.checked_mul(scale)?;
        Some(Self {
            value: scaled_value / other.value,
            decimals: self.decimals,
        })
    }

    /// Multiply by integer
    pub fn mul_int(self, factor: i64) -> Self {
        Self {
            value: self.value * (factor as i128),
            decimals: self.decimals,
        }
    }

    /// Divide by integer
    pub fn div_int(self, divisor: i64) -> Self {
        Self {
            value: self.value / (divisor as i128),
            decimals: self.decimals,
        }
    }

    /// Convert to f64 (for display/logging only, not computation)
    pub fn to_f64(&self) -> f64 {
        let scale = 10f64.powi(self.decimals as i32);
        (self.value as f64) / scale
    }

    /// Convert to string with all decimal places
    pub fn to_string_full(&self) -> String {
        let scale = 10i128.pow(self.decimals as u32);
        let sign = if self.value < 0 { "-" } else { "" };
        let abs_value = self.value.abs();
        let integer = abs_value / scale;
        let decimal = abs_value % scale;
        format!(
            "{}{}.{:0>width$}",
            sign,
            integer,
            decimal,
            width = self.decimals as usize
        )
    }

    /// Convert to string, trimming trailing zeros
    pub fn to_string_trimmed(&self) -> String {
        let full = self.to_string_full();
        if full.contains('.') {
            full.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            full
        }
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Decimal({})", self.to_string_trimmed())
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_trimmed())
    }
}

impl Add for Decimal {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        self.checked_add(other).expect("decimal overflow")
    }
}

impl Sub for Decimal {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        self.checked_sub(other).expect("decimal underflow")
    }
}

impl Mul for Decimal {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        self.checked_mul(other).expect("decimal overflow")
    }
}

impl Div for Decimal {
    type Output = Self;
    fn div(self, other: Self) -> Self {
        self.checked_div(other).expect("division by zero or overflow")
    }
}

impl Neg for Decimal {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            value: -self.value,
            decimals: self.decimals,
        }
    }
}

/// Convenience constructors for common decimal types
impl Decimal {
    /// Parse from string (uses price decimals by default)
    pub fn from_str(s: &str) -> Result<Self, &'static str> {
        Self::from_str_exact(s, Self::PRICE_DECIMALS)
            .ok_or("invalid decimal string")
    }

    /// Scale to target number of decimal places (alias for to_decimals)
    pub fn scale_to(&self, target_decimals: u8) -> Self {
        self.to_decimals(target_decimals)
    }

    /// Create a price value (8 decimals)
    pub fn price(s: &str) -> Self {
        Self::from_str_exact(s, Self::PRICE_DECIMALS)
            .expect("invalid price string")
    }

    /// Create a size value (8 decimals)
    pub fn size(s: &str) -> Self {
        Self::from_str_exact(s, Self::SIZE_DECIMALS)
            .expect("invalid size string")
    }

    /// Create a USDC amount (6 decimals)
    pub fn usdc(s: &str) -> Self {
        Self::from_str_exact(s, Self::USDC_DECIMALS)
            .expect("invalid USDC string")
    }

    /// Create a rate value (10 decimals for precision)
    pub fn rate(s: &str) -> Self {
        Self::from_str_exact(s, Self::RATE_DECIMALS)
            .expect("invalid rate string")
    }

    /// Batch price comparison (Phase E4: SIMD-ready)
    ///
    /// Compares this price against multiple prices at once.
    /// Returns a bitmask where bit i is set if self >= prices[i].
    /// Currently uses scalar implementation; will use SIMD when available.
    pub fn batch_compare_ge(&self, prices: &[Decimal]) -> u64 {
        let mut mask: u64 = 0;
        let self_val = self.to_decimals(Self::PRICE_DECIMALS).value;
        for (i, price) in prices.iter().enumerate().take(64) {
            let other_val = price.to_decimals(Self::PRICE_DECIMALS).value;
            if self_val >= other_val {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Batch addition (Phase E4: SIMD-ready)
    ///
    /// Adds a value to multiple decimals at once.
    /// Currently uses scalar implementation.
    pub fn batch_add(values: &[Decimal], addend: Decimal) -> Vec<Decimal> {
        values.iter().map(|v| *v + addend).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        let d = Decimal::from_str_exact("123.456", 6).unwrap();
        assert_eq!(d.raw(), 123_456_000);

        let d = Decimal::from_str_exact("-100.5", 6).unwrap();
        assert_eq!(d.raw(), -100_500_000);

        let d = Decimal::from_str_exact("0.000001", 6).unwrap();
        assert_eq!(d.raw(), 1);
    }

    #[test]
    fn test_arithmetic() {
        let a = Decimal::from_str_exact("100.5", 6).unwrap();
        let b = Decimal::from_str_exact("50.25", 6).unwrap();

        let sum = a + b;
        assert_eq!(sum.to_string_trimmed(), "150.75");

        let diff = a - b;
        assert_eq!(diff.to_string_trimmed(), "50.25");
    }

    #[test]
    fn test_multiplication() {
        let price = Decimal::from_str_exact("65000", 8).unwrap();
        let size = Decimal::from_str_exact("0.1", 8).unwrap();

        let notional = price * size;
        assert_eq!(notional.to_string_trimmed(), "6500");
    }

    #[test]
    fn test_division() {
        let notional = Decimal::from_str_exact("6500", 8).unwrap();
        let leverage = Decimal::from_int(10, 8);

        let margin = notional / leverage;
        assert_eq!(margin.to_string_trimmed(), "650");
    }

    #[test]
    fn test_conversion() {
        let usdc = Decimal::from_str_exact("1000.50", 6).unwrap();
        let converted = usdc.to_decimals(8);
        assert_eq!(converted.raw(), 100_050_000_000);
        assert_eq!(converted.to_string_trimmed(), "1000.5");
    }

    #[test]
    fn test_bincode_roundtrip_preserves_precision() {
        // This is the exact scenario that caused consensus failure:
        // A Decimal with decimals=10 (RATE_DECIMALS) was persisted via bincode,
        // but the old serde impl always deserialized with decimals=8 (PRICE_DECIMALS),
        // changing the raw value and causing hash divergence.
        let cases = vec![
            Decimal::from_raw(1_000_000_000, 10),  // 0.1 with RATE_DECIMALS
            Decimal::from_raw(10_000_000, 8),       // 0.1 with PRICE_DECIMALS
            Decimal::from_raw(100_000, 6),           // 0.1 with USDC_DECIMALS
            Decimal::from_raw(-500_000_000, 10),    // negative rate
            Decimal::from_raw(0, 10),                // zero with non-default decimals
            Decimal::from_raw(123_456_789, 8),      // arbitrary value
        ];

        for original in &cases {
            let bytes = bincode::serialize(original).unwrap();
            let restored: Decimal = bincode::deserialize(&bytes).unwrap();
            assert_eq!(original.raw(), restored.raw(),
                "raw value mismatch for {:?}: {} != {}", original, original.raw(), restored.raw());
            assert_eq!(original.decimals(), restored.decimals(),
                "decimals mismatch for {:?}: {} != {}", original, original.decimals(), restored.decimals());
        }
    }

    #[test]
    fn test_json_roundtrip_uses_string() {
        let d = Decimal::from_str_exact("123.456", 8).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"123.456\"");
        let restored: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.to_string_trimmed(), "123.456");
    }
}
