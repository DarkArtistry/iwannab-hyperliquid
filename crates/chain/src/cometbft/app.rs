//! CometBFT Application Implementation
//!
//! Implements the `tendermint_abci::Application` trait to connect HyperCore to CometBFT.

use std::sync::Arc;
use parking_lot::RwLock;
use tendermint_abci::Application;
use tendermint_proto::abci::{
    RequestApplySnapshotChunk, RequestCheckTx, RequestEcho, RequestExtendVote,
    RequestFinalizeBlock, RequestInfo, RequestInitChain, RequestLoadSnapshotChunk,
    RequestOfferSnapshot, RequestPrepareProposal, RequestProcessProposal, RequestQuery,
    RequestVerifyVoteExtension, ResponseApplySnapshotChunk, ResponseCheckTx, ResponseCommit,
    ResponseEcho, ResponseExtendVote, ResponseFinalizeBlock, ResponseFlush, ResponseInfo,
    ResponseInitChain, ResponseListSnapshots, ResponseLoadSnapshotChunk, ResponseOfferSnapshot,
    ResponsePrepareProposal, ResponseProcessProposal, ResponseQuery, ResponseVerifyVoteExtension,
};
use tendermint_proto::abci::response_process_proposal::ProposalStatus;

use crate::app::HyperCoreApp;
use crate::tx::Transaction;
use super::validators::ValidatorSet;

use hypercore_evm::{EvmExecutor, decode_raw_transaction, B256, TransactionReceipt, TransactionObject, LogEntry};
use tokio::sync::RwLock as TokioRwLock;
use std::collections::HashMap;
use sha3::{Digest, Keccak256};

#[cfg(feature = "persistence")]
use hypercore_persistence::{RocksDbBackend, StatePersister};
#[cfg(feature = "persistence")]
use hypercore_persistence::{SnapshotManager, SnapshotMetadata, SnapshotRestore};
#[cfg(feature = "persistence")]
use crate::persistence_integration::extract_state;
#[cfg(feature = "persistence")]
use crate::persistence_integration::{restore_state, restore_perp_engine_state};

/// CometBFT Application wrapper
///
/// This wraps the HyperCoreApp and implements the tendermint_abci::Application trait.
/// The app must be Clone + Send + 'static as required by tendermint-abci.
#[derive(Clone)]
pub struct CometBftApp {
    /// Inner application state (wrapped in Arc<RwLock> for thread-safety)
    inner: Arc<RwLock<CometBftAppInner>>,
}

/// Inner application state (not Clone)
struct CometBftAppInner {
    /// The HyperCore application
    app: HyperCoreApp,
    /// Validator set management
    validators: ValidatorSet,
    /// Chain ID from InitChain
    chain_id: String,
    /// EVM executor for raw EVM transaction execution during consensus
    evm_executor: Option<Arc<TokioRwLock<EvmExecutor>>>,
    /// Shared EVM receipt store (populated by FinalizeBlock, read by EVM RPC)
    evm_receipts: Option<Arc<TokioRwLock<HashMap<B256, TransactionReceipt>>>>,
    /// Shared EVM transaction store
    evm_transactions: Option<Arc<TokioRwLock<HashMap<B256, TransactionObject>>>>,
    /// Shared EVM block number counter
    evm_block_number: Option<Arc<TokioRwLock<u64>>>,
    /// State persistence backend (populated when persistence feature enabled)
    #[cfg(feature = "persistence")]
    persistence: Option<Arc<RocksDbBackend>>,
    /// Pending state to persist on Commit (stored by FinalizeBlock, persisted by Commit)
    /// This ensures persistence happens AFTER CometBFT commits, not before.
    #[cfg(feature = "persistence")]
    pending_persistence: Option<hypercore_persistence::PersistedState>,
    /// Snapshot manager for creating and serving ABCI state sync snapshots
    #[cfg(feature = "persistence")]
    snapshot_manager: Option<Arc<parking_lot::Mutex<SnapshotManager>>>,
    /// Snapshot restore helper for receiving state sync snapshots
    #[cfg(feature = "persistence")]
    snapshot_restore: Option<parking_lot::Mutex<SnapshotRestore>>,
}

impl CometBftApp {
    /// Create a new CometBFT application wrapper
    pub fn new(app: HyperCoreApp) -> Self {
        Self {
            inner: Arc::new(RwLock::new(CometBftAppInner {
                app,
                validators: ValidatorSet::new(),
                chain_id: String::new(),
                evm_executor: None,
                evm_receipts: None,
                evm_transactions: None,
                evm_block_number: None,
                #[cfg(feature = "persistence")]
                persistence: None,
                #[cfg(feature = "persistence")]
                pending_persistence: None,
                #[cfg(feature = "persistence")]
                snapshot_manager: None,
                #[cfg(feature = "persistence")]
                snapshot_restore: None,
            })),
        }
    }

    /// Set the EVM executor for consensus-driven EVM transaction execution
    ///
    /// When set, the ABCI app can process raw EVM transactions that are
    /// broadcast directly (not wrapped in a HyperCore Transaction envelope).
    pub fn with_evm_executor(self, executor: Arc<TokioRwLock<EvmExecutor>>) -> Self {
        let mut inner = self.inner.write();
        // Set on CometBftAppInner for EVM tx execution in FinalizeBlock
        inner.evm_executor = Some(Arc::clone(&executor));
        // Also set on AppState so compute_app_hash() includes EVM state
        inner.app.state.set_evm_executor(executor);
        drop(inner);
        self
    }

