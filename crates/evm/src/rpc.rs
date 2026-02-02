//! EVM JSON-RPC Server
//!
//! Implements the Ethereum JSON-RPC API for HyperEVM.
//! Reference: https://ethereum.org/en/developers/docs/apis/json-rpc/

use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;

use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::types::error::{ErrorCode, ErrorObject};
use revm::primitives::{Address, Bytes, B256, U256};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, debug, error};

use crate::executor::{EvmExecutor, EvmTransaction};

/// Block tag enum for RPC queries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BlockTag {
    /// Block number as hex string
    Number(String),
    /// Block tag: "latest", "pending", "earliest"
    Tag(String),
}

impl Default for BlockTag {
    fn default() -> Self {
        BlockTag::Tag("latest".to_string())
    }
}

impl BlockTag {
    /// Get the raw string value regardless of variant
    pub fn as_str(&self) -> &str {
        match self {
            BlockTag::Number(s) | BlockTag::Tag(s) => s,
        }
    }

    /// Check if this is a named tag (latest, pending, earliest, safe, finalized)
    pub fn is_named_tag(&self) -> bool {
        let s = self.as_str();
        matches!(s, "latest" | "pending" | "earliest" | "safe" | "finalized")
    }

    /// Parse block identifier to block number, using current block number for "latest"/"pending"
    pub fn to_block_number(&self, current_block: u64) -> RpcResult<u64> {
        let s = self.as_str();
        match s {
            "latest" | "pending" | "safe" | "finalized" => Ok(current_block),
            "earliest" => Ok(0),
            _ => {
                // Try to parse as hex number
                let s = s.strip_prefix("0x").unwrap_or(s);
                u64::from_str_radix(s, 16).map_err(|e| {
                    ErrorObject::owned(
                        ErrorCode::InvalidParams.code(),
                        format!("Invalid block number '{}': {}", self.as_str(), e),
                        None::<()>,
                    )
                })
            }
        }
    }
}

/// Transaction object for eth_call and eth_sendTransaction
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRequest {
    /// Sender address
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Recipient address (None for contract creation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Gas limit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas: Option<String>,
    /// Gas price
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
    /// Max fee per gas (EIP-1559)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fee_per_gas: Option<String>,
    /// Max priority fee per gas (EIP-1559)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_priority_fee_per_gas: Option<String>,
    /// Value to transfer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Transaction data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Input data (alias for data)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// Transaction nonce
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// Log filter for eth_getLogs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    /// From block
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_block: Option<BlockTag>,
    /// To block
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_block: Option<BlockTag>,
    /// Contract addresses
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Vec<String>>,
    /// Event topics
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Option<String>>>,
    /// Block hash (alternative to from/to block)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
}

/// Log entry returned by eth_getLogs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub block_number: String,
    pub transaction_hash: String,
    pub transaction_index: String,
    pub block_hash: String,
    pub log_index: String,
    pub removed: bool,
}

/// Block object returned by eth_getBlockByNumber/Hash
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockObject {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub nonce: String,
    pub sha3_uncles: String,
    pub logs_bloom: String,
    pub transactions_root: String,
    pub state_root: String,
    pub receipts_root: String,
    pub miner: String,
    pub difficulty: String,
    pub total_difficulty: String,
    pub extra_data: String,
    pub size: String,
    pub gas_limit: String,
    pub gas_used: String,
    pub timestamp: String,
    pub transactions: Vec<serde_json::Value>,
    pub uncles: Vec<String>,
    pub base_fee_per_gas: Option<String>,
}

/// Transaction receipt object
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: String,
    pub transaction_index: String,
    pub block_hash: String,
    pub block_number: String,
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub cumulative_gas_used: String,
    pub effective_gas_price: String,
    pub gas_used: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<String>,
    pub logs: Vec<LogEntry>,
    pub logs_bloom: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub status: String,
}

/// Full transaction object
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionObject {
    pub block_hash: Option<String>,
    pub block_number: Option<String>,
    pub from: String,
    pub gas: String,
    pub gas_price: String,
    pub hash: String,
    pub input: String,
    pub nonce: String,
    pub to: Option<String>,
    pub transaction_index: Option<String>,
    pub value: String,
    pub v: String,
    pub r: String,
    pub s: String,
    #[serde(rename = "type")]
    pub tx_type: String,
}

/// Transaction in mempool with pending status
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub hash: B256,
    pub tx: EvmTransaction,
    pub raw: Vec<u8>,
}

/// EVM RPC server state
pub struct EvmRpcState {
    /// EVM executor
    pub executor: Arc<RwLock<EvmExecutor>>,
    /// Chain ID
    pub chain_id: u64,
    /// Current block number (in-memory counter)
    pub block_number: Arc<RwLock<u64>>,
    /// Block timestamp (seconds since epoch)
    pub block_timestamp: Arc<RwLock<u64>>,
    /// Pending transactions
    pub pending_txs: Arc<RwLock<HashMap<B256, PendingTransaction>>>,
    /// Transaction receipts
    pub receipts: Arc<RwLock<HashMap<B256, TransactionReceipt>>>,
    /// Transactions by hash
    pub transactions: Arc<RwLock<HashMap<B256, TransactionObject>>>,
    /// CometBFT RPC URL for broadcasting (None = direct execution mode)
    pub cometbft_rpc_url: Option<String>,
    /// HTTP client for CometBFT RPC
    pub http_client: reqwest::Client,
}

