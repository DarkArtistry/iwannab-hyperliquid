//! SIMD-like batch operations for trading engine (Phase E1)
//!
//! Provides vectorized batch processing for common engine operations:
//! - Batch PnL calculations across multiple positions
//! - Batch margin checks for multiple accounts
//! - Batch price comparisons for order matching
//!
//! Uses data-parallel patterns that auto-vectorize well with LLVM,
//! achieving near-SIMD performance on stable Rust without nightly features.

/// Batch multiply two slices of i128 values element-wise.
/// Result is scaled down by `scale_factor` to maintain fixed-point precision.
///
/// Used for: notional value = price * size (both fixed-point)
#[inline]
pub fn batch_mul_scaled(a: &[i128], b: &[i128], scale_factor: i128, out: &mut Vec<i128>) {
    out.clear();
    out.reserve(a.len());
    for i in 0..a.len().min(b.len()) {
        out.push(a[i].wrapping_mul(b[i]) / scale_factor);
    }
}

/// Batch compute unrealized PnL for multiple positions.
///
/// For each position: pnl = (mark_price - entry_price) * size
/// Long positions: positive when mark > entry
/// Short positions: size is negative, so pnl is positive when mark < entry
///
/// All values are fixed-point with `price_scale` decimals.
#[inline]
pub fn batch_unrealized_pnl(
    entry_prices: &[i128],
    mark_prices: &[i128],
    sizes: &[i128],
    price_scale: i128,
    out: &mut Vec<i128>,
) {
    out.clear();
    let n = entry_prices.len().min(mark_prices.len()).min(sizes.len());
    out.reserve(n);
    for i in 0..n {
        let price_diff = mark_prices[i] - entry_prices[i];
        out.push(price_diff.wrapping_mul(sizes[i]) / price_scale);
    }
}

/// Batch compute margin requirements for multiple positions.
///
/// margin_required = abs(notional_value) / leverage
/// notional_value = mark_price * size
///
/// Returns margin required for each position.
#[inline]
pub fn batch_margin_required(
    mark_prices: &[i128],
    sizes: &[i128],
    leverages: &[u8],
    price_scale: i128,
    out: &mut Vec<i128>,
) {
    out.clear();
    let n = mark_prices.len().min(sizes.len()).min(leverages.len());
    out.reserve(n);
    for i in 0..n {
        let notional = (mark_prices[i].wrapping_mul(sizes[i]) / price_scale).abs();
        let leverage = leverages[i].max(1) as i128;
        out.push(notional / leverage);
    }
}