    /// Set shared EVM receipt/transaction stores so FinalizeBlock can populate
    /// receipts that the EVM RPC server returns via eth_getTransactionReceipt.
    pub fn with_evm_receipt_store(
        self,
        receipts: Arc<TokioRwLock<HashMap<B256, TransactionReceipt>>>,
        transactions: Arc<TokioRwLock<HashMap<B256, TransactionObject>>>,
        block_number: Arc<TokioRwLock<u64>>,
    ) -> Self {
        let mut inner = self.inner.write();
        inner.evm_receipts = Some(receipts);
        inner.evm_transactions = Some(transactions);
        inner.evm_block_number = Some(block_number);
        drop(inner);
        self
    }

    /// Set the persistence backend for state persistence after each block
    #[cfg(feature = "persistence")]
    pub fn with_persistence(self, db: Arc<RocksDbBackend>) -> Self {
        self.inner.write().persistence = Some(db);
        self
    }

    /// Set the snapshot manager for ABCI state sync
    ///
    /// When set, the ABCI app can serve snapshots to new nodes via ListSnapshots /
    /// LoadSnapshotChunk, and accept snapshots from peers via OfferSnapshot /
    /// ApplySnapshotChunk. Periodic snapshot creation is triggered in Commit().
    #[cfg(feature = "persistence")]
    pub fn with_snapshot_manager(self, manager: SnapshotManager, restore_dir: std::path::PathBuf) -> Self {
        let mut inner = self.inner.write();
        inner.snapshot_manager = Some(Arc::new(parking_lot::Mutex::new(manager)));
        match SnapshotRestore::new(&restore_dir) {
            Ok(restore) => {
                inner.snapshot_restore = Some(parking_lot::Mutex::new(restore));
            }
            Err(e) => {
                tracing::warn!("Failed to create snapshot restore helper: {}", e);
            }
        }
        drop(inner);
        self
    }

    /// Get the current block height
    pub fn current_height(&self) -> u64 {
        self.inner.read().app.current_height()
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> String {
        self.inner.read().chain_id.clone()
    }

    /// Compute the current AppHash from state and compare against an expected value.
    /// Returns (matches, computed_hash).
    pub fn verify_app_hash(&self, expected: &[u8; 32]) -> (bool, [u8; 32]) {
        let inner = self.inner.read();
        let computed = inner.app.state.compute_app_hash();
        (computed == *expected, computed)
    }
}

impl Application for CometBftApp {
    /// Echo request - used for testing connectivity
    fn echo(&self, request: RequestEcho) -> ResponseEcho {
        ResponseEcho {
            message: request.message,
        }
    }

    /// Info request - returns application information
    ///
    /// CRITICAL: This is called by CometBFT on startup to determine the app's last committed state.
    /// The returned height and app_hash must match what CometBFT has in its state.db.
    fn info(&self, _request: RequestInfo) -> ResponseInfo {
        let inner = self.inner.read();
        let height = inner.app.current_height();
        let app_hash = inner.app.state.app_hash;

        // Recompute app_hash to verify it matches the stored value.
        // This helps diagnose state divergence issues on restart.
        let recomputed = inner.app.state.compute_app_hash();
        let matches = app_hash == recomputed;

        // Log full app_hash (not truncated) for debugging consensus divergence
        tracing::info!(
            "ABCI Info: height={}, app_hash={}, recomputed={}, matches={}, version={}",
            height,
            hex::encode(&app_hash),
            hex::encode(&recomputed),
            matches,
            env!("CARGO_PKG_VERSION"),
        );

        if !matches {
            tracing::error!(
                "CRITICAL: Stored app_hash does not match recomputed! \
                 stored={} recomputed={} height={}. \
                 This indicates state corruption or persistence issue.",
                hex::encode(&app_hash),
                hex::encode(&recomputed),
                height,
            );
        }

        ResponseInfo {
            data: "HyperCore".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            app_version: 1,
            last_block_height: height as i64,
            last_block_app_hash: app_hash.to_vec().into(),
        }
    }

    /// Query request - handles ABCI queries
    fn query(&self, request: RequestQuery) -> ResponseQuery {
        let inner = self.inner.read();

        match inner.app.query_account(&request.data) {
            Ok(data) => ResponseQuery {
                code: 0,
                log: String::new(),
                info: String::new(),
                index: 0,
                key: request.data,
                value: data.into(),
                proof_ops: None,
                height: inner.app.current_height() as i64,
                codespace: String::new(),
            },
            Err(e) => ResponseQuery {
                code: 1,
                log: e.to_string(),
                info: String::new(),
                index: 0,
                key: request.data,
                value: Vec::new().into(),
                proof_ops: None,
                height: inner.app.current_height() as i64,
                codespace: "app".to_string(),
            },
        }
    }