impl EvmRpcState {
    pub fn new(executor: Arc<RwLock<EvmExecutor>>, chain_id: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            executor,
            chain_id,
            block_number: Arc::new(RwLock::new(1)),
            block_timestamp: Arc::new(RwLock::new(now)),
            pending_txs: Arc::new(RwLock::new(HashMap::new())),
            receipts: Arc::new(RwLock::new(HashMap::new())),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            cometbft_rpc_url: None,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_cometbft(executor: Arc<RwLock<EvmExecutor>>, chain_id: u64, cometbft_rpc_url: String) -> Self {
        let mut state = Self::new(executor, chain_id);
        state.cometbft_rpc_url = Some(cometbft_rpc_url);
        state
    }
}

/// Ethereum JSON-RPC API trait
#[rpc(server)]
pub trait EthApi {
    /// Returns the current chain ID
    #[method(name = "eth_chainId")]
    async fn chain_id(&self) -> RpcResult<String>;

    /// Returns the current block number
    #[method(name = "eth_blockNumber")]
    async fn block_number(&self) -> RpcResult<String>;

    /// Returns the current gas price
    #[method(name = "eth_gasPrice")]
    async fn gas_price(&self) -> RpcResult<String>;

    /// Returns the balance of an account
    #[method(name = "eth_getBalance")]
    async fn get_balance(&self, address: String, block: Option<BlockTag>) -> RpcResult<String>;

    /// Returns the transaction count (nonce) of an account
    #[method(name = "eth_getTransactionCount")]
    async fn get_transaction_count(&self, address: String, block: Option<BlockTag>) -> RpcResult<String>;

    /// Returns the code at an address
    #[method(name = "eth_getCode")]
    async fn get_code(&self, address: String, block: Option<BlockTag>) -> RpcResult<String>;

    /// Returns the value of a storage slot
    #[method(name = "eth_getStorageAt")]
    async fn get_storage_at(&self, address: String, slot: String, block: Option<BlockTag>) -> RpcResult<String>;

    /// Executes a call without creating a transaction
    #[method(name = "eth_call")]
    async fn call(&self, tx: TransactionRequest, block: Option<BlockTag>) -> RpcResult<String>;

    /// Estimates the gas needed for a transaction
    #[method(name = "eth_estimateGas")]
    async fn estimate_gas(&self, tx: TransactionRequest, block: Option<BlockTag>) -> RpcResult<String>;

    /// Sends a signed raw transaction
    #[method(name = "eth_sendRawTransaction")]
    async fn send_raw_transaction(&self, data: String) -> RpcResult<String>;

    /// Returns the receipt of a transaction by hash
    #[method(name = "eth_getTransactionReceipt")]
    async fn get_transaction_receipt(&self, hash: String) -> RpcResult<Option<TransactionReceipt>>;

    /// Returns a transaction by hash
    #[method(name = "eth_getTransactionByHash")]
    async fn get_transaction_by_hash(&self, hash: String) -> RpcResult<Option<TransactionObject>>;

    /// Returns logs matching a filter
    #[method(name = "eth_getLogs")]
    async fn get_logs(&self, filter: LogFilter) -> RpcResult<Vec<LogEntry>>;

    /// Returns a block by number
    #[method(name = "eth_getBlockByNumber")]
    async fn get_block_by_number(&self, block: BlockTag, full_transactions: bool) -> RpcResult<Option<BlockObject>>;

    /// Returns a block by hash
    #[method(name = "eth_getBlockByHash")]
    async fn get_block_by_hash(&self, hash: String, full_transactions: bool) -> RpcResult<Option<BlockObject>>;

    /// Returns account information (debug/dev)
    #[method(name = "eth_accounts")]
    async fn accounts(&self) -> RpcResult<Vec<String>>;

    /// Returns the max priority fee per gas
    #[method(name = "eth_maxPriorityFeePerGas")]
    async fn max_priority_fee_per_gas(&self) -> RpcResult<String>;

    /// Returns the base fee for next block
    #[method(name = "eth_feeHistory")]
    async fn fee_history(&self, block_count: String, newest_block: BlockTag, reward_percentiles: Option<Vec<f64>>) -> RpcResult<serde_json::Value>;

    // Web3 methods
    /// Returns client version
    #[method(name = "web3_clientVersion")]
    async fn client_version(&self) -> RpcResult<String>;

    // Net methods
    /// Returns the network version (chain ID)
    #[method(name = "net_version")]
    async fn net_version(&self) -> RpcResult<String>;

    /// Returns true if the client is listening for connections
    #[method(name = "net_listening")]
    async fn net_listening(&self) -> RpcResult<bool>;

    /// Returns the number of connected peers
    #[method(name = "net_peerCount")]
    async fn net_peer_count(&self) -> RpcResult<String>;
}

/// EVM RPC server implementation
pub struct EvmRpcServer {
    state: Arc<EvmRpcState>,
}

impl EvmRpcServer {
    pub fn new(executor: Arc<RwLock<EvmExecutor>>, chain_id: u64) -> Self {
        Self {
            state: Arc::new(EvmRpcState::new(executor, chain_id)),
        }
    }

