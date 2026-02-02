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

use hypercore_evm::{EvmExecutor, decode_raw_transaction, B256, TransactionReceipt, TransactionObject};
use tokio::sync::RwLock as TokioRwLock;
use std::collections::HashMap;
use sha3::{Digest, Keccak256};

/// Acquire a write lock on a tokio RwLock with retry (for sync/blocking contexts).
///
/// In FinalizeBlock (which runs on a spawn_blocking thread), blocking_write() can
/// deadlock if an EVM RPC handler concurrently holds a read lock. This helper uses
/// try_write() with short sleeps, matching the pattern in app.rs for perp/spot engines.
fn acquire_tokio_write_with_retry<'a, T>(lock: &'a TokioRwLock<T>, label: &str) -> Option<tokio::sync::RwLockWriteGuard<'a, T>> {
    if let Ok(guard) = lock.try_write() {
        return Some(guard);
    }
    for attempt in 1..=100 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        if let Ok(guard) = lock.try_write() {
            if attempt > 5 {
                tracing::debug!("{}: acquired write lock after {} retries", label, attempt);
            }
            return Some(guard);
        }
    }
    tracing::error!("{}: failed to acquire write lock after 100 retries", label);
    None
}

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
            })),
        }
    }

    /// Set the EVM executor for consensus-driven EVM transaction execution
    ///
    /// When set, the ABCI app can process raw EVM transactions that are
    /// broadcast directly (not wrapped in a HyperCore Transaction envelope).
    pub fn with_evm_executor(self, executor: Arc<TokioRwLock<EvmExecutor>>) -> Self {
        self.inner.write().evm_executor = Some(executor);
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

    /// Get the current block height
    pub fn current_height(&self) -> u64 {
        self.inner.read().app.current_height()
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> String {
        self.inner.read().chain_id.clone()
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
    fn info(&self, _request: RequestInfo) -> ResponseInfo {
        let inner = self.inner.read();
        ResponseInfo {
            data: "HyperCore".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            app_version: 1,
            last_block_height: inner.app.current_height() as i64,
            last_block_app_hash: inner.app.state.app_hash.to_vec().into(),
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
        let mut inner = self.inner.write();

        // Extract timestamp from the request (handle Option<Timestamp>)
        let timestamp = request.time.map(|t| t.seconds as u64 * 1000 + t.nanos as u64 / 1_000_000)
            .unwrap_or(0);

        // Begin block
        inner.app.begin_block(request.height as u64, timestamp);

        // Execute transactions
        let mut tx_results = Vec::with_capacity(request.txs.len());
        let mut events = Vec::new();

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
                        // Acquire write lock with retry to avoid deadlock with EVM RPC readers
                        let Some(mut exec) = acquire_tokio_write_with_retry(executor, "EVM executor (FinalizeBlock)") else {
                            tx_results.push(tendermint_proto::abci::ExecTxResult {
                                code: 1,
                                data: Vec::new().into(),
                                log: "EVM executor lock contention (100 retries exhausted)".to_string(),
                                info: String::new(),
                                gas_wanted: evm_tx.gas_limit as i64,
                                gas_used: 0,
                                events: Vec::new(),
                                codespace: "evm".to_string(),
                            });
                            continue;
                        };
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
                                        logs: vec![],
                                        logs_bloom: format!("0x{}", "0".repeat(512)),
                                        tx_type: "0x2".to_string(),
                                        status: if result.success { "0x1".to_string() } else { "0x0".to_string() },
                                    };
                                    if let Some(mut store) = acquire_tokio_write_with_retry(receipts_store, "EVM receipts store") {
                                        store.insert(tx_hash, receipt);
                                    }
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
                                    if let Some(mut store) = acquire_tokio_write_with_retry(txs_store, "EVM transactions store") {
                                        store.insert(tx_hash, tx_obj);
                                    }
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
            if let Some(mut bn) = acquire_tokio_write_with_retry(block_num, "EVM block number") {
                *bn = request.height as u64;
            }
        }

        // Commit and get app hash
        let app_hash = inner.app.commit();

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
    fn commit(&self) -> ResponseCommit {
        ResponseCommit {
            retain_height: 0,
        }
    }

    /// ListSnapshots - list available state snapshots
    fn list_snapshots(&self) -> ResponseListSnapshots {
        // State sync not implemented yet
        ResponseListSnapshots {
            snapshots: Vec::new(),
        }
    }

    /// OfferSnapshot - offer snapshot for state sync
    fn offer_snapshot(&self, _request: RequestOfferSnapshot) -> ResponseOfferSnapshot {
        // Reject = 3 in the proto
        ResponseOfferSnapshot {
            result: 3, // REJECT
        }
    }

    /// LoadSnapshotChunk - load snapshot chunk
    fn load_snapshot_chunk(&self, _request: RequestLoadSnapshotChunk) -> ResponseLoadSnapshotChunk {
        ResponseLoadSnapshotChunk {
            chunk: Vec::new().into(),
        }
    }

    /// ApplySnapshotChunk - apply snapshot chunk
    fn apply_snapshot_chunk(&self, _request: RequestApplySnapshotChunk) -> ResponseApplySnapshotChunk {
        // Reject = 5 in the proto
        ResponseApplySnapshotChunk {
            result: 5, // REJECT
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
}