    /// CheckTx - validate transaction for mempool
    ///
    /// Handles two transaction formats:
    /// 1. JSON-encoded HyperCore Transaction (orders, cancels, transfers, etc.)
    /// 2. Raw RLP-encoded EVM transactions (from eth_sendRawTransaction)
    fn check_tx(&self, request: RequestCheckTx) -> ResponseCheckTx {
        // Try JSON-encoded HyperCore Transaction first
        if let Ok(tx) = serde_json::from_slice::<Transaction>(&request.tx) {
            let inner = self.inner.read();
            return match inner.app.check_tx(&tx) {
                Ok(()) => ResponseCheckTx {
                    code: 0,
                    data: Vec::new().into(),
                    log: "OK".to_string(),
                    info: String::new(),
                    gas_wanted: 1,
                    gas_used: 1,
                    events: Vec::new(),
                    codespace: String::new(),
                },
                Err(e) => ResponseCheckTx {
                    code: 1,
                    data: Vec::new().into(),
                    log: e.to_string(),
                    info: String::new(),
                    gas_wanted: 0,
                    gas_used: 0,
                    events: Vec::new(),
                    codespace: "app".to_string(),
                },
            };
        }

        // Try raw EVM transaction (RLP-encoded)
        match decode_raw_transaction(&request.tx) {
            Ok(evm_tx) => {
                tracing::debug!(
                    "CheckTx: raw EVM tx from {:?}, nonce={}, gas_limit={}",
                    evm_tx.from, evm_tx.nonce, evm_tx.gas_limit
                );
                // Basic validation: tx decoded successfully, sender recovered
                ResponseCheckTx {
                    code: 0,
                    data: Vec::new().into(),
                    log: "OK (EVM)".to_string(),
                    info: String::new(),
                    gas_wanted: evm_tx.gas_limit as i64,
                    gas_used: 0,
                    events: Vec::new(),
                    codespace: String::new(),
                }
            }
            Err(_) => {
                ResponseCheckTx {
                    code: 1,
                    data: Vec::new().into(),
                    log: "Failed to decode as HyperCore Transaction or raw EVM transaction".to_string(),
                    info: String::new(),
                    gas_wanted: 0,
                    gas_used: 0,
                    events: Vec::new(),
                    codespace: "encoding".to_string(),
                }
            }
        }
    }

    /// InitChain - initialize chain with genesis state
    fn init_chain(&self, request: RequestInitChain) -> ResponseInitChain {
        tracing::info!(
            "ABCI InitChain: chain_id={}, validators={}, app_state_bytes={}",
            request.chain_id,
            request.validators.len(),
            request.app_state_bytes.len(),
        );
        let mut inner = self.inner.write();

        // Store chain ID
        inner.chain_id = request.chain_id.clone();

        // Initialize validators from genesis
        for validator in &request.validators {
            if let Some(ref pub_key) = validator.pub_key {
                inner.validators.add_validator(
                    pub_key.sum.clone().map(|s| match s {
                        tendermint_proto::crypto::public_key::Sum::Ed25519(key) => key.to_vec(),
                        tendermint_proto::crypto::public_key::Sum::Secp256k1(key) => key.to_vec(),
                    }).unwrap_or_default(),
                    validator.power,
                );
            }
        }

        // Parse genesis app state
        if !request.app_state_bytes.is_empty() {
            if let Err(e) = inner.app.init_from_genesis(&request.app_state_bytes) {
                tracing::error!("Failed to init from genesis: {}", e);
            }
        }

        ResponseInitChain {
            consensus_params: request.consensus_params,
            validators: Vec::new(), // Use genesis validators as-is
            app_hash: inner.app.state.app_hash.to_vec().into(),
        }
    }

    /// PrepareProposal - prepare block proposal
    fn prepare_proposal(&self, request: RequestPrepareProposal) -> ResponsePrepareProposal {
        // For now, just return the transactions as-is
        // In production, we'd reorder/filter based on priority
        ResponsePrepareProposal {
            txs: request.txs,
        }
    }

    /// ProcessProposal - validate block proposal
    ///
    /// Accepts proposals containing valid HyperCore Transactions and/or raw EVM transactions.
    fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
        let inner = self.inner.read();

        for tx_bytes in &request.txs {
            // Try JSON-encoded HyperCore Transaction
            if let Ok(tx) = serde_json::from_slice::<Transaction>(tx_bytes) {
                if inner.app.check_tx(&tx).is_err() {
                    return ResponseProcessProposal {
                        status: ProposalStatus::Reject as i32,
                    };
                }
                continue;
            }

            // Try raw EVM transaction
            if decode_raw_transaction(tx_bytes).is_err() {
                return ResponseProcessProposal {
                    status: ProposalStatus::Reject as i32,
                };
            }
        }

        ResponseProcessProposal {
            status: ProposalStatus::Accept as i32,
        }
    }