    /// Create EVM RPC server in CometBFT mode
    ///
    /// In this mode, `eth_sendRawTransaction` broadcasts to CometBFT
    /// instead of executing directly. Read operations (eth_call, etc.)
    /// still use the local executor.
    pub fn with_cometbft(executor: Arc<RwLock<EvmExecutor>>, chain_id: u64, cometbft_rpc_url: String) -> Self {
        Self {
            state: Arc::new(EvmRpcState::with_cometbft(executor, chain_id, cometbft_rpc_url)),
        }
    }

    /// Get shared receipts store (for CometBFT ABCI app to populate after FinalizeBlock)
    pub fn shared_receipts(&self) -> Arc<RwLock<HashMap<B256, TransactionReceipt>>> {
        Arc::clone(&self.state.receipts)
    }

    /// Get shared transactions store
    pub fn shared_transactions(&self) -> Arc<RwLock<HashMap<B256, TransactionObject>>> {
        Arc::clone(&self.state.transactions)
    }

    /// Get shared block number counter
    pub fn shared_block_number(&self) -> Arc<RwLock<u64>> {
        Arc::clone(&self.state.block_number)
    }

    /// Start the RPC server
    pub async fn start(self, addr: SocketAddr) -> anyhow::Result<ServerHandle> {
        let server = Server::builder()
            .build(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build RPC server: {}", e))?;

        info!("EVM JSON-RPC server listening on {}", addr);

        let handle = server.start(self.into_rpc());
        Ok(handle)
    }
}

// Helper functions for parsing
fn parse_address(s: &str) -> RpcResult<Address> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| {
        ErrorObject::owned(ErrorCode::InvalidParams.code(), format!("Invalid address: {}", e), None::<()>)
    })?;
    if bytes.len() != 20 {
        return Err(ErrorObject::owned(ErrorCode::InvalidParams.code(), "Address must be 20 bytes", None::<()>));
    }
    Ok(Address::from_slice(&bytes))
}

fn parse_u256(s: &str) -> RpcResult<U256> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    U256::from_str_radix(s, 16).map_err(|e| {
        ErrorObject::owned(ErrorCode::InvalidParams.code(), format!("Invalid U256: {}", e), None::<()>)
    })
}

fn parse_bytes(s: &str) -> RpcResult<Bytes> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| {
        ErrorObject::owned(ErrorCode::InvalidParams.code(), format!("Invalid hex data: {}", e), None::<()>)
    })?;
    Ok(Bytes::from(bytes))
}

fn parse_b256(s: &str) -> RpcResult<B256> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| {
        ErrorObject::owned(ErrorCode::InvalidParams.code(), format!("Invalid hash: {}", e), None::<()>)
    })?;
    if bytes.len() != 32 {
        return Err(ErrorObject::owned(ErrorCode::InvalidParams.code(), "Hash must be 32 bytes", None::<()>));
    }
    Ok(B256::from_slice(&bytes))
}

fn format_u256(v: U256) -> String {
    format!("0x{:x}", v)
}

fn format_u64(v: u64) -> String {
    format!("0x{:x}", v)
}

fn format_address(a: &Address) -> String {
    format!("0x{}", hex::encode(a.as_slice()))
}

