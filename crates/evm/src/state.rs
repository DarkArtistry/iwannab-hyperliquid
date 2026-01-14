//! EVM state management
//!
//! ## Phase 2A: Unified State Integration
//!
//! EvmState now uses the UnifiedState model for native token balances:
//! - Native token (ETH-equivalent) balance comes from UnifiedState.evm_view
//! - Nonce, code, storage remain in EvmState (EVM-specific)
//!
//! This allows EVM operations to use the same balance sheet as HyperCore trading,
//! with seamless view transfers between layers.

use std::collections::HashMap;

use revm::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use hypercore_primitives::{AccountAddress, Decimal, SharedUnifiedState, new_shared_unified_state, TokenIndex};

/// Native token index for EVM operations (USDC in our system)
/// This is the token whose evm_view is used for EVM gas and native transfers.
pub const NATIVE_TOKEN_INDEX: TokenIndex = 0;

/// Number of decimals for USDC (native token)
const NATIVE_DECIMALS: u8 = 6;

/// EVM account metadata (nonce and code_hash only)
/// Balance is now stored in UnifiedState.evm_view
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmAccount {
    /// Account nonce
    pub nonce: u64,
    /// Contract code hash (None for EOA)
    pub code_hash: Option<B256>,
}

/// EVM contract storage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContractStorage {
    /// Storage slots
    pub slots: HashMap<U256, U256>,
}

/// EVM state database
///
/// ## Unified State Integration (Phase 2A)
///
/// Native token balances are now stored in UnifiedState:
/// - `get_balance()` reads from UnifiedState.evm_view
/// - `set_balance()` writes to UnifiedState.evm_view
/// - Nonce and code remain local to EvmState
#[derive(Debug, Clone)]
pub struct EvmState {
    /// Reference to unified state (master balance sheet)
    unified_state: SharedUnifiedState,
    /// Account metadata (nonce, code_hash - NOT balance)
    accounts: HashMap<Address, EvmAccount>,
    /// Contract storage
    storage: HashMap<Address, ContractStorage>,
    /// Contract code
    code: HashMap<B256, Vec<u8>>,
    /// Block hashes
    block_hashes: HashMap<u64, B256>,
}

impl Default for EvmState {
    fn default() -> Self {
        Self::new()
    }
}

impl EvmState {
    /// Create new EVM state
    pub fn new() -> Self {
        Self::with_unified_state(new_shared_unified_state())
    }

    /// Create new EVM state with shared unified state
    pub fn with_unified_state(unified_state: SharedUnifiedState) -> Self {
        Self {
            unified_state,
            accounts: HashMap::new(),
            storage: HashMap::new(),
            code: HashMap::new(),
            block_hashes: HashMap::new(),
        }
    }

    /// Get the shared unified state reference
    pub fn unified_state(&self) -> &SharedUnifiedState {
        &self.unified_state
    }

    /// Get account metadata
    pub fn get_account(&self, address: &Address) -> Option<&EvmAccount> {
        self.accounts.get(address)
    }

    /// Get or create account metadata
    pub fn get_or_create_account(&mut self, address: Address) -> &mut EvmAccount {
        self.accounts.entry(address).or_default()
    }

    /// Set account balance (writes to UnifiedState.evm_view)
    ///
    /// This converts the U256 balance to Decimal and stores it in the unified state.
    pub fn set_balance(&mut self, address: Address, balance: U256) {
        let account_addr = evm_to_account_address(&address);
        let decimal_balance = u256_to_decimal(balance);

        let mut unified = self.unified_state.write().unwrap();

        // Get current evm_view balance
        let current = unified.get_evm_view(account_addr, NATIVE_TOKEN_INDEX);

        if decimal_balance > current {
            // Need to credit the difference to evm_view
            let diff = decimal_balance - current;
            unified.credit_evm(account_addr, NATIVE_TOKEN_INDEX, diff);
        } else if decimal_balance < current {
            // Need to debit the difference from evm_view
            let diff = current - decimal_balance;
            unified.debit_evm(account_addr, NATIVE_TOKEN_INDEX, diff);
        }
        // If equal, no change needed

        // Ensure account metadata exists
        drop(unified);
        self.accounts.entry(address).or_default();
    }

