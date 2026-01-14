//! Transaction mempool for HyperCore chain

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use hypercore_primitives::AccountAddress;

use crate::tx::{Transaction, TransactionError, TxHash};

/// Maximum transactions per account in mempool
const MAX_TXS_PER_ACCOUNT: usize = 100;
/// Maximum total transactions in mempool
const MAX_MEMPOOL_SIZE: usize = 10_000;
/// Transaction expiry time
const TX_EXPIRY_SECS: u64 = 300;

/// Transaction mempool
pub struct Mempool {
    /// Transactions indexed by hash
    txs: HashMap<TxHash, MempoolEntry>,
    /// Transactions per account, ordered by nonce
    by_account: HashMap<AccountAddress, BTreeMap<u64, TxHash>>,
    /// Transactions ordered by arrival time for eviction
    by_time: BTreeMap<Instant, TxHash>,
    /// Known invalid transaction hashes
    invalid: HashSet<TxHash>,
    /// Configuration
    config: MempoolConfig,
}

/// Mempool entry with metadata
struct MempoolEntry {
    tx: Transaction,
    sender: AccountAddress,
    received_at: Instant,
}

/// Mempool configuration
pub struct MempoolConfig {
    pub max_txs_per_account: usize,
    pub max_total_txs: usize,
    pub expiry_duration: Duration,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_txs_per_account: MAX_TXS_PER_ACCOUNT,
            max_total_txs: MAX_MEMPOOL_SIZE,
            expiry_duration: Duration::from_secs(TX_EXPIRY_SECS),
        }
    }
}

impl Mempool {
    /// Create new mempool
    pub fn new() -> Self {
        Self::with_config(MempoolConfig::default())
    }

    /// Create mempool with custom config
    pub fn with_config(config: MempoolConfig) -> Self {
        Self {
            txs: HashMap::new(),
            by_account: HashMap::new(),
            by_time: BTreeMap::new(),
            invalid: HashSet::new(),
            config,
        }
    }

    /// Add transaction to mempool
    pub fn add(&mut self, mut tx: Transaction) -> Result<TxHash, MempoolError> {
        let hash = tx.hash();

        // Check if already known
        if self.txs.contains_key(&hash) {
            return Err(MempoolError::AlreadyExists);
        }

        // Check if known invalid
        if self.invalid.contains(&hash) {
            return Err(MempoolError::KnownInvalid);
        }

        // Recover sender
        let sender = tx.recover_signer().map_err(MempoolError::InvalidTransaction)?;

        // Check per-account limit first (read-only check)
        {
            let account_txs = self.by_account.get(&sender);
            if let Some(txs) = account_txs {
                if txs.len() >= self.config.max_txs_per_account {
                    return Err(MempoolError::AccountLimitReached);
                }
            }
        }

        // Check total limit - evict oldest if needed
        while self.txs.len() >= self.config.max_total_txs {
            if let Some((_, oldest_hash)) = self.by_time.pop_first() {
                self.remove_internal(&oldest_hash);
            } else {
                break;
            }
        }

        // Check for nonce gap (simple check)
        {
            let account_txs = self.by_account.get(&sender);
            if let Some(txs) = account_txs {
                if !txs.is_empty() {
                    let max_nonce = *txs.keys().last().unwrap();
                    if tx.nonce > max_nonce + 1 {
                        return Err(MempoolError::NonceTooHigh {
                            expected: max_nonce + 1,
                            got: tx.nonce,
                        });
                    }
                }
            }
        }

        // Insert
        let now = Instant::now();
        self.by_account.entry(sender).or_default().insert(tx.nonce, hash);
        self.by_time.insert(now, hash);
        self.txs.insert(hash, MempoolEntry {
            tx,
            sender,
            received_at: now,
        });

        Ok(hash)
    }

    /// Remove transaction by hash
    pub fn remove(&mut self, hash: &TxHash) -> Option<Transaction> {
        self.remove_internal(hash)
    }

    /// Add transaction with explicit sender (for testing)
    /// Bypasses signature verification.
    #[cfg(test)]
    pub fn add_with_sender(
        &mut self,
        mut tx: Transaction,
        sender: AccountAddress,
    ) -> Result<TxHash, MempoolError> {
        let hash = tx.hash();

        // Check if already known
        if self.txs.contains_key(&hash) {
            return Err(MempoolError::AlreadyExists);
        }

        // Check if known invalid
        if self.invalid.contains(&hash) {
            return Err(MempoolError::KnownInvalid);
        }

        // Check per-account limit
        if let Some(txs) = self.by_account.get(&sender) {
            if txs.len() >= self.config.max_txs_per_account {
                return Err(MempoolError::AccountLimitReached);
            }
        }

        // Check total limit - evict oldest if needed
        while self.txs.len() >= self.config.max_total_txs {
            if let Some((_, oldest_hash)) = self.by_time.pop_first() {
                self.remove_internal(&oldest_hash);
            } else {
                break;
            }
        }

        // Insert
        let now = Instant::now();
        self.by_account.entry(sender).or_default().insert(tx.nonce, hash);
        self.by_time.insert(now, hash);
        self.txs.insert(hash, MempoolEntry {
            tx,
            sender,
            received_at: now,
        });

        Ok(hash)
    }