    /// FinalizeBlock - finalize block and return results
    ///
    /// Handles two transaction formats:
    /// 1. JSON-encoded HyperCore Transaction (orders, cancels, transfers, etc.)
    /// 2. Raw RLP-encoded EVM transactions (from eth_sendRawTransaction)
    fn finalize_block(&self, request: RequestFinalizeBlock) -> ResponseFinalizeBlock {
        tracing::info!(
            "ABCI FinalizeBlock: height={}, txs={}",
            request.height,
            request.txs.len(),
        );
        let mut inner = self.inner.write();

        // Guard against duplicate FinalizeBlock at the same height.
        // This can happen if CometBFT replays the last block after a hash mismatch on restart.
        // Without this guard, re-executing on already-committed state corrupts the app_hash.
        if inner.app.current_height() == request.height as u64 {
            tracing::warn!(
                "FinalizeBlock: skipping duplicate height {} (already committed)",
                request.height
            );
            return ResponseFinalizeBlock {
                events: Vec::new(),
                tx_results: Vec::new(),
                validator_updates: Vec::new(),
                consensus_param_updates: None,
                app_hash: inner.app.state.app_hash.to_vec().into(),
            };
        }

        // Log CLOID state and next_order_id at start of block for debugging state divergence
        let cloid_count_start = inner.app.state.cloid_count();
        let next_order_id_start = inner.app.state.perp_engine
            .as_ref()
            .and_then(|pe| pe.try_read().ok())
            .map(|eng| eng.state.peek_next_order_id())
            .unwrap_or(0);
        tracing::debug!(
            "FinalizeBlock START: height={}, cloid_count={}, next_order_id={}",
            request.height,
            cloid_count_start,
            next_order_id_start
        );

        // Extract timestamp from the request (handle Option<Timestamp>)
        let timestamp = request.time.map(|t| t.seconds as u64 * 1000 + t.nanos as u64 / 1_000_000)
            .unwrap_or(0);

        // Begin block
        inner.app.begin_block(request.height as u64, timestamp);

        // Execute transactions
        let mut tx_results = Vec::with_capacity(request.txs.len());
        let mut events = Vec::new();

        // Process misbehavior evidence from CometBFT
        if !request.misbehavior.is_empty() {
            inner.app.state.last_evidence_count = request.misbehavior.len();
            inner.app.state.total_evidence_count += request.misbehavior.len();
            for evidence in &request.misbehavior {
                tracing::error!(
                    "MISBEHAVIOR EVIDENCE: type={:?}, height={}, total_voting_power={}",
                    evidence.r#type,
                    evidence.height,
                    evidence.total_voting_power,
                );
                events.push(tendermint_proto::abci::Event {
                    r#type: "misbehavior_evidence".to_string(),
                    attributes: vec![
                        tendermint_proto::abci::EventAttribute {
                            key: "evidence_type".to_string(),
                            value: format!("{:?}", evidence.r#type),
                            index: true,
                        },
                        tendermint_proto::abci::EventAttribute {
                            key: "evidence_height".to_string(),
                            value: evidence.height.to_string(),
                            index: true,
                        },
                    ],
                });
            }
        } else {
            inner.app.state.last_evidence_count = 0;
        }

        for tx_bytes in request.txs {
            // Try JSON-encoded HyperCore Transaction first
            if let Ok(tx) = serde_json::from_slice::<Transaction>(&tx_bytes) {
                match inner.app.execute_tx(&tx, timestamp) {
                    Ok(tx_result) => {
                        let tx_events: Vec<tendermint_proto::abci::Event> = tx_result.events.iter().map(|e| {
                            tendermint_proto::abci::Event {
                                r#type: e.r#type.clone(),
                                attributes: e.attributes.iter().map(|a| {
                                    tendermint_proto::abci::EventAttribute {
                                        key: a.key.clone(),
                                        value: a.value.clone(),
                                        index: true,
                                    }
                                }).collect(),
                            }
                        }).collect();
                        events.extend(tx_events.clone());
                        tx_results.push(tendermint_proto::abci::ExecTxResult {
                            code: 0,
                            data: Vec::new().into(),
                            log: "OK".to_string(),
                            info: String::new(),
                            gas_wanted: 1,
                            gas_used: tx_result.gas_used as i64,
                            events: tx_events,
                            codespace: String::new(),
                        });
                    }
                    Err(e) => {
                        tx_results.push(tendermint_proto::abci::ExecTxResult {
                            code: 1,
                            data: Vec::new().into(),
                            log: e.to_string(),
                            info: String::new(),
                            gas_wanted: 0,
                            gas_used: 0,
                            events: Vec::new(),
                            codespace: "app".to_string(),
                        });
                    }
                }
                continue;
            }

            // Try raw EVM transaction (RLP-encoded)
            match decode_raw_transaction(&tx_bytes) {
                Ok(evm_tx) => {
                    if let Some(ref executor) = inner.evm_executor {
                        // Use blocking_write() for deterministic consensus execution.
                        // This runs on a spawn_blocking thread, so blocking is safe.
                        // Critical: we MUST acquire this lock reliably. If we skip EVM
                        // transactions due to lock contention, the app_hash will diverge
                        // from other validators, breaking consensus.
                        let mut exec = executor.blocking_write();
                        match exec.execute_tx(&evm_tx) {
                            Ok(result) => {
                                exec.commit();

                                // Compute tx hash and store receipt for EVM RPC
                                let tx_hash = B256::from_slice(&Keccak256::digest(&tx_bytes));
                                let block_height = request.height as u64;
                                let block_hex = format!("0x{:x}", block_height);
                                let block_hash = format!("0x{}", "a".repeat(64));
                                let sender_hex = format!("0x{}", hex::encode(evm_tx.from.as_slice()));
                                let to_hex = evm_tx.to.map(|a| format!("0x{}", hex::encode(a.as_slice())));
                                let tx_hash_hex = format!("0x{}", hex::encode(tx_hash.as_slice()));

                                if let Some(ref receipts_store) = inner.evm_receipts {
                                    // Calculate contract address for deployments (to == None)
                                    let contract_address = if evm_tx.to.is_none() && result.success {
                                        Some(calculate_contract_address_hex(evm_tx.from.as_slice(), evm_tx.nonce))
                                    } else {
                                        None
                                    };
                                    let receipt = TransactionReceipt {
                                        transaction_hash: tx_hash_hex.clone(),
                                        transaction_index: "0x0".to_string(),
                                        block_hash: block_hash.clone(),
                                        block_number: block_hex.clone(),
                                        from: sender_hex.clone(),
                                        to: to_hex.clone(),
                                        cumulative_gas_used: format!("0x{:x}", result.gas_used),
                                        effective_gas_price: format!("0x{:x}", evm_tx.gas_price),
                                        gas_used: format!("0x{:x}", result.gas_used),
                                        contract_address,
                                        logs: result.logs.iter().enumerate().map(|(i, log)| LogEntry {
                                            address: format!("0x{}", hex::encode(log.address.as_slice())),
                                            topics: log.topics.iter().map(|t| format!("0x{}", hex::encode(t))).collect(),
                                            data: format!("0x{}", hex::encode(&log.data)),
                                            block_number: block_hex.clone(),
                                            transaction_hash: tx_hash_hex.clone(),
                                            transaction_index: "0x0".to_string(),
                                            block_hash: block_hash.clone(),
                                            log_index: format!("0x{:x}", i),
                                            removed: false,
                                        }).collect(),
                                        logs_bloom: format!("0x{}", "0".repeat(512)),
                                        tx_type: "0x2".to_string(),
                                        status: if result.success { "0x1".to_string() } else { "0x0".to_string() },
                                    };
                                    let mut store = receipts_store.blocking_write();
                                    store.insert(tx_hash, receipt);
                                }
                                if let Some(ref txs_store) = inner.evm_transactions {
                                    let tx_obj = TransactionObject {
                                        block_hash: Some(block_hash),
                                        block_number: Some(block_hex),
                                        from: sender_hex,
                                        gas: format!("0x{:x}", evm_tx.gas_limit),
                                        gas_price: format!("0x{:x}", evm_tx.gas_price),
                                        hash: tx_hash_hex,
                                        input: format!("0x{}", hex::encode(&evm_tx.data)),
                                        nonce: format!("0x{:x}", evm_tx.nonce),
                                        to: to_hex,
                                        transaction_index: Some("0x0".to_string()),
                                        value: format!("0x{:x}", evm_tx.value),
                                        v: "0x1".to_string(),
                                        r: format!("0x{}", "0".repeat(64)),
                                        s: format!("0x{}", "0".repeat(64)),
                                        tx_type: "0x2".to_string(),
                                    };
                                    let mut store = txs_store.blocking_write();
                                    store.insert(tx_hash, tx_obj);
                                }

                                let evm_event = tendermint_proto::abci::Event {
                                    r#type: "evm_raw_tx".to_string(),
                                    attributes: vec![
                                        tendermint_proto::abci::EventAttribute {
                                            key: "sender".to_string(),
                                            value: format!("{:?}", evm_tx.from),
                                            index: true,
                                        },
                                        tendermint_proto::abci::EventAttribute {
                                            key: "success".to_string(),
                                            value: result.success.to_string(),
                                            index: true,
                                        },
                                        tendermint_proto::abci::EventAttribute {
                                            key: "gas_used".to_string(),
                                            value: result.gas_used.to_string(),
                                            index: true,
                                        },
                                    ],
                                };
                                events.push(evm_event.clone());

                                tx_results.push(tendermint_proto::abci::ExecTxResult {
                                    code: if result.success { 0 } else { 1 },
                                    data: Vec::new().into(),
                                    log: if result.success { "OK (EVM)".to_string() } else { "EVM execution reverted".to_string() },
                                    info: String::new(),
                                    gas_wanted: evm_tx.gas_limit as i64,
                                    gas_used: result.gas_used as i64,
                                    events: vec![evm_event],
                                    codespace: String::new(),
                                });
                            }
                            Err(e) => {
                                tx_results.push(tendermint_proto::abci::ExecTxResult {
                                    code: 1,
                                    data: Vec::new().into(),
                                    log: format!("EVM execution error: {}", e),
                                    info: String::new(),
                                    gas_wanted: evm_tx.gas_limit as i64,
                                    gas_used: 0,
                                    events: Vec::new(),
                                    codespace: "evm".to_string(),
                                });
                            }
                        }
                    } else {
                        tx_results.push(tendermint_proto::abci::ExecTxResult {
                            code: 1,
                            data: Vec::new().into(),
                            log: "EVM executor not available".to_string(),
                            info: String::new(),
                            gas_wanted: 0,
                            gas_used: 0,
                            events: Vec::new(),
                            codespace: "evm".to_string(),
                        });
                    }
                }
                Err(_) => {
                    tx_results.push(tendermint_proto::abci::ExecTxResult {
                        code: 1,
                        data: Vec::new().into(),
                        log: "Failed to decode as HyperCore Transaction or raw EVM transaction".to_string(),
                        info: String::new(),
                        gas_wanted: 0,
                        gas_used: 0,
                        events: Vec::new(),
                        codespace: "encoding".to_string(),
                    });
                }
            }
        }

        // End block and get validator updates
        let app_validator_updates = inner.app.end_block();

        // Convert validator updates to tendermint format
        let validator_updates: Vec<tendermint_proto::abci::ValidatorUpdate> = app_validator_updates
            .into_iter()
            .map(|v| tendermint_proto::abci::ValidatorUpdate {
                pub_key: Some(tendermint_proto::crypto::PublicKey {
                    sum: Some(tendermint_proto::crypto::public_key::Sum::Ed25519(v.pub_key.into())),
                }),
                power: v.power,
            })
            .collect();

        // Update shared EVM block number to match CometBFT height
        if let Some(ref block_num) = inner.evm_block_number {
            let mut bn = block_num.blocking_write();
            *bn = request.height as u64;
        }

        // Log CLOID state before commit for debugging state divergence
        let cloid_count_end = inner.app.state.cloid_count();
        tracing::debug!(
            "FinalizeBlock END: height={}, cloid_count={} (delta={})",
            request.height,
            cloid_count_end,
            cloid_count_end as i64 - cloid_count_start as i64
        );

        // Commit and get app hash
        let app_hash = inner.app.commit();

        // Persist state to RocksDB after commit
        // extract_state() now handles perp_engine extraction directly when provided,
        // storing all trading state in perp.* fields with scalars mirrored to core.*.
        #[cfg(feature = "persistence")]
        if let Some(ref db) = inner.persistence {
            let mut persisted_state = extract_state(
                request.height as u64,
                timestamp,
                app_hash,
                &inner.app.state.unified_state,
                &inner.app.state.engine,
                inner.app.state.perp_engine.as_ref().map(|p| p as &_),
                inner.app.state.spot_engine.as_ref().map(|s| s as &_),
                inner.app.get_nonces(),
                inner.app.get_cloid_index(),
                inner.app.get_block_metadata(),
            );

            tracing::info!(
                "Persisting state at height {} with {} balances, {} perp orders, {} perp positions, next_order_id={}",
                request.height,
                persisted_state.core.balances.len(),
                persisted_state.perp.orders.len(),
                persisted_state.perp.positions.len(),
                persisted_state.perp.next_order_id,
            );

            // Extract EVM state for persistence
            // CRITICAL: Without this, restarted nodes have empty EVM state, causing
            // different evm_root in compute_app_hash() and consensus failure.
            if let Some(ref executor) = inner.evm_executor {
                use hypercore_persistence::{EvmAccountEntry, EvmStorageEntry, EvmCodeEntry, EvmBlockHashEntry};

                let evm = executor.blocking_read();
                let evm_state = evm.state();

                // Extract EVM accounts (address, nonce, code_hash)
                for (addr, account) in evm_state.get_all_accounts_data() {
                    persisted_state.evm.accounts.push(EvmAccountEntry {
                        address: addr.0 .0,
                        nonce: account.nonce,
                        code_hash: account.code_hash.map(|h| h.0),
                    });
                }

                // Extract EVM storage (address, slot, value)
                for (addr, slots) in evm_state.get_all_storage_entries() {
                    for (slot, value) in slots.iter() {
                        persisted_state.evm.storage.push(EvmStorageEntry {
                            address: addr.0 .0,
                            slot: slot.to_be_bytes(),
                            value: value.to_be_bytes(),
                        });
                    }
                }

                // Extract EVM code (code_hash, bytecode)
                for (code_hash, bytecode) in evm_state.get_all_code_entries() {
                    persisted_state.evm.code.push(EvmCodeEntry {
                        code_hash: code_hash.0,
                        bytecode: bytecode.clone(),
                    });
                }

                // Extract EVM block hashes
                for (height, hash) in evm_state.get_all_block_hashes() {
                    persisted_state.evm.block_hashes.push(EvmBlockHashEntry {
                        height: *height,
                        hash: hash.0,
                    });
                }

                tracing::debug!(
                    "EVM state extracted: {} accounts, {} storage slots, {} code entries",
                    persisted_state.evm.accounts.len(),
                    persisted_state.evm.storage.len(),
                    persisted_state.evm.code.len(),
                );
            }

            // Store state for persistence in Commit (not here!)
            // CRITICAL: Persisting in FinalizeBlock causes CometBFT restart panics.
            // CometBFT stores app_hash in state.db AFTER calling Commit.
            // If we persist in FinalizeBlock and the node crashes before Commit,
            // on restart the app has new state but CometBFT has old app_hash.
            tracing::debug!(
                "FinalizeBlock: prepared state for height {} ({} balances, app_hash={}), will persist in Commit",
                request.height,
                persisted_state.core.balances.len(),
                hex::encode(&app_hash[..8]),
            );
            inner.pending_persistence = Some(persisted_state);
        }
        #[cfg(feature = "persistence")]
        if inner.persistence.is_none() {
            if request.height == 1 {
                tracing::warn!("Persistence backend not set - state will not be persisted");
            }
        }

        ResponseFinalizeBlock {
            events,
            tx_results,
            validator_updates,
            consensus_param_updates: None,
            app_hash: app_hash.to_vec().into(),
        }
    }

    /// Flush - no-op for sync implementation
    fn flush(&self) -> ResponseFlush {
        ResponseFlush {}
    }

    /// Commit - persist state (called after FinalizeBlock in ABCI 2.0)
    /// CRITICAL: State persistence MUST happen here, not in FinalizeBlock.
    /// CometBFT stores app_hash in state.db AFTER Commit returns.
    /// If we persist in FinalizeBlock, a crash before Commit causes mismatch on restart.
    fn commit(&self) -> ResponseCommit {
        #[cfg(feature = "persistence")]
        {
            let mut inner = self.inner.write();
            if let Some(persisted_state) = inner.pending_persistence.take() {
                if let Some(ref db) = inner.persistence {
                    let persister = StatePersister::new(db.as_ref());
                    match persister.persist_state(&persisted_state) {
                        Ok(()) => {
                            tracing::info!(
                                "Commit: persisted state at height {} ({} balances, {} perp orders)",
                                persisted_state.height,
                                persisted_state.core.balances.len(),
                                persisted_state.perp.orders.len(),
                            );
                        }
                        Err(e) => {
                            tracing::error!("Commit: failed to persist state at height {}: {}", persisted_state.height, e);
                        }
                    }
                }
            }
        }
        // Create periodic snapshots for state sync
        #[cfg(feature = "persistence")]
        {
            let inner = self.inner.read();
            if let Some(ref manager) = inner.snapshot_manager {
                let height = inner.app.current_height();
                let mut mgr = manager.lock();
                if mgr.should_create_snapshot(height) {
                    if let Err(e) = mgr.create_snapshot(height) {
                        tracing::warn!("Failed to create snapshot at height {}: {}", height, e);
                    }
                }
            }
        }
        ResponseCommit {
            retain_height: 0,
        }
    }

    /// ListSnapshots - list available state snapshots for state sync
    fn list_snapshots(&self) -> ResponseListSnapshots {
        #[cfg(feature = "persistence")]
        {
            let inner = self.inner.read();
            if let Some(ref manager) = inner.snapshot_manager {
                let mgr = manager.lock();
                let snapshots = mgr.list_snapshots()
                    .into_iter()
                    .map(|meta| tendermint_proto::abci::Snapshot {
                        height: meta.height,
                        format: meta.format,
                        chunks: meta.chunks,
                        hash: meta.hash.to_vec().into(),
                        metadata: meta.metadata.as_bytes().to_vec().into(),
                    })
                    .collect();
                return ResponseListSnapshots { snapshots };
            }
        }
        ResponseListSnapshots {
            snapshots: Vec::new(),
        }
    }

    /// OfferSnapshot - decide whether to accept a snapshot from a peer
    fn offer_snapshot(&self, request: RequestOfferSnapshot) -> ResponseOfferSnapshot {
        #[cfg(feature = "persistence")]
        {
            let inner = self.inner.read();
            if let Some(ref restore) = inner.snapshot_restore {
                if let Some(ref snapshot) = request.snapshot {
                    let mut hash = [0u8; 32];
                    if snapshot.hash.len() == 32 {
                        hash.copy_from_slice(&snapshot.hash);
                    }
                    let metadata = SnapshotMetadata::new(
                        snapshot.height,
                        snapshot.format,
                        snapshot.chunks,
                        hash,
                    );
                    let mut restore_guard = restore.lock();
                    match restore_guard.offer_snapshot(metadata) {
                        Ok(true) => {
                            tracing::info!(
                                "Accepted snapshot at height {} ({} chunks)",
                                snapshot.height, snapshot.chunks
                            );
                            return ResponseOfferSnapshot { result: 1 }; // ACCEPT
                        }
                        Ok(false) => {
                            tracing::info!("Rejected snapshot at height {} (incompatible format)", snapshot.height);
                            return ResponseOfferSnapshot { result: 2 }; // ABORT
                        }
                        Err(e) => {
                            tracing::warn!("Error processing snapshot offer: {}", e);
                            return ResponseOfferSnapshot { result: 3 }; // REJECT
                        }
                    }
                }
            }
        }
        ResponseOfferSnapshot {
            result: 3, // REJECT
        }
    }

    /// LoadSnapshotChunk - serve a chunk of our snapshot to a syncing peer
    fn load_snapshot_chunk(&self, request: RequestLoadSnapshotChunk) -> ResponseLoadSnapshotChunk {
        #[cfg(feature = "persistence")]
        {
            let inner = self.inner.read();
            if let Some(ref manager) = inner.snapshot_manager {
                let mgr = manager.lock();
                match mgr.load_chunk(request.height, request.chunk) {
                    Ok(chunk) => {
                        tracing::debug!(
                            "Serving snapshot chunk {} for height {}: {} bytes",
                            request.chunk, request.height, chunk.len()
                        );
                        return ResponseLoadSnapshotChunk {
                            chunk: chunk.into(),
                        };
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load snapshot chunk {} for height {}: {}",
                            request.chunk, request.height, e
                        );
                    }
                }
            }
        }
        ResponseLoadSnapshotChunk {
            chunk: Vec::new().into(),
        }
    }