    /// Get balance (reads from UnifiedState.evm_view)
    pub fn get_balance(&self, address: &Address) -> U256 {
        let account_addr = evm_to_account_address(address);
        let unified = self.unified_state.read().unwrap();
        let evm_view = unified.get_evm_view(account_addr, NATIVE_TOKEN_INDEX);
        decimal_to_u256(evm_view)
    }

    /// Add to balance (credits UnifiedState.evm_view)
    pub fn add_balance(&mut self, address: Address, amount: U256) {
        let account_addr = evm_to_account_address(&address);
        let decimal_amount = u256_to_decimal(amount);

        let mut unified = self.unified_state.write().unwrap();
        unified.credit_evm(account_addr, NATIVE_TOKEN_INDEX, decimal_amount);

        // Ensure account metadata exists
        drop(unified);
        self.accounts.entry(address).or_default();
    }

    /// Subtract from balance (debits UnifiedState.evm_view)
    /// Returns true if successful, false if insufficient balance
    pub fn sub_balance(&mut self, address: Address, amount: U256) -> bool {
        let account_addr = evm_to_account_address(&address);
        let decimal_amount = u256_to_decimal(amount);

        let mut unified = self.unified_state.write().unwrap();
        unified.debit_evm(account_addr, NATIVE_TOKEN_INDEX, decimal_amount)
    }

    /// Transfer balance between accounts on EVM layer
    pub fn transfer_balance(&mut self, from: Address, to: Address, amount: U256) -> bool {
        if amount.is_zero() {
            return true;
        }

        let from_addr = evm_to_account_address(&from);
        let to_addr = evm_to_account_address(&to);
        let decimal_amount = u256_to_decimal(amount);

        let mut unified = self.unified_state.write().unwrap();
        let result = unified.transfer_evm(from_addr, to_addr, NATIVE_TOKEN_INDEX, decimal_amount);

        // Ensure account metadata exists
        drop(unified);
        self.accounts.entry(from).or_default();
        self.accounts.entry(to).or_default();

        result
    }

    /// Increment nonce
    pub fn increment_nonce(&mut self, address: Address) -> u64 {
        let account = self.get_or_create_account(address);
        account.nonce += 1;
        account.nonce
    }

