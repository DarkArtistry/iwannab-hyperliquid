//! Unified State Model - Master Balance Sheet with Views
//!
//! This module implements Hyperliquid's unified state architecture where all user balances
//! are stored in a single master balance sheet with separate views for HyperCore and HyperEVM.
//!
//! ## Architecture
//!
//! Unlike having separate balance systems for trading and EVM, we maintain:
//! - **Total Balance**: The source of truth, changes only on deposits/withdrawals
//! - **Core View**: Portion available for HyperCore trading (orders, margin)
//! - **EVM View**: Portion available for HyperEVM (gas, contracts, DeFi)
//!
//! ## Invariant
//!
//! `total == core_view + evm_view` must ALWAYS hold.
//!
//! ## View Transfers
//!
//! Moving tokens between layers is NOT a transfer - it's a view adjustment:
//! - Core → EVM: `core_view -= amount; evm_view += amount; total unchanged`
//! - EVM → Core: `evm_view -= amount; core_view += amount; total unchanged`
//!
//! This is atomic and instant - no bridge contracts, no wrapped tokens.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{AccountAddress, Decimal, TokenIndex};

/// Balance with separate views for Core and EVM layers
///
/// The total is the source of truth. The views represent how much of that
/// total is available on each layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnifiedBalance {
    /// Total balance (source of truth)
    /// Only changes on actual deposits/withdrawals to the system
    pub total: Decimal,
    /// Portion available for HyperCore trading (orders, margin, etc.)
    pub core_view: Decimal,
    /// Portion available for HyperEVM (gas fees, contract calls, DeFi)
    pub evm_view: Decimal,
}

impl UnifiedBalance {
    /// Create a new unified balance with all balance in core view (default)
    pub fn new(total: Decimal) -> Self {
        Self {
            total,
            core_view: total,
            evm_view: Decimal::from_raw(0, total.decimals()),
        }
    }

    /// Create a new unified balance with specified views
    pub fn with_views(total: Decimal, core_view: Decimal, evm_view: Decimal) -> Self {
        debug_assert!(
            total == core_view + evm_view,
            "Invariant violated: total != core_view + evm_view"
        );
        Self {
            total,
            core_view,
            evm_view,
        }
    }

    /// Check the invariant: total == core_view + evm_view
    pub fn is_valid(&self) -> bool {
        self.total == self.core_view + self.evm_view
    }

    /// Check if balance is zero
    pub fn is_zero(&self) -> bool {
        self.total.is_zero()
    }
}

/// Unified State - Master balance sheet with views
///
/// This is the single source of truth for all user balances across both layers.
#[derive(Debug, Clone, Default)]
pub struct UnifiedState {
    /// Master balance sheet: (user, token) -> UnifiedBalance
    balances: HashMap<(AccountAddress, TokenIndex), UnifiedBalance>,
}

impl UnifiedState {
    /// Create a new empty unified state
    pub fn new() -> Self {
        Self::default()
    }

    /// Get unified balance for a user and token
    pub fn get_balance(&self, user: AccountAddress, token: TokenIndex) -> Option<&UnifiedBalance> {
        self.balances.get(&(user, token))
    }

    /// Get unified balance or default (zero balance)
    pub fn get_balance_or_default(&self, user: AccountAddress, token: TokenIndex) -> UnifiedBalance {
        self.balances
            .get(&(user, token))
            .cloned()
            .unwrap_or_else(|| {
                // Default decimals based on token: USDC (0) = 6, others = 18
                let decimals = if token == 0 { 6 } else { 18 };
                UnifiedBalance::new(Decimal::from_raw(0, decimals))
            })
    }

    /// Get all balances for a user
    pub fn get_all_balances(&self, user: AccountAddress) -> Vec<(TokenIndex, UnifiedBalance)> {
        self.balances
            .iter()
            .filter(|((u, _), _)| *u == user)
            .map(|((_, token), balance)| (*token, balance.clone()))
            .collect()
    }

    /// Get the core view balance (available for trading)
    pub fn get_core_view(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        self.balances
            .get(&(user, token))
            .map(|b| b.core_view)
            .unwrap_or_else(|| {
                let decimals = if token == 0 { 6 } else { 18 };
                Decimal::from_raw(0, decimals)
            })
    }