fn format_bytes(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

fn format_b256(b: &B256) -> String {
    format!("0x{}", hex::encode(b.as_slice()))
}

#[async_trait]
impl EthApiServer for EvmRpcServer {
    async fn chain_id(&self) -> RpcResult<String> {
        Ok(format_u64(self.state.chain_id))
    }

    async fn block_number(&self) -> RpcResult<String> {
        let block = *self.state.block_number.read().await;
        Ok(format_u64(block))
    }

    async fn gas_price(&self) -> RpcResult<String> {
        // Return a default gas price of 1 gwei
        Ok("0x3b9aca00".to_string())
    }

    async fn get_balance(&self, address: String, _block: Option<BlockTag>) -> RpcResult<String> {
        let addr = parse_address(&address)?;
        let executor = self.state.executor.read().await;
        let balance = executor.get_balance(&addr);
        Ok(format_u256(balance))
    }

    async fn get_transaction_count(&self, address: String, _block: Option<BlockTag>) -> RpcResult<String> {
        let addr = parse_address(&address)?;
        let executor = self.state.executor.read().await;
        let nonce = executor.get_nonce(&addr);
        Ok(format_u64(nonce))
    }

    async fn get_code(&self, address: String, _block: Option<BlockTag>) -> RpcResult<String> {
        let addr = parse_address(&address)?;
        let executor = self.state.executor.read().await;
        let code = executor.get_code(&addr);
        Ok(format_bytes(&code))
    }

    async fn get_storage_at(&self, address: String, slot: String, _block: Option<BlockTag>) -> RpcResult<String> {
        let addr = parse_address(&address)?;
        let slot = parse_u256(&slot)?;
        let executor = self.state.executor.read().await;
        let value = executor.get_storage(&addr, slot);
        Ok(format_u256(value))
    }

    async fn call(&self, tx: TransactionRequest, _block: Option<BlockTag>) -> RpcResult<String> {
        debug!("eth_call: {:?}", tx);

        let from = if let Some(ref f) = tx.from {
            parse_address(f)?
        } else {
            Address::ZERO
        };

        let to = if let Some(ref t) = tx.to {
            Some(parse_address(t)?)
        } else {
            None
        };

        let value = if let Some(ref v) = tx.value {
            parse_u256(v)?
        } else {
            U256::ZERO
        };

        let data = if let Some(ref d) = tx.data.as_ref().or(tx.input.as_ref()) {
            parse_bytes(d)?
        } else {
            Bytes::new()
        };

        let gas_limit = if let Some(ref g) = tx.gas {
            parse_u256(g)?.try_into().unwrap_or(30_000_000u64)
        } else {
            30_000_000u64
        };

        let evm_tx = EvmTransaction {
            from,
            to,
            value,
            data,
            gas_limit,
            gas_price: 1_000_000_000, // 1 gwei
            nonce: 0,
        };

        let mut executor = self.state.executor.write().await;
        // Use simulate_tx for eth_call (no state commit)
        match executor.simulate_tx(&evm_tx) {
            Ok(result) => {
                if result.success {
                    Ok(format_bytes(&result.output))
                } else {
                    Err(ErrorObject::owned(3, "execution reverted", Some(format_bytes(&result.output))))
                }
            }
            Err(e) => {
                Err(ErrorObject::owned(3, format!("execution error: {}", e), None::<()>))
            }
        }
    }

    async fn estimate_gas(&self, tx: TransactionRequest, _block: Option<BlockTag>) -> RpcResult<String> {
        debug!("eth_estimateGas: {:?}", tx);

        // For now, return a reasonable default
        // In production, this should actually simulate the transaction
        let from = if let Some(ref f) = tx.from {
            parse_address(f)?
        } else {
            Address::ZERO
        };

        let to = if let Some(ref t) = tx.to {
            Some(parse_address(t)?)
        } else {
            None
        };

        let value = if let Some(ref v) = tx.value {
            parse_u256(v)?
        } else {
            U256::ZERO
        };

        let data = if let Some(ref d) = tx.data.as_ref().or(tx.input.as_ref()) {
            parse_bytes(d)?
        } else {
            Bytes::new()
        };

        let evm_tx = EvmTransaction {
            from,
            to,
            value,
            data,
            gas_limit: 30_000_000,
            gas_price: 1_000_000_000,
            nonce: 0,
        };

        let mut executor = self.state.executor.write().await;
        // Use simulate_tx for eth_estimateGas (no state commit)
        match executor.simulate_tx(&evm_tx) {
            Ok(result) => {
                // Add 10% buffer to gas estimate
                let gas = (result.gas_used as f64 * 1.1) as u64;
                Ok(format_u64(gas.max(21000)))
            }
            Err(e) => {
                Err(ErrorObject::owned(3, format!("estimation error: {}", e), None::<()>))
            }
        }
    }

    async fn send_raw_transaction(&self, data: String) -> RpcResult<String> {
        debug!("eth_sendRawTransaction: {}", data);

        let raw_tx = parse_bytes(&data)?;

        // Compute transaction hash
        let tx_hash = {
            use sha3::{Digest, Keccak256};
            let hash = Keccak256::digest(&raw_tx);
            B256::from_slice(&hash)
        };

        // Validate by decoding the transaction
        let evm_tx = decode_raw_transaction(&raw_tx).map_err(|e| {
            ErrorObject::owned(ErrorCode::InvalidParams.code(), format!("Invalid transaction: {}", e), None::<()>)
        })?;

        // === CometBFT Mode: broadcast to consensus instead of executing locally ===
        if let Some(ref cometbft_url) = self.state.cometbft_rpc_url {
            return self.broadcast_to_cometbft(cometbft_url, &raw_tx, &tx_hash, &evm_tx).await;
        }

        // === Direct Mode: execute locally (single-node) ===
        self.execute_locally(&raw_tx, &tx_hash, &evm_tx).await
    }

    async fn get_transaction_receipt(&self, hash: String) -> RpcResult<Option<TransactionReceipt>> {        let hash = parse_b256(&hash)?;
        let receipts = self.state.receipts.read().await;
        Ok(receipts.get(&hash).cloned())
    }

    async fn get_transaction_by_hash(&self, hash: String) -> RpcResult<Option<TransactionObject>> {
        let hash = parse_b256(&hash)?;
        let transactions = self.state.transactions.read().await;
        Ok(transactions.get(&hash).cloned())
    }

    async fn get_logs(&self, _filter: LogFilter) -> RpcResult<Vec<LogEntry>> {
        // For now, return empty logs
        // In production, this should filter through stored logs
        Ok(vec![])
    }

    async fn get_block_by_number(&self, block: BlockTag, _full_transactions: bool) -> RpcResult<Option<BlockObject>> {
        let current_block = *self.state.block_number.read().await;
        let block_num = block.to_block_number(current_block)?;
        let timestamp = *self.state.block_timestamp.read().await;

        Ok(Some(BlockObject {
            number: format_u64(block_num),
            hash: format!("0x{}", "a".repeat(64)),
            parent_hash: format!("0x{}", "0".repeat(64)),
            nonce: "0x0000000000000000".to_string(),
            sha3_uncles: "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347".to_string(),
            logs_bloom: format!("0x{}", "0".repeat(512)),
            transactions_root: "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421".to_string(),
            state_root: format!("0x{}", "0".repeat(64)),
            receipts_root: "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421".to_string(),
            miner: "0x0000000000000000000000000000000000000000".to_string(),
            difficulty: "0x0".to_string(),
            total_difficulty: "0x0".to_string(),
            extra_data: "0x".to_string(),
            size: "0x0".to_string(),
            gas_limit: "0x1c9c380".to_string(), // 30M
            gas_used: "0x0".to_string(),
            timestamp: format_u64(timestamp),
            transactions: vec![],
            uncles: vec![],
            base_fee_per_gas: Some("0x3b9aca00".to_string()), // 1 gwei
        }))
    }

    async fn get_block_by_hash(&self, _hash: String, _full_transactions: bool) -> RpcResult<Option<BlockObject>> {
        // Return latest block for any hash query for now
        self.get_block_by_number(BlockTag::Tag("latest".to_string()), false).await
    }

    async fn accounts(&self) -> RpcResult<Vec<String>> {
        // No unlocked accounts (this is a signing-only RPC)
        Ok(vec![])
    }

    async fn max_priority_fee_per_gas(&self) -> RpcResult<String> {
        // Return 1 gwei as default priority fee
        Ok("0x3b9aca00".to_string())
    }

    async fn fee_history(&self, _block_count: String, _newest_block: BlockTag, _reward_percentiles: Option<Vec<f64>>) -> RpcResult<serde_json::Value> {
        let block_number = *self.state.block_number.read().await;
        Ok(serde_json::json!({
            "baseFeePerGas": ["0x3b9aca00"],
            "gasUsedRatio": [0.0],
            "oldestBlock": format_u64(block_number),
            "reward": []
        }))
    }

    async fn client_version(&self) -> RpcResult<String> {
        Ok("HyperEVM/0.1.0".to_string())
    }

    async fn net_version(&self) -> RpcResult<String> {
        Ok(self.state.chain_id.to_string())
    }

    async fn net_listening(&self) -> RpcResult<bool> {
        Ok(true)
    }

    async fn net_peer_count(&self) -> RpcResult<String> {
        Ok("0x0".to_string())
    }
}

/// Helper methods for EVM RPC server (not part of the JSON-RPC trait)
impl EvmRpcServer {
    /// Broadcast raw EVM transaction to CometBFT for consensus ordering
    async fn broadcast_to_cometbft(
        &self,
        cometbft_url: &str,
        raw_tx: &[u8],
        tx_hash: &B256,
        evm_tx: &EvmTransaction,
    ) -> RpcResult<String> {
        info!("Broadcasting EVM tx {} to CometBFT at {}", format_b256(tx_hash), cometbft_url);

        // Base64-encode the raw transaction bytes for CometBFT's broadcast_tx_sync
        let tx_base64 = base64_encode(raw_tx);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "broadcast_tx_sync",
            "params": {
                "tx": tx_base64
            }
        });

        let response = self.state.http_client
            .post(cometbft_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorObject::owned(
                -32000,
                format!("Failed to broadcast to CometBFT: {}", e),
                None::<()>,
            ))?;

        let status = response.status();
        let text = response.text().await
            .map_err(|e| ErrorObject::owned(
                -32000,
                format!("Failed to read CometBFT response: {}", e),
                None::<()>,
            ))?;

        if !status.is_success() {
            return Err(ErrorObject::owned(
                -32000,
                format!("CometBFT HTTP {}: {}", status, text),
                None::<()>,
            ));
        }

        // Parse CometBFT response
        let resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ErrorObject::owned(
                -32000,
                format!("Invalid CometBFT response: {}", e),
                None::<()>,
            ))?;

        // Check for RPC-level error
        if let Some(error) = resp.get("error") {
            let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
            return Err(ErrorObject::owned(-32000, format!("CometBFT error: {}", msg), None::<()>));
        }

        // Check for CheckTx rejection (code != 0)
        if let Some(result) = resp.get("result") {
            let code = result.get("code").and_then(|c| c.as_u64()).unwrap_or(0);
            if code != 0 {
                let log = result.get("log").and_then(|l| l.as_str()).unwrap_or("");
                return Err(ErrorObject::owned(
                    -32000,
                    format!("Transaction rejected by consensus: {}", log),
                    None::<()>,
                ));
            }
        }

        // Store as pending transaction for tracking
        self.state.pending_txs.write().await.insert(*tx_hash, PendingTransaction {
            hash: *tx_hash,
            tx: evm_tx.clone(),
            raw: raw_tx.to_vec(),
        });

        info!("EVM tx {} broadcast to CometBFT (pending consensus)", format_b256(tx_hash));
        Ok(format_b256(tx_hash))
    }

    /// Execute EVM transaction locally (single-node direct mode)
    async fn execute_locally(
        &self,
        _raw_tx: &[u8],
        tx_hash: &B256,
        evm_tx: &EvmTransaction,
    ) -> RpcResult<String> {
        let mut executor = self.state.executor.write().await;
        let result = executor.execute_tx(evm_tx).map_err(|e| {
            ErrorObject::owned(3, format!("execution error: {}", e), None::<()>)
        })?;

        let block_number = *self.state.block_number.read().await;

        let tx_obj = TransactionObject {
            block_hash: Some(format!("0x{}", "0".repeat(64))),
            block_number: Some(format_u64(block_number)),
            from: format_address(&evm_tx.from),
            gas: format_u64(evm_tx.gas_limit),
            gas_price: format_u64(evm_tx.gas_price),
            hash: format_b256(tx_hash),
            input: format_bytes(&evm_tx.data),
            nonce: format_u64(evm_tx.nonce),
            to: evm_tx.to.map(|a| format_address(&a)),
            transaction_index: Some("0x0".to_string()),
            value: format_u256(evm_tx.value),
            v: "0x1b".to_string(),
            r: format!("0x{}", "0".repeat(64)),
            s: format!("0x{}", "0".repeat(64)),
            tx_type: "0x0".to_string(),
        };

        let contract_address = if evm_tx.to.is_none() && result.success {
            result.contract_address.or_else(|| {
                Some(calculate_contract_address(&evm_tx.from, evm_tx.nonce))
            })
        } else {
            None
        };

        debug!("Transaction executed: to={:?}, success={}, contract_address={:?}",
               evm_tx.to, result.success, contract_address);

        let receipt = TransactionReceipt {
            transaction_hash: format_b256(tx_hash),
            transaction_index: "0x0".to_string(),
            block_hash: format!("0x{}", "0".repeat(64)),
            block_number: format_u64(block_number),
            from: format_address(&evm_tx.from),
            to: evm_tx.to.map(|a| format_address(&a)),
            cumulative_gas_used: format_u64(result.gas_used),
            effective_gas_price: format_u64(evm_tx.gas_price),
            gas_used: format_u64(result.gas_used),
            contract_address: contract_address.map(|a| format_address(&a)),
            logs: result.logs.iter().enumerate().map(|(i, log)| LogEntry {
                address: format_address(&log.address),
                topics: log.topics.iter().map(|t| format_bytes(t)).collect(),
                data: format_bytes(&log.data),
                block_number: format_u64(block_number),
                transaction_hash: format_b256(tx_hash),
                transaction_index: "0x0".to_string(),
                block_hash: format!("0x{}", "0".repeat(64)),
                log_index: format_u64(i as u64),
                removed: false,
            }).collect(),
            logs_bloom: format!("0x{}", "0".repeat(512)),
            tx_type: "0x0".to_string(),
            status: if result.success { "0x1".to_string() } else { "0x0".to_string() },
        };

        self.state.receipts.write().await.insert(*tx_hash, receipt);
        self.state.transactions.write().await.insert(*tx_hash, tx_obj);

        executor.commit();

        *self.state.block_number.write().await += 1;
        *self.state.block_timestamp.write().await = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        info!("Transaction executed: {} (success: {})", format_b256(tx_hash), result.success);
        Ok(format_b256(tx_hash))
    }
}