    /// ApplySnapshotChunk - receive a chunk from a peer and restore state when complete
    fn apply_snapshot_chunk(&self, request: RequestApplySnapshotChunk) -> ResponseApplySnapshotChunk {
        #[cfg(feature = "persistence")]
        {
            let inner = self.inner.read();
            if let Some(ref restore) = inner.snapshot_restore {
                let mut restore_guard = restore.lock();
                match restore_guard.apply_chunk(request.index, request.chunk.to_vec()) {
                    Ok(true) => {
                        // More chunks needed
                        let (received, total) = restore_guard.progress();
                        tracing::debug!(
                            "Applied snapshot chunk {}/{}", received, total
                        );
                        return ResponseApplySnapshotChunk {
                            result: 1, // ACCEPT
                            refetch_chunks: Vec::new(),
                            reject_senders: Vec::new(),
                        };
                    }
                    Ok(false) => {
                        // All chunks received — finalize
                        tracing::info!("All snapshot chunks received, finalizing...");
                        match restore_guard.finalize() {
                            Ok(persisted_state) => {
                                tracing::info!(
                                    "Snapshot restored: height={}, {} balances, {} positions",
                                    persisted_state.height,
                                    persisted_state.core.balances.len(),
                                    persisted_state.perp.positions.len(),
                                );
                                // Drop restore_guard and inner before acquiring write lock
                                drop(restore_guard);
                                drop(inner);

                                // Restore state into the running app
                                let mut inner = self.inner.write();
                                let unified_state = &inner.app.state.unified_state;
                                let engine = &inner.app.state.engine;
                                let spot_engine = inner.app.state.spot_engine.as_ref().map(|s| s as &_);

                                match restore_state(
                                    &persisted_state,
                                    unified_state,
                                    engine,
                                    spot_engine,
                                ) {
                                    Ok((height, timestamp, app_hash)) => {
                                        // Restore chain-level state
                                        inner.app.state.restore_chain_state(height, timestamp, app_hash);

                                        // Restore perp engine if available
                                        if let Some(ref perp) = inner.app.state.perp_engine {
                                            restore_perp_engine_state(&persisted_state, perp);
                                        }

                                        tracing::info!(
                                            "State sync complete: height={}, app_hash={}",
                                            height, hex::encode(&app_hash[..8])
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to restore state from snapshot: {}", e);
                                        return ResponseApplySnapshotChunk {
                                            result: 3, // REJECT
                                            refetch_chunks: Vec::new(),
                                            reject_senders: Vec::new(),
                                        };
                                    }
                                }

                                return ResponseApplySnapshotChunk {
                                    result: 1, // ACCEPT
                                    refetch_chunks: Vec::new(),
                                    reject_senders: Vec::new(),
                                };
                            }
                            Err(e) => {
                                tracing::error!("Failed to finalize snapshot: {}", e);
                                return ResponseApplySnapshotChunk {
                                    result: 3, // REJECT
                                    refetch_chunks: Vec::new(),
                                    reject_senders: Vec::new(),
                                };
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to apply snapshot chunk: {}", e);
                        return ResponseApplySnapshotChunk {
                            result: 2, // RETRY
                            refetch_chunks: vec![request.index],
                            reject_senders: Vec::new(),
                        };
                    }
                }
            }
        }
        ResponseApplySnapshotChunk {
            result: 3, // REJECT (no snapshot restore configured)
            refetch_chunks: Vec::new(),
            reject_senders: Vec::new(),
        }
    }

    /// ExtendVote - vote extension (ABCI 2.0)
    fn extend_vote(&self, _request: RequestExtendVote) -> ResponseExtendVote {
        ResponseExtendVote {
            vote_extension: Vec::new().into(),
        }
    }

    /// VerifyVoteExtension - verify vote extension (ABCI 2.0)
    fn verify_vote_extension(&self, _request: RequestVerifyVoteExtension) -> ResponseVerifyVoteExtension {
        // Accept = 1
        ResponseVerifyVoteExtension {
            status: 1, // ACCEPT
        }
    }
}

/// Calculate contract address from sender and nonce (CREATE opcode).
/// Returns the 20-byte contract address as a hex string "0x..."
fn calculate_contract_address_hex(sender_bytes: &[u8], nonce: u64) -> String {
    let mut buf = Vec::new();

    // RLP: [sender (20 bytes), nonce]
    let sender_len = 21; // 0x94 + 20 bytes
    let nonce_len = if nonce == 0 { 1 } else if nonce < 0x80 { 1 } else if nonce < 0x100 { 2 } else { 9 };
    let total_len = sender_len + nonce_len;

    if total_len < 56 {
        buf.push(0xc0 + total_len as u8);
    } else {
        buf.push(0xf7 + 1);
        buf.push(total_len as u8);
    }

    buf.push(0x94);
    buf.extend_from_slice(sender_bytes);

    if nonce == 0 {
        buf.push(0x80);
    } else if nonce < 0x80 {
        buf.push(nonce as u8);
    } else if nonce < 0x100 {
        buf.push(0x81);
        buf.push(nonce as u8);
    } else {
        let nonce_bytes = nonce.to_be_bytes();
        let leading_zeros = nonce_bytes.iter().take_while(|&&b| b == 0).count();
        let nonce_bytes = &nonce_bytes[leading_zeros..];
        buf.push(0x80 + nonce_bytes.len() as u8);
        buf.extend_from_slice(nonce_bytes);
    }

    let hash = Keccak256::digest(&buf);
    format!("0x{}", hex::encode(&hash[12..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cometbft_app_creation() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);
        assert_eq!(cometbft_app.current_height(), 0);
    }

    #[test]
    fn test_echo() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);

        let request = RequestEcho {
            message: "test".to_string(),
        };
        let response = cometbft_app.echo(request);
        assert_eq!(response.message, "test");
    }

    #[test]
    fn test_info() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);

        let request = RequestInfo {
            version: String::new(),
            block_version: 0,
            p2p_version: 0,
            abci_version: String::new(),
        };
        let response = cometbft_app.info(request);
        assert_eq!(response.data, "HyperCore");
        assert_eq!(response.last_block_height, 0);
    }

    #[test]
    fn test_list_snapshots_returns_empty_without_manager() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);

        let response = cometbft_app.list_snapshots();
        assert!(response.snapshots.is_empty());
    }

    #[test]
    fn test_offer_snapshot_rejects_without_restore() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);

        let request = RequestOfferSnapshot {
            snapshot: Some(tendermint_proto::abci::Snapshot {
                height: 1000,
                format: 1,
                chunks: 3,
                hash: vec![0u8; 32].into(),
                metadata: Vec::new().into(),
            }),
            app_hash: Vec::new().into(),
        };
        let response = cometbft_app.offer_snapshot(request);
        assert_eq!(response.result, 3); // REJECT (no restore configured)
    }

    #[test]
    fn test_apply_snapshot_chunk_rejects_without_restore() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);

        let request = RequestApplySnapshotChunk {
            index: 0,
            chunk: vec![1, 2, 3].into(),
            sender: String::new(),
        };
        let response = cometbft_app.apply_snapshot_chunk(request);
        assert_eq!(response.result, 3); // REJECT (no restore configured)
    }
}