    /// Get the EVM view balance (available for EVM operations)
    pub fn get_evm_view(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        self.balances
            .get(&(user, token))
            .map(|b| b.evm_view)
            .unwrap_or_else(|| {
                let decimals = if token == 0 { 6 } else { 18 };
                Decimal::from_raw(0, decimals)
            })
    }

    /// Get total balance (source of truth)
    pub fn get_total(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        self.balances
            .get(&(user, token))
            .map(|b| b.total)
            .unwrap_or_else(|| {
                let decimals = if token == 0 { 6 } else { 18 };
                Decimal::from_raw(0, decimals)
            })
    }

    /// Credit balance (deposit) - adds to total and core_view by default
    pub fn credit(&mut self, user: AccountAddress, token: TokenIndex, amount: Decimal) {
        let balance = self.balances.entry((user, token)).or_insert_with(|| {
            UnifiedBalance::new(Decimal::from_raw(0, amount.decimals()))
        });
        balance.total = balance.total + amount;
        balance.core_view = balance.core_view + amount;
    }

    /// Credit balance to EVM view (for EVM-originated deposits)
    pub fn credit_evm(&mut self, user: AccountAddress, token: TokenIndex, amount: Decimal) {
        let balance = self.balances.entry((user, token)).or_insert_with(|| {
            UnifiedBalance::new(Decimal::from_raw(0, amount.decimals()))
        });
        balance.total = balance.total + amount;
        balance.evm_view = balance.evm_view + amount;
    }

    /// Debit balance (withdrawal) from core view
    /// Returns true if successful, false if insufficient balance
    pub fn debit_core(&mut self, user: AccountAddress, token: TokenIndex, amount: Decimal) -> bool {
        if let Some(balance) = self.balances.get_mut(&(user, token)) {
            if balance.core_view >= amount {
                balance.total = balance.total - amount;
                balance.core_view = balance.core_view - amount;
                return true;
            }
        }
        false
    }

    /// Debit balance (withdrawal) from EVM view
    /// Returns true if successful, false if insufficient balance
    pub fn debit_evm(&mut self, user: AccountAddress, token: TokenIndex, amount: Decimal) -> bool {
        if let Some(balance) = self.balances.get_mut(&(user, token)) {
            if balance.evm_view >= amount {
                balance.total = balance.total - amount;
                balance.evm_view = balance.evm_view - amount;
                return true;
            }
        }
        false
    }

    /// Transfer balance from Core view to EVM view
    ///
    /// This is NOT a transfer - it's a view adjustment. The total remains unchanged.
    /// Returns Ok(()) if successful, Err with reason if failed.
    pub fn transfer_to_evm_view(
        &mut self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), ViewTransferError> {
        let balance = self
            .balances
            .get_mut(&(user, token))
            .ok_or(ViewTransferError::NoBalance)?;

        if balance.core_view < amount {
            return Err(ViewTransferError::InsufficientCoreView {
                available: balance.core_view,
                requested: amount,
            });
        }

        balance.core_view = balance.core_view - amount;
        balance.evm_view = balance.evm_view + amount;

        // Verify invariant
        debug_assert!(balance.is_valid(), "Invariant violated after transfer_to_evm_view");

        Ok(())
    }

    /// Transfer balance from EVM view to Core view
    ///
    /// This is NOT a transfer - it's a view adjustment. The total remains unchanged.
    /// Returns Ok(()) if successful, Err with reason if failed.
    pub fn transfer_to_core_view(
        &mut self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), ViewTransferError> {
        let balance = self
            .balances
            .get_mut(&(user, token))
            .ok_or(ViewTransferError::NoBalance)?;

        if balance.evm_view < amount {
            return Err(ViewTransferError::InsufficientEvmView {
                available: balance.evm_view,
                requested: amount,
            });
        }

        balance.evm_view = balance.evm_view - amount;
        balance.core_view = balance.core_view + amount;

        // Verify invariant
        debug_assert!(balance.is_valid(), "Invariant violated after transfer_to_core_view");

        Ok(())
    }

