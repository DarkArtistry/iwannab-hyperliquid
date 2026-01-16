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
}

impl CometBftApp {
    /// Create a new CometBFT application wrapper
    pub fn new(app: HyperCoreApp) -> Self {
        Self {
            inner: Arc::new(RwLock::new(CometBftAppInner {
                app,
                validators: ValidatorSet::new(),
                chain_id: String::new(),
            })),
        }
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
    fn check_tx(&self, request: RequestCheckTx) -> ResponseCheckTx {
        let tx: Transaction = match serde_json::from_slice(&request.tx) {
            Ok(tx) => tx,
            Err(e) => {
                return ResponseCheckTx {
                    code: 1,
                    data: Vec::new().into(),
                    log: format!("Failed to decode transaction: {}", e),
                    info: String::new(),
                    gas_wanted: 0,
                    gas_used: 0,
                    events: Vec::new(),
                    codespace: "encoding".to_string(),
                };
            }
        };

        let inner = self.inner.read();
        match inner.app.check_tx(&tx) {
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
    fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
        let inner = self.inner.read();

        // Validate all transactions in the proposal
        for tx_bytes in &request.txs {
            let tx: Transaction = match serde_json::from_slice(tx_bytes) {
                Ok(tx) => tx,
                Err(_) => {
                    return ResponseProcessProposal {
                        status: ProposalStatus::Reject as i32,
                    };
                }
            };

            if inner.app.check_tx(&tx).is_err() {
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
            let tx: Transaction = match serde_json::from_slice(&tx_bytes) {
                Ok(tx) => tx,
                Err(e) => {
                    tx_results.push(tendermint_proto::abci::ExecTxResult {
                        code: 1,
                        data: Vec::new().into(),
                        log: format!("Failed to decode: {}", e),
                        info: String::new(),
                        gas_wanted: 0,
                        gas_used: 0,
                        events: Vec::new(),
                        codespace: "encoding".to_string(),
                    });
                    continue;
                }
            };

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