/// Batch check if prices cross (for order matching).
///
/// For buy orders: crosses if ask_price <= bid_price
/// For sell orders: crosses if bid_price >= ask_price
///
/// Returns a bitmask where bit i is set if order i crosses.
/// Supports up to 64 orders per batch.
#[inline]
pub fn batch_price_crosses(bid_prices: &[i128], ask_prices: &[i128]) -> u64 {
    let mut mask: u64 = 0;
    let n = bid_prices.len().min(ask_prices.len()).min(64);
    for i in 0..n {
        if bid_prices[i] >= ask_prices[i] {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Batch sum of absolute values (for total notional position calculation).
#[inline]
pub fn batch_abs_sum(values: &[i128]) -> i128 {
    values.iter().map(|v| v.abs()).sum()
}

/// Batch compute liquidation prices for multiple positions.
///
/// For long: liq_price = entry_price * (1 - 1/leverage + maintenance_margin_rate)
/// For short: liq_price = entry_price * (1 + 1/leverage - maintenance_margin_rate)
///
/// Simplified: liq_price = entry_price - (balance * price_scale) / size (for longs)
#[inline]
pub fn batch_liquidation_check(
    mark_prices: &[i128],
    entry_prices: &[i128],
    sizes: &[i128],
    balances: &[i128],
    maintenance_rates: &[i128], // Fixed-point maintenance margin rates
    price_scale: i128,
    out: &mut Vec<bool>, // true = needs liquidation
) {
    out.clear();
    let n = mark_prices.len()
        .min(entry_prices.len())
        .min(sizes.len())
        .min(balances.len())
        .min(maintenance_rates.len());
    out.reserve(n);

    for i in 0..n {
        if sizes[i] == 0 {
            out.push(false);
            continue;
        }

        let unrealized_pnl = (mark_prices[i] - entry_prices[i]).wrapping_mul(sizes[i]) / price_scale;
        let equity = balances[i] + unrealized_pnl;
        let notional = (mark_prices[i].wrapping_mul(sizes[i]) / price_scale).abs();
        let maintenance_margin = notional.wrapping_mul(maintenance_rates[i]) / price_scale;

        out.push(equity < maintenance_margin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRICE_SCALE: i128 = 100_000_000; // 8 decimals

    #[test]
    fn test_batch_mul_scaled() {
        let prices = vec![65000 * PRICE_SCALE, 3500 * PRICE_SCALE, 150 * PRICE_SCALE];
        let sizes = vec![1 * PRICE_SCALE, 10 * PRICE_SCALE, 100 * PRICE_SCALE];
        let mut out = Vec::new();

        batch_mul_scaled(&prices, &sizes, PRICE_SCALE, &mut out);

        assert_eq!(out[0], 65000 * PRICE_SCALE); // 65000 * 1
        assert_eq!(out[1], 35000 * PRICE_SCALE); // 3500 * 10
        assert_eq!(out[2], 15000 * PRICE_SCALE); // 150 * 100
    }

    #[test]
    fn test_batch_unrealized_pnl() {
        let entry = vec![65000 * PRICE_SCALE, 3500 * PRICE_SCALE];
        let mark = vec![66000 * PRICE_SCALE, 3400 * PRICE_SCALE];
        let sizes = vec![1 * PRICE_SCALE, -10 * PRICE_SCALE]; // long BTC, short ETH
        let mut out = Vec::new();

        batch_unrealized_pnl(&entry, &mark, &sizes, PRICE_SCALE, &mut out);

        assert_eq!(out[0], 1000 * PRICE_SCALE); // +1000 (long, price up)
        assert_eq!(out[1], 1000 * PRICE_SCALE); // +1000 (short, price down: (-100) * (-10) = +1000)
    }

    #[test]
    fn test_batch_margin_required() {
        let mark = vec![65000 * PRICE_SCALE, 3500 * PRICE_SCALE];
        let sizes = vec![1 * PRICE_SCALE, 10 * PRICE_SCALE];
        let leverages = vec![10u8, 20u8];
        let mut out = Vec::new();

        batch_margin_required(&mark, &sizes, &leverages, PRICE_SCALE, &mut out);

        assert_eq!(out[0], 6500 * PRICE_SCALE); // 65000 / 10
        assert_eq!(out[1], 1750 * PRICE_SCALE); // 35000 / 20
    }

    #[test]
    fn test_batch_price_crosses() {
        let bids = vec![100 * PRICE_SCALE, 99 * PRICE_SCALE, 101 * PRICE_SCALE, 50 * PRICE_SCALE];
        let asks = vec![100 * PRICE_SCALE, 100 * PRICE_SCALE, 100 * PRICE_SCALE, 100 * PRICE_SCALE];

        let mask = batch_price_crosses(&bids, &asks);

        assert_eq!(mask & 1, 1); // 100 >= 100, crosses
        assert_eq!(mask & 2, 0); // 99 < 100, doesn't cross
        assert_eq!(mask & 4, 4); // 101 >= 100, crosses
        assert_eq!(mask & 8, 0); // 50 < 100, doesn't cross
    }

    #[test]
    fn test_batch_abs_sum() {
        let values = vec![100, -200, 300, -400];
        assert_eq!(batch_abs_sum(&values), 1000);
    }

    #[test]
    fn test_batch_liquidation_check() {
        let mark = vec![60000 * PRICE_SCALE, 70000 * PRICE_SCALE];
        let entry = vec![65000 * PRICE_SCALE, 65000 * PRICE_SCALE];
        let sizes = vec![1 * PRICE_SCALE, 1 * PRICE_SCALE]; // both long
        let balances = vec![3000 * PRICE_SCALE, 10000 * PRICE_SCALE]; // low balance, high balance
        let maint_rates = vec![PRICE_SCALE / 100, PRICE_SCALE / 100]; // 1% maintenance
        let mut out = Vec::new();

        batch_liquidation_check(&mark, &entry, &sizes, &balances, &maint_rates, PRICE_SCALE, &mut out);

        // Position 1: equity = 3000 + (60000-65000)*1 = -2000. margin = 60000*0.01 = 600. -2000 < 600 = needs liquidation
        assert!(out[0]);
        // Position 2: equity = 10000 + (70000-65000)*1 = 15000. margin = 70000*0.01 = 700. 15000 > 700 = safe
        assert!(!out[1]);
    }

    #[test]
    fn test_batch_liquidation_zero_size() {
        let mark = vec![65000 * PRICE_SCALE];
        let entry = vec![65000 * PRICE_SCALE];
        let sizes = vec![0i128]; // no position
        let balances = vec![0i128];
        let maint_rates = vec![PRICE_SCALE / 100];
        let mut out = Vec::new();

        batch_liquidation_check(&mark, &entry, &sizes, &balances, &maint_rates, PRICE_SCALE, &mut out);

        assert!(!out[0]); // No position = no liquidation
    }

    #[test]
    fn test_batch_empty_inputs() {
        let empty: Vec<i128> = vec![];
        let mut out = Vec::new();

        batch_mul_scaled(&empty, &empty, PRICE_SCALE, &mut out);
        assert!(out.is_empty());

        batch_unrealized_pnl(&empty, &empty, &empty, PRICE_SCALE, &mut out);
        assert!(out.is_empty());

        assert_eq!(batch_price_crosses(&empty, &empty), 0);
        assert_eq!(batch_abs_sum(&empty), 0);
    }

    #[test]
    fn test_batch_large_batch() {
        // Test with 64 items (max for bitmask)
        let bids: Vec<i128> = (0..64).map(|i| (100 + i) * PRICE_SCALE).collect();
        let asks: Vec<i128> = (0..64).map(|_| 132 * PRICE_SCALE).collect();

        let mask = batch_price_crosses(&bids, &asks);

        // Items 32..63 should cross (100+32=132 >= 132)
        for i in 0..32 {
            assert_eq!(mask & (1u64 << i), 0, "Item {} should not cross", i);
        }
        for i in 32..64 {
            assert_ne!(mask & (1u64 << i), 0, "Item {} should cross", i);
        }
    }
}