    /// Internal transfer between users on the same view (for trading fills)
    /// Transfers from sender's core_view to receiver's core_view
    pub fn transfer_core(
        &mut self,
        from: AccountAddress,
        to: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> bool {
        // Check sender has enough
        let from_balance = self.balances.get(&(from, token));
        if from_balance.map(|b| b.core_view).unwrap_or_default() < amount {
            return false;
        }

        // Debit sender
        if let Some(balance) = self.balances.get_mut(&(from, token)) {
            balance.total = balance.total - amount;
            balance.core_view = balance.core_view - amount;
        }

        // Credit receiver
        let to_balance = self.balances.entry((to, token)).or_insert_with(|| {
            UnifiedBalance::new(Decimal::from_raw(0, amount.decimals()))
        });
        to_balance.total = to_balance.total + amount;
        to_balance.core_view = to_balance.core_view + amount;

        true
    }

    /// Internal transfer between users on EVM view (for EVM transfers)
    pub fn transfer_evm(
        &mut self,
        from: AccountAddress,
        to: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> bool {
        // Check sender has enough
        let from_balance = self.balances.get(&(from, token));
        if from_balance.map(|b| b.evm_view).unwrap_or_default() < amount {
            return false;
        }

        // Debit sender
        if let Some(balance) = self.balances.get_mut(&(from, token)) {
            balance.total = balance.total - amount;
            balance.evm_view = balance.evm_view - amount;
        }

        // Credit receiver
        let to_balance = self.balances.entry((to, token)).or_insert_with(|| {
            UnifiedBalance::new(Decimal::from_raw(0, amount.decimals()))
        });
        to_balance.total = to_balance.total + amount;
        to_balance.evm_view = to_balance.evm_view + amount;

        true
    }

    /// Set balance directly (for initialization/testing)
    pub fn set_balance(&mut self, user: AccountAddress, token: TokenIndex, balance: UnifiedBalance) {
        self.balances.insert((user, token), balance);
    }

    /// Get all users with balances for a specific token
    pub fn get_token_holders(&self, token: TokenIndex) -> Vec<(AccountAddress, UnifiedBalance)> {
        self.balances
            .iter()
            .filter(|((_, t), _)| *t == token)
            .map(|((user, _), balance)| (*user, balance.clone()))
            .collect()
    }

    /// Verify all balances satisfy the invariant
    pub fn verify_invariants(&self) -> bool {
        self.balances.values().all(|b| b.is_valid())
    }
}

/// Error type for view transfer operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum ViewTransferError {
    #[error("No balance found for user/token")]
    NoBalance,

    #[error("Insufficient Core view balance: available {available}, requested {requested}")]
    InsufficientCoreView { available: Decimal, requested: Decimal },

    #[error("Insufficient EVM view balance: available {available}, requested {requested}")]
    InsufficientEvmView { available: Decimal, requested: Decimal },

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),
}

/// Thread-safe wrapper for UnifiedState
pub type SharedUnifiedState = Arc<RwLock<UnifiedState>>;