    /// Get nonce
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.accounts.get(address).map(|a| a.nonce).unwrap_or(0)
    }

    /// Set nonce
    pub fn set_nonce(&mut self, address: Address, nonce: u64) {
        self.get_or_create_account(address).nonce = nonce;
    }

    /// Set contract code
    pub fn set_code(&mut self, address: Address, code: Vec<u8>) {
        use sha3::{Digest, Keccak256};

        let digest = Keccak256::digest(&code);
        let code_hash: B256 = B256::from_slice(digest.as_slice());
        self.code.insert(code_hash, code);
        self.get_or_create_account(address).code_hash = Some(code_hash);
    }

    /// Get contract code
    pub fn get_code(&self, address: &Address) -> Option<&Vec<u8>> {
        let account = self.accounts.get(address)?;
        let code_hash = account.code_hash.as_ref()?;
        self.code.get(code_hash)
    }

    /// Get code hash
    pub fn get_code_hash(&self, address: &Address) -> Option<B256> {
        self.accounts.get(address)?.code_hash
    }

    /// Get code by hash
    pub fn get_code_by_hash(&self, hash: &B256) -> Option<&Vec<u8>> {
        self.code.get(hash)
    }

    /// Set storage slot
    pub fn set_storage(&mut self, address: Address, key: U256, value: U256) {
        self.storage.entry(address).or_default().slots.insert(key, value);
    }

    /// Get storage slot
    pub fn get_storage(&self, address: &Address, key: &U256) -> U256 {
        self.storage
            .get(address)
            .and_then(|s| s.slots.get(key))
            .copied()
            .unwrap_or_default()
    }

    /// Set block hash
    pub fn set_block_hash(&mut self, number: u64, hash: B256) {
        self.block_hashes.insert(number, hash);
    }

    /// Get block hash
    pub fn get_block_hash(&self, number: u64) -> Option<B256> {
        self.block_hashes.get(&number).copied()
    }

    /// Check if account exists (has metadata or balance)
    pub fn account_exists(&self, address: &Address) -> bool {
        if self.accounts.contains_key(address) {
            return true;
        }
        // Also check if account has balance in unified state
        let account_addr = evm_to_account_address(address);
        let unified = self.unified_state.read().unwrap();
        !unified.get_evm_view(account_addr, NATIVE_TOKEN_INDEX).is_zero()
    }

    /// Check if account has code (is contract)
    pub fn is_contract(&self, address: &Address) -> bool {
        self.accounts
            .get(address)
            .map(|a| a.code_hash.is_some())
            .unwrap_or(false)
    }

    /// Delete account
    pub fn delete_account(&mut self, address: &Address) {
        self.accounts.remove(address);
        self.storage.remove(address);
        // Note: We don't delete from unified state as the balance belongs to the user
    }

    /// Create state snapshot (for transaction rollback)
    pub fn snapshot(&self) -> EvmStateSnapshot {
        // Snapshot the unified state balances for EVM accounts
        let unified = self.unified_state.read().unwrap();
        let mut balance_snapshot = HashMap::new();
        for addr in self.accounts.keys() {
            let account_addr = evm_to_account_address(addr);
            let balance = unified.get_evm_view(account_addr, NATIVE_TOKEN_INDEX);
            balance_snapshot.insert(*addr, balance);
        }
        drop(unified);

        EvmStateSnapshot {
            accounts: self.accounts.clone(),
            storage: self.storage.clone(),
            code: self.code.clone(),
            balance_snapshot,
        }
    }

    /// Restore from snapshot
    pub fn restore(&mut self, snapshot: EvmStateSnapshot) {
        self.accounts = snapshot.accounts;
        self.storage = snapshot.storage;
        self.code = snapshot.code;

        // Restore balances from snapshot
        for (addr, balance) in snapshot.balance_snapshot {
            self.set_balance(addr, decimal_to_u256(balance));
        }
    }
}

/// EVM state snapshot for rollback
#[derive(Debug, Clone)]
pub struct EvmStateSnapshot {
    accounts: HashMap<Address, EvmAccount>,
    storage: HashMap<Address, ContractStorage>,
    code: HashMap<B256, Vec<u8>>,
    /// Snapshot of EVM view balances (for rollback)
    balance_snapshot: HashMap<Address, Decimal>,
}

/// State changes from transaction execution
#[derive(Debug, Clone, Default)]
pub struct StateChanges {
    /// Balance changes
    pub balance_changes: Vec<BalanceChange>,
    /// Storage changes
    pub storage_changes: Vec<StorageChange>,
    /// Created contracts
    pub created_contracts: Vec<Address>,
    /// Destroyed contracts
    pub destroyed_contracts: Vec<Address>,
}

#[derive(Debug, Clone)]
pub struct BalanceChange {
    pub address: Address,
    pub old_balance: U256,
    pub new_balance: U256,
}

#[derive(Debug, Clone)]
pub struct StorageChange {
    pub address: Address,
    pub key: U256,
    pub old_value: U256,
    pub new_value: U256,
}

// Conversion utilities

/// Convert revm Address to AccountAddress
fn evm_to_account_address(address: &Address) -> AccountAddress {
    // Both are [u8; 20], but different types
    AccountAddress::from_slice(address.as_slice())
}

/// Convert U256 to Decimal (USDC has 6 decimals)
fn u256_to_decimal(value: U256) -> Decimal {
    // U256 to i128 - will panic on overflow, but USDC amounts fit in i128
    let raw: i128 = value.try_into().unwrap_or(i128::MAX);
    Decimal::from_raw(raw, NATIVE_DECIMALS)
}

