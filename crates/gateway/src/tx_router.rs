//! Transaction routing for multi-node support
//!
//! In single-node mode (BlockProducer), transactions are executed directly
//! through the HyperCoreApp. In CometBFT multi-node mode, transactions
//! are broadcast to the local CometBFT node via its JSON-RPC endpoint,
//! which handles mempool gossip and consensus ordering.

use std::sync::Arc;

use hypercore_chain::HyperCoreApp;
use serde::Deserialize;
use tokio::sync::RwLock;

/// Transaction router - determines how transactions are executed
#[derive(Clone)]
pub enum TxRouter {
    /// Direct execution through HyperCoreApp (single-node/BlockProducer mode)
    Direct(Arc<RwLock<HyperCoreApp>>),
    /// Route through CometBFT consensus (multi-node mode)
    CometBft(CometBftRpcClient),
}

/// CometBFT JSON-RPC client for broadcasting transactions
///
/// Broadcasts transactions to the local CometBFT node's RPC endpoint.
/// CometBFT handles mempool validation (CheckTx), gossip to other validators,
/// consensus ordering, and block finalization (FinalizeBlock).
#[derive(Clone)]
pub struct CometBftRpcClient {
    /// CometBFT RPC URL (e.g., "http://cometbft-0:26657")
    url: String,
    /// HTTP client
    client: reqwest::Client,
}

/// Response from CometBFT broadcast_tx_sync
#[derive(Debug, Deserialize)]
pub struct BroadcastResponse {
    pub jsonrpc: String,
    pub id: i64,
    pub result: Option<BroadcastResult>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct BroadcastResult {
    pub code: u32,
    pub data: String,
    pub log: String,
    pub codespace: String,
    pub hash: String,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<String>,
}

/// Errors from transaction routing
#[derive(Debug, thiserror::Error)]
pub enum TxRouterError {
    #[error("CometBFT RPC error: {0}")]
    RpcError(String),
    #[error("Transaction rejected by CometBFT: code={code}, log={log}")]
    TxRejected { code: u32, log: String },
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl CometBftRpcClient {
    /// Create a new CometBFT RPC client
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }

    /// Broadcast a transaction synchronously via CometBFT RPC
    ///
    /// This submits the transaction to CometBFT's mempool and waits for
    /// CheckTx validation. The transaction will be included in a future
    /// block via consensus.
    ///
    /// Returns the transaction hash on success.
    pub async fn broadcast_tx_sync(&self, tx_bytes: &[u8]) -> Result<BroadcastResult, TxRouterError> {
        use serde_json::json;

        let tx_base64 = base64_encode(tx_bytes);

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "broadcast_tx_sync",
            "params": {
                "tx": tx_base64
            }
        });

        let response = self.client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TxRouterError::RpcError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let text = response.text().await
            .map_err(|e| TxRouterError::RpcError(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(TxRouterError::RpcError(format!(
                "HTTP {}: {}", status, text
            )));
        }

        let broadcast_response: BroadcastResponse = serde_json::from_str(&text)
            .map_err(|e| TxRouterError::RpcError(format!(
                "Failed to parse CometBFT response: {} (body: {})", e, text
            )))?;

        if let Some(error) = broadcast_response.error {
            return Err(TxRouterError::RpcError(format!(
                "CometBFT RPC error {}: {} ({})",
                error.code,
                error.message,
                error.data.unwrap_or_default()
            )));
        }

        match broadcast_response.result {
            Some(result) => {
                if result.code != 0 {
                    Err(TxRouterError::TxRejected {
                        code: result.code,
                        log: result.log,
                    })
                } else {
                    Ok(result)
                }
            }
            None => Err(TxRouterError::RpcError(
                "Empty result from CometBFT".to_string()
            )),
        }
    }

    /// Get the RPC URL
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Simple base64 encoder (avoids adding base64 crate dependency)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_cometbft_rpc_client_creation() {
        let client = CometBftRpcClient::new("http://localhost:26657".to_string());
        assert_eq!(client.url(), "http://localhost:26657");
    }

    #[test]
    fn test_tx_router_variants() {
        // Test CometBft variant
        let client = CometBftRpcClient::new("http://localhost:26657".to_string());
        let _router = TxRouter::CometBft(client);

        // Test Direct variant
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let _router = TxRouter::Direct(app);
    }
}