/// Create a new shared unified state
pub fn new_shared_unified_state() -> SharedUnifiedState {
    Arc::new(RwLock::new(UnifiedState::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user() -> AccountAddress {
        AccountAddress::from([0x42u8; 20])
    }

    fn test_user2() -> AccountAddress {
        AccountAddress::from([0x43u8; 20])
    }

    #[test]
    fn test_unified_balance_creation() {
        let balance = UnifiedBalance::new(Decimal::from_raw(1000, 6));
        assert_eq!(balance.total.raw(), 1000);
        assert_eq!(balance.core_view.raw(), 1000);
        assert_eq!(balance.evm_view.raw(), 0);
        assert!(balance.is_valid());
    }

    #[test]
    fn test_unified_balance_with_views() {
        let balance = UnifiedBalance::with_views(
            Decimal::from_raw(1000, 6),
            Decimal::from_raw(600, 6),
            Decimal::from_raw(400, 6),
        );
        assert!(balance.is_valid());
        assert_eq!(balance.total.raw(), 1000);
        assert_eq!(balance.core_view.raw(), 600);
        assert_eq!(balance.evm_view.raw(), 400);
    }

    #[test]
    fn test_credit_balance() {
        let mut state = UnifiedState::new();
        let user = test_user();

        state.credit(user, 0, Decimal::from_raw(1000, 6));

        let balance = state.get_balance(user, 0).unwrap();
        assert_eq!(balance.total.raw(), 1000);
        assert_eq!(balance.core_view.raw(), 1000);
        assert_eq!(balance.evm_view.raw(), 0);
    }

    #[test]
    fn test_view_transfer_to_evm() {
        let mut state = UnifiedState::new();
        let user = test_user();

        // Credit 1000 USDC to core view
        state.credit(user, 0, Decimal::from_raw(1000, 6));

        // Transfer 400 to EVM view
        let result = state.transfer_to_evm_view(user, 0, Decimal::from_raw(400, 6));
        assert!(result.is_ok());

        let balance = state.get_balance(user, 0).unwrap();
        assert_eq!(balance.total.raw(), 1000); // Unchanged!
        assert_eq!(balance.core_view.raw(), 600);
        assert_eq!(balance.evm_view.raw(), 400);
        assert!(balance.is_valid());
    }

    #[test]
    fn test_view_transfer_to_core() {
        let mut state = UnifiedState::new();
        let user = test_user();

        // Start with balance split between views
        state.set_balance(
            user,
            0,
            UnifiedBalance::with_views(
                Decimal::from_raw(1000, 6),
                Decimal::from_raw(600, 6),
                Decimal::from_raw(400, 6),
            ),
        );

        // Transfer 200 from EVM to Core
        let result = state.transfer_to_core_view(user, 0, Decimal::from_raw(200, 6));
        assert!(result.is_ok());

        let balance = state.get_balance(user, 0).unwrap();
        assert_eq!(balance.total.raw(), 1000); // Unchanged!
        assert_eq!(balance.core_view.raw(), 800);
        assert_eq!(balance.evm_view.raw(), 200);
        assert!(balance.is_valid());
    }

    #[test]
    fn test_insufficient_core_view() {
        let mut state = UnifiedState::new();
        let user = test_user();

        state.credit(user, 0, Decimal::from_raw(1000, 6));

        // Try to transfer more than available in core view
        let result = state.transfer_to_evm_view(user, 0, Decimal::from_raw(1500, 6));
        assert!(matches!(result, Err(ViewTransferError::InsufficientCoreView { .. })));
    }

    #[test]
    fn test_insufficient_evm_view() {
        let mut state = UnifiedState::new();
        let user = test_user();

        // Start with all balance in core view (no EVM view)
        state.credit(user, 0, Decimal::from_raw(1000, 6));

        // Try to transfer from empty EVM view
        let result = state.transfer_to_core_view(user, 0, Decimal::from_raw(100, 6));
        assert!(matches!(result, Err(ViewTransferError::InsufficientEvmView { .. })));
    }

    #[test]
    fn test_internal_core_transfer() {
        let mut state = UnifiedState::new();
        let alice = test_user();
        let bob = test_user2();

        // Alice has 1000 USDC
        state.credit(alice, 0, Decimal::from_raw(1000, 6));

        // Transfer 400 from Alice to Bob (on core layer)
        let result = state.transfer_core(alice, bob, 0, Decimal::from_raw(400, 6));
        assert!(result);

        // Check Alice
        let alice_balance = state.get_balance(alice, 0).unwrap();
        assert_eq!(alice_balance.total.raw(), 600);
        assert_eq!(alice_balance.core_view.raw(), 600);

        // Check Bob
        let bob_balance = state.get_balance(bob, 0).unwrap();
        assert_eq!(bob_balance.total.raw(), 400);
        assert_eq!(bob_balance.core_view.raw(), 400);
    }

    #[test]
    fn test_verify_invariants() {
        let mut state = UnifiedState::new();
        let user = test_user();

        state.credit(user, 0, Decimal::from_raw(1000, 6));
        state.transfer_to_evm_view(user, 0, Decimal::from_raw(400, 6)).unwrap();

        assert!(state.verify_invariants());
    }

    #[test]
    fn test_multiple_tokens() {
        let mut state = UnifiedState::new();
        let user = test_user();

        // Credit USDC (token 0)
        state.credit(user, 0, Decimal::from_raw(1000, 6));
        // Credit TEST (token 1) with 18 decimals
        state.credit(user, 1, Decimal::from_raw(500_000_000_000_000_000_000i128, 18)); // 500 TEST

        // Transfer some TEST to EVM
        state.transfer_to_evm_view(user, 1, Decimal::from_raw(200_000_000_000_000_000_000i128, 18)).unwrap();

        // Check USDC unchanged
        let usdc = state.get_balance(user, 0).unwrap();
        assert_eq!(usdc.total.raw(), 1000);
        assert_eq!(usdc.core_view.raw(), 1000);

        // Check TEST split
        let test = state.get_balance(user, 1).unwrap();
        assert_eq!(test.total.raw(), 500_000_000_000_000_000_000i128);
        assert_eq!(test.core_view.raw(), 300_000_000_000_000_000_000i128);
        assert_eq!(test.evm_view.raw(), 200_000_000_000_000_000_000i128);
    }
}
