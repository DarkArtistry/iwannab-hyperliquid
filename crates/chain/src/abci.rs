//! ABCI (Application BlockChain Interface) service for CometBFT

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::tx::Transaction;

/// ABCI service handling CometBFT callbacks
pub struct AbciService {
    app: Arc<RwLock<HyperCoreApp>>,
}

impl AbciService {
    /// Create new ABCI service
    pub fn new(app: Arc<RwLock<HyperCoreApp>>) -> Self {
        Self { app }
    }

    /// Info request - returns application info
    pub async fn info(&self) -> InfoResponse {
        let app = self.app.read().await;
        InfoResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            app_version: 1,
            last_block_height: app.state.height as i64,
            last_block_app_hash: app.state.app_hash.to_vec(),
        }
    }

    /// Query request - handles ABCI queries
    pub async fn query(&self, request: QueryRequest) -> QueryResponse {
        let app = self.app.read().await;

        match request.path.as_str() {
            "/account" => {
                // Query account state
                match app.query_account(&request.data) {
                    Ok(data) => QueryResponse {
                        code: 0,
                        log: String::new(),
                        info: String::new(),
                        index: 0,
                        key: request.data,
                        value: data,
                        proof_ops: None,
                        height: app.state.height as i64,
                        codespace: String::new(),
                    },
                    Err(e) => QueryResponse {
                        code: 1,
                        log: e.to_string(),
                        info: String::new(),
                        index: 0,
                        key: request.data,
                        value: Vec::new(),
                        proof_ops: None,
                        height: app.state.height as i64,
                        codespace: "app".to_string(),
                    },
                }
            }
            "/position" => {
                match app.query_position(&request.data) {
                    Ok(data) => QueryResponse {
                        code: 0,
                        log: String::new(),
                        info: String::new(),
                        index: 0,
                        key: request.data,
                        value: data,
                        proof_ops: None,
                        height: app.state.height as i64,
                        codespace: String::new(),
                    },
                    Err(e) => QueryResponse {
                        code: 1,
                        log: e.to_string(),
                        ..Default::default()
                    },
                }
            }
            "/orderbook" => {
                match app.query_orderbook(&request.data) {
                    Ok(data) => QueryResponse {
                        code: 0,
                        value: data,
                        height: app.state.height as i64,
                        ..Default::default()
                    },
                    Err(e) => QueryResponse {
                        code: 1,
                        log: e.to_string(),
                        ..Default::default()
                    },
                }
            }
            _ => QueryResponse {
                code: 1,
                log: format!("Unknown query path: {}", request.path),
                ..Default::default()
            },
        }
    }

    /// CheckTx - validate transaction for mempool
    pub async fn check_tx(&self, request: CheckTxRequest) -> CheckTxResponse {
        let tx: Transaction = match serde_json::from_slice(&request.tx) {
            Ok(tx) => tx,
            Err(e) => {
                return CheckTxResponse {
                    code: 1,
                    data: Vec::new(),
                    log: format!("Failed to decode transaction: {}", e),
                    info: String::new(),
                    gas_wanted: 0,
                    gas_used: 0,
                    events: Vec::new(),
                    codespace: "encoding".to_string(),
                    sender: String::new(),
                    priority: 0,
                    mempool_error: String::new(),
                };
            }
        };

        let app = self.app.read().await;
        match app.check_tx(&tx) {
            Ok(()) => CheckTxResponse {
                code: 0,
                data: Vec::new(),
                log: "OK".to_string(),
                info: String::new(),
                gas_wanted: 1,
                gas_used: 1,
                events: Vec::new(),
                codespace: String::new(),
                sender: String::new(),
                priority: 1,
                mempool_error: String::new(),
            },
            Err(e) => CheckTxResponse {
                code: 1,
                data: Vec::new(),
                log: e.to_string(),
                info: String::new(),
                gas_wanted: 0,
                gas_used: 0,
                events: Vec::new(),
                codespace: "app".to_string(),
                sender: String::new(),
                priority: 0,
                mempool_error: String::new(),
            },
        }
    }

    /// InitChain - initialize chain with genesis state
    pub async fn init_chain(&self, request: InitChainRequest) -> InitChainResponse {
        let mut app = self.app.write().await;

        // Parse genesis app state
        if let Some(app_state) = request.app_state_bytes {
            if let Err(e) = app.init_from_genesis(&app_state) {
                tracing::error!("Failed to init from genesis: {}", e);
            }
        }

        // Set initial validators (simplified)
        InitChainResponse {
            consensus_params: request.consensus_params,
            validators: Vec::new(),
            app_hash: app.state.app_hash.to_vec(),
        }
    }

    /// PrepareProposal - prepare block proposal
    pub async fn prepare_proposal(&self, request: PrepareProposalRequest) -> PrepareProposalResponse {
        // For now, just return the transactions as-is
        // In production, we'd reorder/filter based on priority
        PrepareProposalResponse {
            txs: request.txs,
        }
    }

    /// ProcessProposal - validate block proposal
    pub async fn process_proposal(&self, request: ProcessProposalRequest) -> ProcessProposalResponse {
        // Validate all transactions in the proposal
        let app = self.app.read().await;

        for tx_bytes in &request.txs {
            let tx: Transaction = match serde_json::from_slice(tx_bytes) {
                Ok(tx) => tx,
                Err(_) => {
                    return ProcessProposalResponse {
                        status: ProposalStatus::Reject,
                    };
                }
            };

            if app.check_tx(&tx).is_err() {
                return ProcessProposalResponse {
                    status: ProposalStatus::Reject,
                };
            }
        }

        ProcessProposalResponse {
            status: ProposalStatus::Accept,
        }
    }

    /// FinalizeBlock - finalize block and return results
    pub async fn finalize_block(&self, request: FinalizeBlockRequest) -> FinalizeBlockResponse {
        let mut app = self.app.write().await;

        // Begin block
        app.begin_block(request.height as u64, request.time.unwrap_or(0) as u64);

        // Execute transactions
        let mut tx_results = Vec::with_capacity(request.txs.len());
        let mut events = Vec::new();

        for tx_bytes in request.txs {
            let tx: Transaction = match serde_json::from_slice(&tx_bytes) {
                Ok(tx) => tx,
                Err(e) => {
                    tx_results.push(ExecTxResult {
                        code: 1,
                        data: Vec::new(),
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

            let timestamp = request.time.unwrap_or(0) as u64;
            match app.execute_tx(&tx, timestamp) {
                Ok(tx_result) => {
                    let tx_events: Vec<Event> = tx_result.events.iter().map(|e| {
                        Event {
                            r#type: e.r#type.clone(),
                            attributes: e.attributes.iter().map(|a| {
                                EventAttribute {
                                    key: a.key.clone(),
                                    value: a.value.clone(),
                                    index: true,
                                }
                            }).collect(),
                        }
                    }).collect();
                    events.extend(tx_events.clone());
                    tx_results.push(ExecTxResult {
                        code: 0,
                        data: Vec::new(),
                        log: "OK".to_string(),
                        info: String::new(),
                        gas_wanted: 1,
                        gas_used: tx_result.gas_used as i64,
                        events: tx_events,
                        codespace: String::new(),
                    });
                }
                Err(e) => {
                    tx_results.push(ExecTxResult {
                        code: 1,
                        data: Vec::new(),
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

        // End block
        let validator_updates = app.end_block();

        // Commit and get app hash
        let app_hash = app.commit();

        FinalizeBlockResponse {
            events,
            tx_results,
            validator_updates: validator_updates.into_iter().map(|v| {
                crate::abci::ValidatorUpdate {
                    pub_key: v.pub_key,
                    power: v.power,
                }
            }).collect(),
            consensus_param_updates: None,
            app_hash: app_hash.to_vec(),
        }
    }

    /// Commit - persist state
    pub async fn commit(&self) -> CommitResponse {
        let mut app = self.app.write().await;
        app.commit();

        CommitResponse {
            retain_height: 0,
        }
    }

    /// ListSnapshots - list available state snapshots
    pub async fn list_snapshots(&self) -> ListSnapshotsResponse {
        // State sync not implemented yet
        ListSnapshotsResponse {
            snapshots: Vec::new(),
        }
    }

    /// OfferSnapshot - offer snapshot for state sync
    pub async fn offer_snapshot(&self, _request: OfferSnapshotRequest) -> OfferSnapshotResponse {
        OfferSnapshotResponse {
            result: OfferSnapshotResult::Reject,
        }
    }

    /// LoadSnapshotChunk - load snapshot chunk
    pub async fn load_snapshot_chunk(&self, _request: LoadSnapshotChunkRequest) -> LoadSnapshotChunkResponse {
        LoadSnapshotChunkResponse {
            chunk: Vec::new(),
        }
    }

    /// ApplySnapshotChunk - apply snapshot chunk
    pub async fn apply_snapshot_chunk(&self, _request: ApplySnapshotChunkRequest) -> ApplySnapshotChunkResponse {
        ApplySnapshotChunkResponse {
            result: ApplySnapshotChunkResult::Reject,
            refetch_chunks: Vec::new(),
            reject_senders: Vec::new(),
        }
    }
}

// ABCI Request/Response types (simplified)

#[derive(Debug, Clone, Default)]
pub struct InfoResponse {
    pub version: String,
    pub app_version: u64,
    pub last_block_height: i64,
    pub last_block_app_hash: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct QueryRequest {
    pub data: Vec<u8>,
    pub path: String,
    pub height: i64,
    pub prove: bool,
}

#[derive(Debug, Clone, Default)]
pub struct QueryResponse {
    pub code: u32,
    pub log: String,
    pub info: String,
    pub index: i64,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub proof_ops: Option<Vec<u8>>,
    pub height: i64,
    pub codespace: String,
}

#[derive(Debug, Clone)]
pub struct CheckTxRequest {
    pub tx: Vec<u8>,
    pub check_type: CheckTxType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTxType {
    New,
    Recheck,
}

#[derive(Debug, Clone, Default)]
pub struct CheckTxResponse {
    pub code: u32,
    pub data: Vec<u8>,
    pub log: String,
    pub info: String,
    pub gas_wanted: i64,
    pub gas_used: i64,
    pub events: Vec<Event>,
    pub codespace: String,
    pub sender: String,
    pub priority: i64,
    pub mempool_error: String,
}

#[derive(Debug, Clone)]
pub struct InitChainRequest {
    pub time: Option<i64>,
    pub chain_id: String,
    pub consensus_params: Option<ConsensusParams>,
    pub validators: Vec<ValidatorUpdate>,
    pub app_state_bytes: Option<Vec<u8>>,
    pub initial_height: i64,
}

#[derive(Debug, Clone, Default)]
pub struct InitChainResponse {
    pub consensus_params: Option<ConsensusParams>,
    pub validators: Vec<ValidatorUpdate>,
    pub app_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PrepareProposalRequest {
    pub max_tx_bytes: i64,
    pub txs: Vec<Vec<u8>>,
    pub local_last_commit: Option<ExtendedCommitInfo>,
    pub misbehavior: Vec<Misbehavior>,
    pub height: i64,
    pub time: Option<i64>,
    pub next_validators_hash: Vec<u8>,
    pub proposer_address: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PrepareProposalResponse {
    pub txs: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ProcessProposalRequest {
    pub txs: Vec<Vec<u8>>,
    pub proposed_last_commit: Option<CommitInfo>,
    pub misbehavior: Vec<Misbehavior>,
    pub hash: Vec<u8>,
    pub height: i64,
    pub time: Option<i64>,
    pub next_validators_hash: Vec<u8>,
    pub proposer_address: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ProcessProposalResponse {
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStatus {
    Unknown,
    Accept,
    Reject,
}

#[derive(Debug, Clone)]
pub struct FinalizeBlockRequest {
    pub txs: Vec<Vec<u8>>,
    pub decided_last_commit: Option<CommitInfo>,
    pub misbehavior: Vec<Misbehavior>,
    pub hash: Vec<u8>,
    pub height: i64,
    pub time: Option<i64>,
    pub next_validators_hash: Vec<u8>,
    pub proposer_address: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FinalizeBlockResponse {
    pub events: Vec<Event>,
    pub tx_results: Vec<ExecTxResult>,
    pub validator_updates: Vec<ValidatorUpdate>,
    pub consensus_param_updates: Option<ConsensusParams>,
    pub app_hash: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecTxResult {
    pub code: u32,
    pub data: Vec<u8>,
    pub log: String,
    pub info: String,
    pub gas_wanted: i64,
    pub gas_used: i64,
    pub events: Vec<Event>,
    pub codespace: String,
}

#[derive(Debug, Clone)]
pub struct CommitResponse {
    pub retain_height: i64,
}

#[derive(Debug, Clone)]
pub struct ListSnapshotsResponse {
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Clone)]
pub struct OfferSnapshotRequest {
    pub snapshot: Option<Snapshot>,
    pub app_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct OfferSnapshotResponse {
    pub result: OfferSnapshotResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferSnapshotResult {
    Unknown,
    Accept,
    Abort,
    Reject,
    RejectFormat,
    RejectSender,
}

#[derive(Debug, Clone)]
pub struct LoadSnapshotChunkRequest {
    pub height: u64,
    pub format: u32,
    pub chunk: u32,
}

#[derive(Debug, Clone)]
pub struct LoadSnapshotChunkResponse {
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ApplySnapshotChunkRequest {
    pub index: u32,
    pub chunk: Vec<u8>,
    pub sender: String,
}

#[derive(Debug, Clone)]
pub struct ApplySnapshotChunkResponse {
    pub result: ApplySnapshotChunkResult,
    pub refetch_chunks: Vec<u32>,
    pub reject_senders: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplySnapshotChunkResult {
    Unknown,
    Accept,
    Abort,
    Retry,
    RetrySnapshot,
    Reject,
}

// Supporting types

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub r#type: String,
    pub attributes: Vec<EventAttribute>,
}

impl Event {
    pub fn new(event_type: &str) -> Self {
        Self {
            r#type: event_type.to_string(),
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(mut self, key: &str, value: &str) -> Self {
        self.attributes.push(EventAttribute {
            key: key.to_string(),
            value: value.to_string(),
            index: true,
        });
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
    pub index: bool,
}

#[derive(Debug, Clone)]
pub struct ConsensusParams {
    pub block: Option<BlockParams>,
    pub evidence: Option<EvidenceParams>,
    pub validator: Option<ValidatorParams>,
    pub version: Option<VersionParams>,
}

#[derive(Debug, Clone)]
pub struct BlockParams {
    pub max_bytes: i64,
    pub max_gas: i64,
}

#[derive(Debug, Clone)]
pub struct EvidenceParams {
    pub max_age_num_blocks: i64,
    pub max_age_duration: i64,
    pub max_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct ValidatorParams {
    pub pub_key_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VersionParams {
    pub app: u64,
}

#[derive(Debug, Clone)]
pub struct ValidatorUpdate {
    pub pub_key: Vec<u8>,
    pub power: i64,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub height: u64,
    pub format: u32,
    pub chunks: u32,
    pub hash: Vec<u8>,
    pub metadata: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ExtendedCommitInfo {
    pub round: i32,
    pub votes: Vec<ExtendedVoteInfo>,
}

#[derive(Debug, Clone)]
pub struct ExtendedVoteInfo {
    pub validator: Validator,
    pub vote_extension: Vec<u8>,
    pub extension_signature: Vec<u8>,
    pub block_id_flag: i32,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub round: i32,
    pub votes: Vec<VoteInfo>,
}

#[derive(Debug, Clone)]
pub struct VoteInfo {
    pub validator: Validator,
    pub block_id_flag: i32,
}

#[derive(Debug, Clone)]
pub struct Validator {
    pub address: Vec<u8>,
    pub power: i64,
}

#[derive(Debug, Clone)]
pub struct Misbehavior {
    pub r#type: i32,
    pub validator: Validator,
    pub height: i64,
    pub time: i64,
    pub total_voting_power: i64,
}