/// Convert Decimal to U256
fn decimal_to_u256(value: Decimal) -> U256 {
    // Convert to native decimals first
    let adjusted = value.to_decimals(NATIVE_DECIMALS);
    U256::from(adjusted.raw() as u128)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_management() {
        let mut state = EvmState::new();
        let addr = Address::ZERO;

        // Initially no account
        assert!(!state.account_exists(&addr));
        assert_eq!(state.get_balance(&addr), U256::ZERO);

        // Set balance creates account
        state.set_balance(addr, U256::from(1000));
        assert!(state.account_exists(&addr));
        assert_eq!(state.get_balance(&addr), U256::from(1000));
    }

    #[test]
    fn test_balance_operations() {
        let mut state = EvmState::new();
        let addr = Address::ZERO;

        // Add balance
        state.add_balance(addr, U256::from(1000));
        assert_eq!(state.get_balance(&addr), U256::from(1000));

        // Subtract balance
        assert!(state.sub_balance(addr, U256::from(300)));
        assert_eq!(state.get_balance(&addr), U256::from(700));

        // Try to subtract more than available
        assert!(!state.sub_balance(addr, U256::from(1000)));
        assert_eq!(state.get_balance(&addr), U256::from(700)); // Unchanged
    }

    #[test]
    fn test_transfer_balance() {
        let mut state = EvmState::new();
        let alice = Address::repeat_byte(1);
        let bob = Address::repeat_byte(2);

        // Give Alice some balance
        state.set_balance(alice, U256::from(1000));

        // Transfer from Alice to Bob
        assert!(state.transfer_balance(alice, bob, U256::from(400)));
        assert_eq!(state.get_balance(&alice), U256::from(600));
        assert_eq!(state.get_balance(&bob), U256::from(400));

        // Try to transfer more than Alice has
        assert!(!state.transfer_balance(alice, bob, U256::from(1000)));
        // Balances unchanged
        assert_eq!(state.get_balance(&alice), U256::from(600));
        assert_eq!(state.get_balance(&bob), U256::from(400));
    }

    #[test]
    fn test_unified_state_integration() {
        let mut state = EvmState::new();
        let addr = Address::repeat_byte(1);
        let account_addr = evm_to_account_address(&addr);

        // Set balance through EvmState
        state.set_balance(addr, U256::from(1000));

        // Verify it's in unified state's evm_view
        let unified = state.unified_state.read().unwrap();
        let balance = unified.get_balance(account_addr, NATIVE_TOKEN_INDEX).unwrap();
        assert_eq!(balance.evm_view.raw(), 1000);
        assert_eq!(balance.core_view.raw(), 0); // Not in core view
    }

    #[test]
    fn test_storage() {
        let mut state = EvmState::new();
        let addr = Address::ZERO;
        let key = U256::from(1);

        assert_eq!(state.get_storage(&addr, &key), U256::ZERO);

        state.set_storage(addr, key, U256::from(42));
        assert_eq!(state.get_storage(&addr, &key), U256::from(42));
    }

    #[test]
    fn test_code() {
        let mut state = EvmState::new();
        let addr = Address::ZERO;

        assert!(!state.is_contract(&addr));

        state.set_code(addr, vec![0x60, 0x00, 0x60, 0x00]);
        assert!(state.is_contract(&addr));
        assert!(state.get_code_hash(&addr).is_some());
    }

    #[test]
    fn test_snapshot_restore() {
        let mut state = EvmState::new();
        let addr = Address::ZERO;

        state.set_balance(addr, U256::from(100));
        let snapshot = state.snapshot();

        state.set_balance(addr, U256::from(200));
        assert_eq!(state.get_balance(&addr), U256::from(200));

        state.restore(snapshot);
        assert_eq!(state.get_balance(&addr), U256::from(100));
    }
}