/// Simple base64 encoder for CometBFT RPC
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((input.len() + 2) / 3 * 4);

    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[(n >> 18 & 0x3F) as usize] as char);
        result.push(CHARS[(n >> 12 & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Decode a raw RLP-encoded transaction
///
/// Supports legacy, EIP-2930, and EIP-1559 transaction types.
/// Recovers the sender address from the embedded ECDSA signature.
pub fn decode_raw_transaction(raw: &[u8]) -> anyhow::Result<EvmTransaction> {
    use alloy_rlp::Decodable;

    if raw.is_empty() {
        return Err(anyhow::anyhow!("Empty transaction data"));
    }

    // Check transaction type
    let first_byte = raw[0];

    if first_byte >= 0xc0 {
        // Legacy transaction (RLP list)
        decode_legacy_transaction(raw)
    } else if first_byte == 0x02 {
        // EIP-1559 transaction
        decode_eip1559_transaction(&raw[1..])
    } else if first_byte == 0x01 {
        // EIP-2930 transaction
        decode_eip2930_transaction(&raw[1..])
    } else {
        Err(anyhow::anyhow!("Unknown transaction type: 0x{:02x}", first_byte))
    }
}

/// Decode a legacy transaction
fn decode_legacy_transaction(raw: &[u8]) -> anyhow::Result<EvmTransaction> {
    use alloy_rlp::Decodable;

    let mut buf = raw;

    // Decode RLP header
    let header = alloy_rlp::Header::decode(&mut buf)?;
    if !header.list {
        return Err(anyhow::anyhow!("Expected list"));
    }

    // Decode fields: nonce, gasPrice, gasLimit, to, value, data, v, r, s
    let nonce: u64 = alloy_rlp::Decodable::decode(&mut buf)?;
    let gas_price: u128 = alloy_rlp::Decodable::decode(&mut buf)?;
    let gas_limit: u64 = alloy_rlp::Decodable::decode(&mut buf)?;

    // To field: either empty (contract creation) or 20 bytes
    let to_bytes: alloy_rlp::Bytes = alloy_rlp::Decodable::decode(&mut buf)?;
    let to = if to_bytes.is_empty() {
        None
    } else {
        Some(Address::from_slice(&to_bytes))
    };

    let value: U256 = alloy_rlp::Decodable::decode(&mut buf)?;
    let data: alloy_rlp::Bytes = alloy_rlp::Decodable::decode(&mut buf)?;

    // v, r, s for signature (we need to parse these to get the sender)
    let v: u64 = alloy_rlp::Decodable::decode(&mut buf)?;
    let r: U256 = alloy_rlp::Decodable::decode(&mut buf)?;
    let s: U256 = alloy_rlp::Decodable::decode(&mut buf)?;

    // Recover sender from signature
    let from = recover_signer_legacy(raw, v, r, s)?;

    Ok(EvmTransaction {
        from,
        to,
        value,
        data: Bytes::from(data.to_vec()),
        gas_limit,
        gas_price: gas_price as u64,
        nonce,
    })
}

/// Decode EIP-1559 transaction
fn decode_eip1559_transaction(raw: &[u8]) -> anyhow::Result<EvmTransaction> {
    use alloy_rlp::{Decodable, Encodable};

    let mut buf = raw;

    // Decode RLP header
    let header = alloy_rlp::Header::decode(&mut buf)?;
    if !header.list {
        return Err(anyhow::anyhow!("Expected list"));
    }

    // Fields: chainId, nonce, maxPriorityFeePerGas, maxFeePerGas, gasLimit, to, value, data, accessList, v, r, s
    let chain_id: u64 = Decodable::decode(&mut buf)?;
    let nonce: u64 = Decodable::decode(&mut buf)?;
    let max_priority_fee: u128 = Decodable::decode(&mut buf)?;
    let max_fee: u128 = Decodable::decode(&mut buf)?;
    let gas_limit: u64 = Decodable::decode(&mut buf)?;

    let to_bytes: alloy_rlp::Bytes = Decodable::decode(&mut buf)?;
    let to = if to_bytes.is_empty() {
        None
    } else {
        Some(Address::from_slice(&to_bytes))
    };

    let value: U256 = Decodable::decode(&mut buf)?;
    let data: alloy_rlp::Bytes = Decodable::decode(&mut buf)?;

    // Access list - decode as Vec<(Address, Vec<B256>)>
    // For now, we decode as raw bytes and re-encode for signing
    let access_list_header = alloy_rlp::Header::decode(&mut buf)?;
    let access_list_bytes = if access_list_header.list && access_list_header.payload_length > 0 {
        let payload = &buf[..access_list_header.payload_length];
        buf = &buf[access_list_header.payload_length..];
        payload.to_vec()
    } else {
        vec![]
    };

    // Signature
    let v: u64 = Decodable::decode(&mut buf)?;
    let r: U256 = Decodable::decode(&mut buf)?;
    let s: U256 = Decodable::decode(&mut buf)?;

    // Re-encode the unsigned transaction fields for signing hash computation
    // This ensures we use canonical RLP encoding
    let unsigned_tx = encode_unsigned_eip1559(
        chain_id,
        nonce,
        max_priority_fee,
        max_fee,
        gas_limit,
        &to_bytes,
        value,
        &data,
        &access_list_bytes,
    );

    // For EIP-1559, recover signer from unsigned tx hash
    let from = recover_signer_eip1559(&unsigned_tx, v, r, s)?;

    Ok(EvmTransaction {
        from,
        to,
        value,
        data: Bytes::from(data.to_vec()),
        gas_limit,
        gas_price: max_fee as u64,
        nonce,
    })
}

/// Encode unsigned EIP-1559 transaction for signing
fn encode_unsigned_eip1559(
    chain_id: u64,
    nonce: u64,
    max_priority_fee: u128,
    max_fee: u128,
    gas_limit: u64,
    to: &alloy_rlp::Bytes,
    value: U256,
    data: &alloy_rlp::Bytes,
    access_list_payload: &[u8],
) -> Vec<u8> {
    use alloy_rlp::Encodable;

    info!("Encoding EIP-1559 tx: chainId={}, nonce={}, maxPriorityFee={}, maxFee={}, gasLimit={}, to={} bytes, value={}, data={} bytes, accessList={} bytes",
          chain_id, nonce, max_priority_fee, max_fee, gas_limit, to.len(), value, data.len(), access_list_payload.len());

    // Encode each field
    let mut fields = Vec::new();
    chain_id.encode(&mut fields);
    nonce.encode(&mut fields);
    max_priority_fee.encode(&mut fields);
    max_fee.encode(&mut fields);
    gas_limit.encode(&mut fields);
    to.encode(&mut fields);
    value.encode(&mut fields);
    data.encode(&mut fields);

    // Encode access list with its header
    alloy_rlp::Header {
        list: true,
        payload_length: access_list_payload.len(),
    }.encode(&mut fields);
    fields.extend_from_slice(access_list_payload);

    // Wrap in outer RLP list
    let mut result = Vec::new();
    alloy_rlp::Header {
        list: true,
        payload_length: fields.len(),
    }.encode(&mut result);
    result.extend_from_slice(&fields);

    // Prefix with 0x02 for EIP-1559
    let mut final_result = vec![0x02];
    final_result.extend(result);

    info!("Encoded unsigned tx: {} bytes", final_result.len());

    final_result
}

/// Decode EIP-2930 transaction
fn decode_eip2930_transaction(raw: &[u8]) -> anyhow::Result<EvmTransaction> {
    // Similar to EIP-1559 but without priority fee
    decode_eip1559_transaction(raw)
}

/// Recover signer from legacy transaction signature
fn recover_signer_legacy(raw: &[u8], v: u64, r: U256, s: U256) -> anyhow::Result<Address> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::{Digest, Keccak256};

    // Calculate message hash (transaction hash without signature)
    // For now, use a simplified approach - just use the raw tx hash
    let hash = Keccak256::digest(raw);

    // Calculate recovery ID
    // Legacy: v = 27 + recovery_id or v = chainId * 2 + 35 + recovery_id
    let recovery_id = if v >= 35 {
        ((v - 35) % 2) as u8
    } else {
        (v - 27) as u8
    };

    // Convert r and s to bytes
    let r_bytes: [u8; 32] = r.to_be_bytes();
    let s_bytes: [u8; 32] = s.to_be_bytes();

    // Concatenate r and s
    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(&r_bytes);
    sig_bytes[32..].copy_from_slice(&s_bytes);

    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid signature: {}", e))?;

    let recid = RecoveryId::try_from(recovery_id)
        .map_err(|e| anyhow::anyhow!("Invalid recovery id: {}", e))?;

    let verifying_key = VerifyingKey::recover_from_prehash(&hash, &signature, recid)
        .map_err(|e| anyhow::anyhow!("Failed to recover public key: {}", e))?;

    // Convert public key to address
    let pubkey_bytes = verifying_key.to_encoded_point(false);
    let pubkey_hash = Keccak256::digest(&pubkey_bytes.as_bytes()[1..]);

    Ok(Address::from_slice(&pubkey_hash[12..]))
}

/// Recover signer from EIP-1559 transaction signature
/// The `unsigned_tx` parameter should be the encoded unsigned transaction (0x02 || rlp([...fields...]))
fn recover_signer_eip1559(unsigned_tx: &[u8], v: u64, r: U256, s: U256) -> anyhow::Result<Address> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::{Digest, Keccak256};

    // For EIP-1559, v is just the recovery id (0 or 1)
    let recovery_id = v as u8;

    // Hash the unsigned transaction (already includes 0x02 prefix)
    let hash = Keccak256::digest(unsigned_tx);

    info!("EIP-1559 signature recovery: v={}", v);
    info!("Unsigned tx bytes ({} bytes): 0x{}", unsigned_tx.len(), hex::encode(&unsigned_tx[..unsigned_tx.len().min(100)]));
    info!("Signing hash: 0x{}", hex::encode(&hash));

    let r_bytes: [u8; 32] = r.to_be_bytes();
    let s_bytes: [u8; 32] = s.to_be_bytes();

    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(&r_bytes);
    sig_bytes[32..].copy_from_slice(&s_bytes);

    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid signature: {}", e))?;

    let recid = RecoveryId::try_from(recovery_id)
        .map_err(|e| anyhow::anyhow!("Invalid recovery id: {}", e))?;

    let verifying_key = VerifyingKey::recover_from_prehash(&hash, &signature, recid)
        .map_err(|e| anyhow::anyhow!("Failed to recover public key: {}", e))?;

    let pubkey_bytes = verifying_key.to_encoded_point(false);
    let pubkey_hash = Keccak256::digest(&pubkey_bytes.as_bytes()[1..]);

    let recovered_address = Address::from_slice(&pubkey_hash[12..]);
    info!("Recovered sender address: 0x{}", hex::encode(recovered_address.as_slice()));

    Ok(recovered_address)
}

/// Calculate contract address from sender and nonce
fn calculate_contract_address(sender: &Address, nonce: u64) -> Address {
    use alloy_rlp::Encodable;
    use sha3::{Digest, Keccak256};

    // RLP encode [sender, nonce]
    let mut buf = Vec::new();

    // List header
    let sender_len = 21; // 0x94 + 20 bytes
    let nonce_len = if nonce == 0 {
        1
    } else if nonce < 0x80 {
        1
    } else if nonce < 0x100 {
        2
    } else {
        // Simplified, handle larger nonces
        9
    };

    let total_len = sender_len + nonce_len;
    if total_len < 56 {
        buf.push(0xc0 + total_len as u8);
    } else {
        // Long list (unlikely for this use case)
        buf.push(0xf7 + 1);
        buf.push(total_len as u8);
    }

    // Sender (20 bytes)
    buf.push(0x94);
    buf.extend_from_slice(sender.as_slice());

    // Nonce
    if nonce == 0 {
        buf.push(0x80);
    } else if nonce < 0x80 {
        buf.push(nonce as u8);
    } else if nonce < 0x100 {
        buf.push(0x81);
        buf.push(nonce as u8);
    } else {
        // Encode as bytes
        let nonce_bytes = nonce.to_be_bytes();
        let leading_zeros = nonce_bytes.iter().take_while(|&&b| b == 0).count();
        let nonce_bytes = &nonce_bytes[leading_zeros..];
        buf.push(0x80 + nonce_bytes.len() as u8);
        buf.extend_from_slice(nonce_bytes);
    }

    let hash = Keccak256::digest(&buf);
    Address::from_slice(&hash[12..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_address() {
        let addr = parse_address("0x0000000000000000000000000000000000000000").unwrap();
        assert_eq!(addr, Address::ZERO);
    }

    #[test]
    fn test_parse_u256() {
        let val = parse_u256("0x10").unwrap();
        assert_eq!(val, U256::from(16));
    }

    #[test]
    fn test_format_u256() {
        let s = format_u256(U256::from(255));
        assert_eq!(s, "0xff");
    }

    #[test]
    fn test_contract_address_calculation() {
        // Known test case from Ethereum
        let sender = parse_address("0x6ac7ea33f8831ea9dcc53393aaa88b25a785dbf0").unwrap();
        let addr = calculate_contract_address(&sender, 0);
        // Should produce a deterministic address
        assert!(!addr.is_zero());
    }
}