    fn remove_internal(&mut self, hash: &TxHash) -> Option<Transaction> {
        if let Some(entry) = self.txs.remove(hash) {
            // Remove from account index
            if let Some(account_txs) = self.by_account.get_mut(&entry.sender) {
                account_txs.remove(&entry.tx.nonce);
                if account_txs.is_empty() {
                    self.by_account.remove(&entry.sender);
                }
            }

            // Remove from time index
            self.by_time.retain(|_, h| h != hash);

            Some(entry.tx)
        } else {
            None
        }
    }

    /// Get transaction by hash
    pub fn get(&self, hash: &TxHash) -> Option<&Transaction> {
        self.txs.get(hash).map(|e| &e.tx)
    }

    /// Check if transaction exists
    pub fn contains(&self, hash: &TxHash) -> bool {
        self.txs.contains_key(hash)
    }

    /// Get transactions ready for inclusion
    /// Returns transactions in nonce order for each account
    pub fn get_pending(&self, limit: usize) -> Vec<Transaction> {
        let mut result = Vec::with_capacity(limit);

        for (_sender, nonce_map) in &self.by_account {
            for (_nonce, hash) in nonce_map {
                if let Some(entry) = self.txs.get(hash) {
                    result.push(entry.tx.clone());
                    if result.len() >= limit {
                        return result;
                    }
                }
            }
        }

        result
    }

    /// Get transactions for a specific account
    pub fn get_account_pending(&self, address: &AccountAddress) -> Vec<Transaction> {
        let mut result = Vec::new();

        if let Some(nonce_map) = self.by_account.get(address) {
            for (_nonce, hash) in nonce_map {
                if let Some(entry) = self.txs.get(hash) {
                    result.push(entry.tx.clone());
                }
            }
        }

        result
    }

    /// Mark transaction as invalid (prevent re-adding)
    pub fn mark_invalid(&mut self, hash: TxHash) {
        self.remove(&hash);
        self.invalid.insert(hash);
    }

    /// Prune expired transactions
    pub fn prune_expired(&mut self) -> usize {
        let now = Instant::now();
        let cutoff = now - self.config.expiry_duration;
        let mut expired = Vec::new();

        for (time, hash) in &self.by_time {
            if *time < cutoff {
                expired.push(*hash);
            } else {
                break; // Ordered by time, so we can stop
            }
        }

        let count = expired.len();
        for hash in expired {
            self.remove_internal(&hash);
        }

        count
    }

    /// Update after block committed (remove included txs, update nonces)
    pub fn update_after_commit(&mut self, included_hashes: &[TxHash]) {
        for hash in included_hashes {
            self.remove(hash);
        }
    }

    /// Get mempool size
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    /// Check if mempool is empty
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    /// Get mempool stats
    pub fn stats(&self) -> MempoolStats {
        MempoolStats {
            total_txs: self.txs.len(),
            unique_senders: self.by_account.len(),
            invalid_count: self.invalid.len(),
        }
    }

    /// Clear invalid transaction cache
    pub fn clear_invalid_cache(&mut self) {
        self.invalid.clear();
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

/// Mempool errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum MempoolError {
    #[error("Transaction already exists in mempool")]
    AlreadyExists,
    #[error("Transaction is known invalid")]
    KnownInvalid,
    #[error("Account transaction limit reached")]
    AccountLimitReached,
    #[error("Mempool full")]
    MempoolFull,
    #[error("Nonce too high: expected {expected}, got {got}")]
    NonceTooHigh { expected: u64, got: u64 },
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(#[from] TransactionError),
}

/// Mempool statistics
#[derive(Debug, Clone, Default)]
pub struct MempoolStats {
    pub total_txs: usize,
    pub unique_senders: usize,
    pub invalid_count: usize,
}

/// Thread-safe mempool wrapper
pub struct SharedMempool {
    inner: Arc<RwLock<Mempool>>,
}

impl SharedMempool {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Mempool::new())),
        }
    }

    pub fn add(&self, tx: Transaction) -> Result<TxHash, MempoolError> {
        self.inner.write().unwrap().add(tx)
    }

    pub fn remove(&self, hash: &TxHash) -> Option<Transaction> {
        self.inner.write().unwrap().remove(hash)
    }

    pub fn get_pending(&self, limit: usize) -> Vec<Transaction> {
        self.inner.read().unwrap().get_pending(limit)
    }

    pub fn stats(&self) -> MempoolStats {
        self.inner.read().unwrap().stats()
    }

    /// Add transaction with explicit sender (for testing)
    #[cfg(test)]
    pub fn add_with_sender(
        &self,
        tx: Transaction,
        sender: AccountAddress,
    ) -> Result<TxHash, MempoolError> {
        self.inner.write().unwrap().add_with_sender(tx, sender)
    }
}

impl Default for SharedMempool {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SharedMempool {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::TransactionType;
    use hypercore_primitives::Signature;

    fn mock_tx(nonce: u64) -> Transaction {
        Transaction {
            action: TransactionType::CancelAll,
            nonce,
            signature: Signature::zero(),
            hash: None,
        }
    }

    #[test]
    fn test_mempool_basic() {
        let mempool = Mempool::new();
        assert!(mempool.is_empty());
        assert_eq!(mempool.len(), 0);
    }

    #[test]
    fn test_mempool_stats() {
        let mempool = Mempool::new();
        let stats = mempool.stats();
        assert_eq!(stats.total_txs, 0);
        assert_eq!(stats.unique_senders, 0);
    }
}
