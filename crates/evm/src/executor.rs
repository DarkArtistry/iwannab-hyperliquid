//! EVM executor for HyperEVM
//!
//! Integrates revm for actual EVM bytecode execution with HyperCore precompiles.

use std::sync::Arc;
use std::collections::HashMap;

use hypercore_engine::EngineState;
use revm::primitives::{
    Address, Bytes, U256, B256,
    AccountInfo, Bytecode, ExecutionResult, Output,
    TransactTo, Env, TxEnv, BlockEnv, CfgEnv, PrecompileOutput,
};
use revm::interpreter::analysis::to_analysed;
use revm::{Database, DatabaseCommit, Evm};
use sha3::{Digest, Keccak256};
use tokio::sync::RwLock;
use tracing::debug;

use crate::precompiles::{HyperCorePrecompiles, PrecompileAddress};
use crate::state::EvmState;

/// CoreWriter contract address
pub const CORE_WRITER_ADDRESS: Address = Address::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x00,
]);

/// Fee collector address - receives gas fees from EVM transactions
/// This is a system address that accumulates gas fees.
/// In production, fees could be redistributed to validators or burned.
pub const FEE_COLLECTOR_ADDRESS: Address = Address::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFE, 0xE1,
]);

/// EVM transaction
#[derive(Debug, Clone)]
pub struct EvmTransaction {
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Bytes,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub nonce: u64,
}

/// Execution result
#[derive(Debug, Clone)]
pub struct EvmExecutionResult {
    pub success: bool,
    pub gas_used: u64,
    pub output: Bytes,
    pub logs: Vec<EvmLog>,
    pub contract_address: Option<Address>,
}

/// EVM log
#[derive(Debug, Clone)]
pub struct EvmLog {
    pub address: Address,
    pub topics: Vec<[u8; 32]>,
    pub data: Bytes,
}

/// EVM executor error
#[derive(Debug, Clone, thiserror::Error)]
pub enum EvmExecutorError {
    #[error("Execution reverted: {0}")]
    Reverted(String),
    #[error("Out of gas")]
    OutOfGas,
    #[error("Insufficient balance for gas: need {required}, have {available}")]
    InsufficientGasBalance { required: String, available: String },
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Custom database wrapper that combines EvmState with HyperCore precompile data
pub struct HyperEvmDb {
    /// The EVM state (accounts, storage, code)
    state: EvmState,
}

impl HyperEvmDb {
    pub fn new(state: EvmState) -> Self {
        Self { state }
    }

    pub fn inner(&self) -> &EvmState {
        &self.state
    }

    pub fn inner_mut(&mut self) -> &mut EvmState {
        &mut self.state
    }
}

impl Database for HyperEvmDb {
    type Error = String;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Get balance from unified state (Phase 2A)
        let balance = self.state.get_balance(&address);

        // Check if account has metadata (nonce, code)
        if let Some(account) = self.state.get_account(&address) {
            let stored_code_hash = account.code_hash.unwrap_or(B256::ZERO);

            // Load code directly if this is a contract
            let code = if stored_code_hash != B256::ZERO {
                if let Some(code_bytes) = self.state.get_code_by_hash(&stored_code_hash) {
                    // Create raw bytecode and analyze it for proper JUMPDEST handling
                    let raw = Bytecode::new_raw(Bytes::from(code_bytes.clone()));
                    let analyzed = to_analysed(raw);
                    Some(analyzed)
                } else {
                    None
                }
            } else {
                None
            };

            Ok(Some(AccountInfo {
                balance,
                nonce: account.nonce,
                code_hash: stored_code_hash,
                code,
            }))
        } else if !balance.is_zero() {
            // Account has balance in unified state but no metadata - it's an EOA
            Ok(Some(AccountInfo {
                balance,
                nonce: 0,
                code_hash: B256::ZERO,
                code: None,
            }))
        } else {
            Ok(None)
        }
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if code_hash == B256::ZERO {
            return Ok(Bytecode::default());
        }

        // Look up code by hash from our state and analyze it
        if let Some(code) = self.state.get_code_by_hash(&code_hash) {
            let raw = Bytecode::new_raw(Bytes::from(code.clone()));
            let analyzed = to_analysed(raw);
            Ok(analyzed)
        } else {
            Ok(Bytecode::default())
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        Ok(self.state.get_storage(&address, &index))
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        Ok(self.state.get_block_hash(number).unwrap_or(B256::ZERO))
    }
}

impl DatabaseCommit for HyperEvmDb {
    fn commit(&mut self, changes: HashMap<Address, revm::primitives::Account>) {
        for (address, account) in changes {
            // Update account info
            if account.is_selfdestructed() {
                self.state.delete_account(&address);
                continue;
            }

            // Update balance and nonce
            self.state.set_balance(address, account.info.balance);
            let acct = self.state.get_or_create_account(address);
            acct.nonce = account.info.nonce;

            // Update code if present (for contract creation or code changes)
            // Note: code can be Some with non-zero hash for new contracts
            if let Some(ref code) = account.info.code {
                let code_bytes = code.original_bytes();
                if !code_bytes.is_empty() {
                    self.state.set_code(address, code_bytes.to_vec());
                }
            }

            // Update storage - store all values including zeros (for proper state tracking)
            for (slot, value) in account.storage {
                self.state.set_storage(address, slot, value.present_value);
            }
        }
    }
}

/// EVM executor with HyperCore integration
pub struct EvmExecutor {
    /// EVM database
    db: HyperEvmDb,
    /// HyperCore engine state reference (for precompiles)
    engine: Arc<RwLock<EngineState>>,
    /// HyperCore precompiles
    precompiles: HyperCorePrecompiles,
    /// Chain ID
    chain_id: u64,
    /// Block gas limit
    block_gas_limit: u64,
    /// Current block number
    block_number: u64,
    /// Current block timestamp
    block_timestamp: u64,
    /// Whether to enforce gas fees (Phase 3B)
    /// When true, transactions require sufficient evm_view balance for gas
    /// When false, gas fees are not charged (development mode)
    enforce_gas_fees: bool,
}

impl EvmExecutor {
    /// Create new executor
    pub fn new(engine: Arc<RwLock<EngineState>>, chain_id: u64) -> Self {
        let precompiles = HyperCorePrecompiles::new(Arc::clone(&engine));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            db: HyperEvmDb::new(EvmState::new()),
            engine,
            precompiles,
            chain_id,
            block_gas_limit: 30_000_000,
            block_number: 1,
            block_timestamp: now,
            enforce_gas_fees: false, // Disabled by default for backward compatibility
        }
    }

    /// Create new executor with shared unified state (Phase 2A)
    ///
    /// This allows the EVM to share the same balance sheet as the HyperCore spot engine.
    pub fn with_unified_state(
        engine: Arc<RwLock<EngineState>>,
        unified_state: hypercore_primitives::SharedUnifiedState,
        chain_id: u64,
    ) -> Self {
        let precompiles = HyperCorePrecompiles::new(Arc::clone(&engine));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            db: HyperEvmDb::new(EvmState::with_unified_state(unified_state)),
            engine,
            precompiles,
            chain_id,
            block_gas_limit: 30_000_000,
            block_number: 1,
            block_timestamp: now,
            enforce_gas_fees: false, // Disabled by default for backward compatibility
        }
    }

    /// Enable or disable gas fee enforcement (Phase 3B)
    ///
    /// When enabled:
    /// - Pre-flight check: sender must have evm_view >= gas_limit * gas_price + value
    /// - Post-execution: gas_used * gas_price is deducted from sender and credited to fee collector
    ///
    /// When disabled (default):
    /// - No gas balance checks
    /// - No gas fee deductions
    pub fn set_enforce_gas_fees(&mut self, enforce: bool) {
        self.enforce_gas_fees = enforce;
    }

    /// Check if gas fee enforcement is enabled
    pub fn is_gas_fees_enforced(&self) -> bool {
        self.enforce_gas_fees
    }

    /// Execute a transaction
    ///
    /// If `commit_state` is true, state changes will be persisted (for eth_sendRawTransaction).
    /// If false, this is a read-only simulation (for eth_call and eth_estimateGas).
    pub fn execute_tx(&mut self, tx: &EvmTransaction) -> Result<EvmExecutionResult, EvmExecutorError> {
        self.execute_tx_inner(tx, true)
    }

    /// Execute a transaction in simulation mode (no state commit)
    /// Used for eth_call and eth_estimateGas
    pub fn simulate_tx(&mut self, tx: &EvmTransaction) -> Result<EvmExecutionResult, EvmExecutorError> {
        self.execute_tx_inner(tx, false)
    }

    /// Internal transaction execution
    ///
    /// Phase 3B: When `enforce_gas_fees` is true and `commit_state` is true:
    /// - Pre-flight check: sender must have evm_view >= gas_limit * gas_price + value
    /// - Post-execution: gas_used * gas_price is deducted from sender's evm_view
    /// - Gas fees are credited to the fee collector
    fn execute_tx_inner(&mut self, tx: &EvmTransaction, commit_state: bool) -> Result<EvmExecutionResult, EvmExecutorError> {
        debug!("Executing tx from {:?} to {:?} (commit={}, enforce_gas={})",
               tx.from, tx.to, commit_state, self.enforce_gas_fees);

        // Phase 3B: Pre-flight gas balance check
        // Only for actual transactions (commit_state=true) when gas fees are enforced
        let max_gas_cost = if self.enforce_gas_fees && commit_state {
            let max_cost = U256::from(tx.gas_limit) * U256::from(tx.gas_price);
            let total_required = max_cost + tx.value;
            let sender_balance = self.db.state.get_balance(&tx.from);

            if sender_balance < total_required {
                return Err(EvmExecutorError::InsufficientGasBalance {
                    required: format!("{}", total_required),
                    available: format!("{}", sender_balance),
                });
            }
            debug!("Gas pre-flight passed: required={}, balance={}", total_required, sender_balance);
            Some(max_cost)
        } else {
            None
        };

        // Debug: Log if we're calling a contract and whether it has code
        if let Some(to) = tx.to {
            let code = self.db.state.get_code(&to);
            if let Some(c) = code {
                debug!("Target address {:?} has code: {} bytes, first 100: 0x{}",
                       to, c.len(), hex::encode(&c[..c.len().min(100)]));
            } else {
                debug!("Target address {:?} has NO code", to);
            }
        }

        // Check if this is a precompile call
        if let Some(to) = tx.to {
            if HyperCorePrecompiles::is_precompile(&to) {
                let result = self.execute_precompile(tx, to)?;
                // Apply gas fees for precompile calls too
                if let Some(_max_cost) = max_gas_cost {
                    self.apply_gas_fee(tx.from, result.gas_used, tx.gas_price);
                }
                return Ok(result);
            }
        }

        // Build EVM environment
        let mut env = Env::default();

        // Configure chain
        env.cfg = CfgEnv::default();
        env.cfg.chain_id = self.chain_id;

        // Block environment - set coinbase to fee collector
        env.block = BlockEnv {
            number: U256::from(self.block_number),
            timestamp: U256::from(self.block_timestamp),
            gas_limit: U256::from(self.block_gas_limit),
            coinbase: FEE_COLLECTOR_ADDRESS, // Gas fees go to fee collector
            basefee: U256::from(1_000_000_000u64), // 1 gwei
            difficulty: U256::ZERO,
            prevrandao: Some(B256::ZERO),
            blob_excess_gas_and_price: Some(revm::primitives::BlobExcessGasAndPrice {
                excess_blob_gas: 0,
                blob_gasprice: 1,
            }),
        };

        // Transaction environment
        env.tx = TxEnv {
            caller: tx.from,
            gas_limit: tx.gas_limit,
            gas_price: U256::from(tx.gas_price),
            transact_to: match tx.to {
                Some(to) => TransactTo::Call(to),
                None => TransactTo::Create,
            },
            value: tx.value,
            data: tx.data.clone(),
            nonce: Some(tx.nonce),
            chain_id: Some(self.chain_id),
            access_list: vec![],
            gas_priority_fee: None,
            blob_hashes: vec![],
            max_fee_per_blob_gas: None,
            authorization_list: None,
        };

        // Ensure sender account exists with sufficient balance
        // Phase 3B: Only auto-create account if gas fees NOT enforced (development mode)
        if self.db.state.get_account(&tx.from).is_none() {
            if !self.enforce_gas_fees {
                // Development mode: auto-create account with balance for testing
                self.db.state.set_balance(tx.from, U256::from(10u64).pow(U256::from(20)));
            }
            // Production mode with gas fees: account must already exist with balance
        }

        // Execute transaction using EVM and capture results before dropping
        // Use transact_preverified to skip pre-execution validation checks (nonce, balance, etc.)
        // Our own gas balance check above handles the important case
        let (execution_result, state_changes) = {
            let mut evm = Evm::builder()
                .with_db(&mut self.db)
                .with_env(Box::new(env))
                .build();

            match evm.transact_preverified() {
                Ok(result_and_state) => (Ok(result_and_state.result), Some(result_and_state.state)),
                Err(e) => (Err(EvmExecutorError::Internal(format!("EVM error: {:?}", e))), None),
            }
        };

        // Only commit state changes for actual transactions, not simulations
        if commit_state {
            if let Some(changes) = state_changes {
                self.db.commit(changes);
            }
        }

        match execution_result {
            Ok(result) => match result.clone() {
                ExecutionResult::Success { output, gas_used, logs, .. } => {
                    debug!("Execution success: gas_used={}, output={} bytes", gas_used, output.data().len());

                    // Phase 3B: Apply gas fees for successful transactions
                    if max_gas_cost.is_some() {
                        self.apply_gas_fee(tx.from, gas_used, tx.gas_price);
                    }

                    let (output_data, contract_address) = match output {
                        Output::Call(data) => (data, None),
                        Output::Create(data, addr) => (data, addr),
                    };

                    Ok(EvmExecutionResult {
                        success: true,
                        gas_used,
                        output: output_data,
                        logs: logs.into_iter().map(|l| EvmLog {
                            address: l.address,
                            topics: l.topics().iter().map(|t| t.0).collect(),
                            data: l.data.data.clone(),
                        }).collect(),
                        contract_address,
                    })
                }
                ExecutionResult::Revert { gas_used, output } => {
                    debug!("Execution reverted: gas_used={}, output=0x{}", gas_used, hex::encode(&output));

                    // Phase 3B: Still charge gas for reverted transactions
                    if max_gas_cost.is_some() {
                        self.apply_gas_fee(tx.from, gas_used, tx.gas_price);
                    }

                    Ok(EvmExecutionResult {
                        success: false,
                        gas_used,
                        output,
                        logs: vec![],
                        contract_address: None,
                    })
                }
                ExecutionResult::Halt { reason, gas_used } => {
                    debug!("Execution halted: reason={:?}, gas_used={}", reason, gas_used);

                    // Phase 3B: Charge gas for halted transactions (up to gas_used)
                    if max_gas_cost.is_some() {
                        self.apply_gas_fee(tx.from, gas_used, tx.gas_price);
                    }

                    Err(EvmExecutorError::Internal(format!("Execution halted: {:?}", reason)))
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Apply gas fee - deduct from sender, credit to fee collector
    ///
    /// Phase 3B: This implements the gas fee settlement after transaction execution.
    /// Gas fee = gas_used * gas_price
    fn apply_gas_fee(&mut self, sender: Address, gas_used: u64, gas_price: u64) {
        let gas_fee = U256::from(gas_used) * U256::from(gas_price);

        if gas_fee.is_zero() {
            return;
        }

        // Debit from sender
        let debit_success = self.db.inner_mut().sub_balance(sender, gas_fee);
        if !debit_success {
            // This shouldn't happen if pre-flight check passed, but log it
            debug!("Warning: Failed to debit gas fee {} from {:?}", gas_fee, sender);
            return;
        }

        // Credit to fee collector
        self.db.inner_mut().add_balance(FEE_COLLECTOR_ADDRESS, gas_fee);

        debug!("Gas fee applied: {} from {:?} to fee collector", gas_fee, sender);
    }

    /// Execute a precompile call directly
    fn execute_precompile(&self, tx: &EvmTransaction, to: Address) -> Result<EvmExecutionResult, EvmExecutorError> {
        match self.precompiles.execute(to, &tx.data, tx.gas_limit) {
            Ok(PrecompileOutput { gas_used, bytes }) => {
                Ok(EvmExecutionResult {
                    success: true,
                    gas_used,
                    output: bytes,
                    logs: vec![],
                    contract_address: None,
                })
            }
            Err(e) => {
                Err(EvmExecutorError::Reverted(format!("Precompile error: {:?}", e)))
            }
        }
    }

    /// Get current state root (simplified - returns hash of serialized state)
    pub fn state_root(&self) -> [u8; 32] {
        // In production, this would compute a proper Merkle Patricia Trie root
        // For now, return a hash of the current block number
        let hash = Keccak256::digest(self.block_number.to_be_bytes());
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Commit state changes and advance block
    pub fn commit(&mut self) {
        self.block_number += 1;
        self.block_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> U256 {
        self.db.state.get_balance(address)
    }

    /// Set account balance
    pub fn set_balance(&mut self, address: Address, balance: U256) {
        self.db.state.set_balance(address, balance);
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.db.state.get_nonce(address)
    }

    /// Get contract code
    pub fn get_code(&self, address: &Address) -> Bytes {
        self.db.state.get_code(address)
            .map(|v| Bytes::from(v.clone()))
            .unwrap_or_default()
    }

    /// Get storage value
    pub fn get_storage(&self, address: &Address, slot: U256) -> U256 {
        self.db.state.get_storage(address, &slot)
    }

    /// Get current block number
    pub fn block_number(&self) -> u64 {
        self.block_number
    }

    /// Get chain ID
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Access the underlying state
    pub fn state(&self) -> &EvmState {
        &self.db.state
    }

    /// Access the underlying state mutably
    pub fn state_mut(&mut self) -> &mut EvmState {
        &mut self.db.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let executor = EvmExecutor::new(engine, 1337);
        assert_eq!(executor.chain_id, 1337);
    }

    #[tokio::test]
    async fn test_simple_transfer() {
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let mut executor = EvmExecutor::new(engine, 1337);

        let sender = Address::from([1u8; 20]);
        let recipient = Address::from([2u8; 20]);

        // Fund sender
        executor.set_balance(sender, U256::from(10u64).pow(U256::from(18)));

        let tx = EvmTransaction {
            from: sender,
            to: Some(recipient),
            value: U256::from(1000u64),
            data: Bytes::new(),
            gas_limit: 21000,
            gas_price: 1_000_000_000,
            nonce: 0,
        };

        let result = executor.execute_tx(&tx).unwrap();
        assert!(result.success);
        assert_eq!(result.gas_used, 21000);
    }

    #[test]
    fn test_contract_address_calculation() {
        let sender = Address::from([1u8; 20]);
        let nonce = 0u64;

        // RLP encode [sender, nonce]
        let mut buf = Vec::new();
        buf.push(0xd6); // list header (22 bytes)
        buf.push(0x94); // address prefix (20 bytes)
        buf.extend_from_slice(sender.as_slice());
        buf.push(0x80); // nonce 0

        let hash = Keccak256::digest(&buf);
        let addr = Address::from_slice(&hash[12..]);

        assert!(!addr.is_zero());
    }

    #[tokio::test]
    async fn test_contract_deploy_and_call() {
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let mut executor = EvmExecutor::new(engine, 1337);

        let sender = Address::from([1u8; 20]);
        executor.set_balance(sender, U256::from(10u64).pow(U256::from(20)));

        // SimpleStorage contract init code - fresh from solc 0.8.29 (uses PUSH0 opcode)
        // contract SimpleTest { uint256 public value; function set(uint256 v) external { value = v; } }
        let init_code = hex::decode(
            "6080604052348015600e575f5ffd5b506101268061001c5f395ff3fe6080604052348015600e575f5ffd5b50600436106030575f3560e01c80633fa4f24514603457806360fe47b114604e575b5f5ffd5b603a6066565b60405160459190608a565b60405180910390f35b606460048036038101906060919060ca565b606b565b005b5f5481565b805f8190555050565b5f819050919050565b6084816074565b82525050565b5f602082019050609b5f830184607d565b92915050565b5f5ffd5b60ac816074565b811460b5575f5ffd5b50565b5f8135905060c48160a5565b92915050565b5f6020828403121560dc5760db60a1565b5b5f60e78482850160b8565b9150509291505056fea2646970667358221220c4743886b71e5877ab5d3cedebeb402b1b2d64a4c5cbc2c3ecf6a478e520ec3c64736f6c634300081d0033"
        ).unwrap();

        // Deploy contract
        let deploy_tx = EvmTransaction {
            from: sender,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(init_code),
            gas_limit: 1_000_000,
            gas_price: 1_000_000_000,
            nonce: 0,
        };

        let deploy_result = executor.execute_tx(&deploy_tx).unwrap();
        assert!(deploy_result.success, "Deployment failed");

        let contract_addr = deploy_result.contract_address.expect("No contract address");
        println!("Contract deployed at: {:?}", contract_addr);

        // Verify contract code was stored
        let stored_code = executor.get_code(&contract_addr);
        println!("Stored code length: {} bytes", stored_code.len());
        assert!(!stored_code.is_empty(), "No code stored");

        // Call value() function - selector is 0x3fa4f245
        let call_data = hex::decode("3fa4f245").unwrap();
        let call_tx = EvmTransaction {
            from: sender,
            to: Some(contract_addr),
            value: U256::ZERO,
            data: Bytes::from(call_data),
            gas_limit: 100_000,
            gas_price: 1_000_000_000,
            nonce: 1,
        };

        let call_result = executor.execute_tx(&call_tx);
        println!("Call result: {:?}", call_result);

        match call_result {
            Ok(result) => {
                assert!(result.success, "Call failed: output = 0x{}", hex::encode(&result.output));
                // value() should return 0 initially
                let value = U256::from_be_slice(&result.output);
                assert_eq!(value, U256::ZERO, "Initial value should be 0");
            }
            Err(e) => {
                panic!("Call execution error: {:?}", e);
            }
        }
    }

    // Test minimal function dispatch pattern (like Solidity uses)
    #[test]
    fn test_minimal_function_dispatch() {
        use revm::db::InMemoryDB;

        let mut db = InMemoryDB::default();
        let sender = Address::from([1u8; 20]);

        db.insert_account_info(sender, AccountInfo {
            balance: U256::from(10u64).pow(U256::from(20)),
            nonce: 0,
            code_hash: B256::ZERO,
            code: None,
        });

        // Minimal contract with function dispatch for value() selector 0x3fa4f245
        // Returns 42 if selector matches, 0 otherwise
        //
        // Position 0-1: 60 00      PUSH1 0
        // Position 2: 35          CALLDATALOAD
        // Position 3-4: 60 e0     PUSH1 224 (0xe0)
        // Position 5: 1c         SHR
        // Position 6-10: 63 3fa4f245  PUSH4 0x3fa4f245 (5 bytes!)
        // Position 11: 14        EQ
        // Position 12-13: 60 19  PUSH1 25 (jump to JUMPDEST at pos 25)
        // Position 14: 57        JUMPI
        // Position 15-16: 60 00  PUSH1 0 (default return value)
        // Position 17-18: 60 00  PUSH1 0
        // Position 19: 52        MSTORE
        // Position 20-21: 60 20  PUSH1 32
        // Position 22-23: 60 00  PUSH1 0
        // Position 24: f3        RETURN
        // Position 25: 5b        JUMPDEST
        // Position 26-27: 60 2a  PUSH1 42
        // Position 28-29: 60 00  PUSH1 0
        // Position 30: 52        MSTORE
        // Position 31-32: 60 20  PUSH1 32
        // Position 33-34: 60 00  PUSH1 0
        // Position 35: f3        RETURN

        let runtime_code = hex::decode("60003560e01c633fa4f24514601957600060005260206000f35b602a60005260206000f3").unwrap();
        println!("[FuncDispatch] Runtime code len: {}", runtime_code.len());
        println!("[FuncDispatch] Runtime code hex: {}", hex::encode(&runtime_code));

        let code = Bytecode::new_raw(Bytes::from(runtime_code));
        let analyzed = to_analysed(code);
        println!("[FuncDispatch] Code analyzed: {} bytes", analyzed.len());

        let code_hash = B256::from_slice(&Keccak256::digest(analyzed.original_bytes()));
        let contract_addr = Address::from([0x44u8; 20]);

        db.insert_account_info(contract_addr, AccountInfo {
            balance: U256::ZERO,
            nonce: 0,
            code_hash,
            code: Some(analyzed),
        });

        // Call with value() selector
        let call_data = hex::decode("3fa4f245").unwrap();

        let mut env = Env::default();
        env.cfg.chain_id = 1337;
        env.block.number = U256::from(1);
        env.block.gas_limit = U256::from(30_000_000u64);
        env.block.basefee = U256::from(1_000_000_000u64);
        env.tx.caller = sender;
        env.tx.gas_limit = 100_000;
        env.tx.gas_price = U256::from(1_000_000_000u64);
        env.tx.transact_to = TransactTo::Call(contract_addr);
        env.tx.data = Bytes::from(call_data);
        env.tx.nonce = Some(0);
        env.tx.chain_id = Some(1337);

        let mut evm = Evm::builder()
            .with_db(db)
            .with_env(Box::new(env))
            .build();

        let result = evm.transact_preverified().expect("Call should succeed");
        println!("[FuncDispatch] Call result: {:?}", result.result);

        match &result.result {
            ExecutionResult::Success { output: Output::Call(data), .. } => {
                let value = U256::from_be_slice(data);
                println!("[FuncDispatch] Returned: {}", value);
                assert_eq!(value, U256::from(42), "Should return 42");
            }
            ExecutionResult::Halt { reason, .. } => {
                panic!("[FuncDispatch] Call halted: {:?}", reason);
            }
            other => panic!("[FuncDispatch] Unexpected: {:?}", other),
        }
    }

    // Test with a contract that has a simple unconditional JUMP
    #[test]
    fn test_contract_with_jump() {
        use revm::db::InMemoryDB;

        let mut db = InMemoryDB::default();
        let sender = Address::from([1u8; 20]);

        db.insert_account_info(sender, AccountInfo {
            balance: U256::from(10u64).pow(U256::from(20)),
            nonce: 0,
            code_hash: B256::ZERO,
            code: None,
        });

        // Contract with unconditional JUMP:
        // Position 0-1: 60 03  - PUSH1 3 (jump target)
        // Position 2: 56       - JUMP
        // Position 3: 5b       - JUMPDEST
        // Position 4-5: 60 2a  - PUSH1 42
        // Position 6-7: 60 00  - PUSH1 0
        // Position 8: 52       - MSTORE
        // Position 9-10: 60 20 - PUSH1 32
        // Position 11-12: 60 00 - PUSH1 0
        // Position 13: f3      - RETURN
        let runtime_code = hex::decode("6003565b602a60005260206000f3").unwrap();
        println!("[JumpTest] Runtime code: {:02x?}", runtime_code);
        println!("[JumpTest] Code length: {}", runtime_code.len());

        let code = Bytecode::new_raw(Bytes::from(runtime_code));
        let analyzed = to_analysed(code);
        println!("[JumpTest] Code analyzed: {} bytes", analyzed.len());

        let code_hash = B256::from_slice(&Keccak256::digest(analyzed.original_bytes()));
        let contract_addr = Address::from([0x43u8; 20]);

        db.insert_account_info(contract_addr, AccountInfo {
            balance: U256::ZERO,
            nonce: 0,
            code_hash,
            code: Some(analyzed),
        });

        let mut env = Env::default();
        env.cfg.chain_id = 1337;
        env.block.number = U256::from(1);
        env.block.gas_limit = U256::from(30_000_000u64);
        env.block.basefee = U256::from(1_000_000_000u64);
        env.tx.caller = sender;
        env.tx.gas_limit = 100_000;
        env.tx.gas_price = U256::from(1_000_000_000u64);
        env.tx.transact_to = TransactTo::Call(contract_addr);
        env.tx.data = Bytes::new();
        env.tx.nonce = Some(0);
        env.tx.chain_id = Some(1337);

        let mut evm = Evm::builder()
            .with_db(db)
            .with_env(Box::new(env))
            .build();

        let result = evm.transact_preverified().expect("Call should succeed");
        println!("[JumpTest] Call result: {:?}", result.result);

        match &result.result {
            ExecutionResult::Success { output: Output::Call(data), .. } => {
                let value = U256::from_be_slice(data);
                println!("[JumpTest] Returned: {}", value);
                assert_eq!(value, U256::from(42), "Should return 42");
            }
            ExecutionResult::Halt { reason, .. } => {
                panic!("[JumpTest] Call halted: {:?}", reason);
            }
            other => panic!("[JumpTest] Unexpected: {:?}", other),
        }
    }

    // Test with simplest possible contract that just returns 42
    #[test]
    fn test_simplest_contract() {
        use revm::db::InMemoryDB;

        let mut db = InMemoryDB::default();
        let sender = Address::from([1u8; 20]);

        // Fund sender
        db.insert_account_info(sender, AccountInfo {
            balance: U256::from(10u64).pow(U256::from(20)),
            nonce: 0,
            code_hash: B256::ZERO,
            code: None,
        });

        // Simplest contract: just returns 42
        // Bytecode: PUSH1 42, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
        // 602a 6000 52 6020 6000 f3
        let runtime_code = hex::decode("602a60005260206000f3").unwrap();

        // Create contract account directly with the code
        let code = Bytecode::new_raw(Bytes::from(runtime_code));
        let analyzed = to_analysed(code);
        println!("[Simplest] Code analyzed: {} bytes", analyzed.len());

        let code_hash = B256::from_slice(&Keccak256::digest(analyzed.original_bytes()));
        let contract_addr = Address::from([0x42u8; 20]);

        db.insert_account_info(contract_addr, AccountInfo {
            balance: U256::ZERO,
            nonce: 0,
            code_hash,
            code: Some(analyzed),
        });

        // Call the contract (no function selector needed - just runs)
        let mut env = Env::default();
        env.cfg.chain_id = 1337;
        env.block.number = U256::from(1);
        env.block.gas_limit = U256::from(30_000_000u64);
        env.block.basefee = U256::from(1_000_000_000u64);
        env.tx.caller = sender;
        env.tx.gas_limit = 100_000;
        env.tx.gas_price = U256::from(1_000_000_000u64);
        env.tx.transact_to = TransactTo::Call(contract_addr);
        env.tx.data = Bytes::new();
        env.tx.nonce = Some(0);
        env.tx.chain_id = Some(1337);

        let mut evm = Evm::builder()
            .with_db(db)
            .with_env(Box::new(env))
            .build();

        let result = evm.transact_preverified().expect("Call should succeed");
        println!("[Simplest] Call result: {:?}", result.result);

        match &result.result {
            ExecutionResult::Success { output: Output::Call(data), .. } => {
                let value = U256::from_be_slice(data);
                println!("[Simplest] Returned: {}", value);
                assert_eq!(value, U256::from(42), "Should return 42");
            }
            ExecutionResult::Halt { reason, .. } => {
                panic!("[Simplest] Call halted: {:?}", reason);
            }
            other => panic!("[Simplest] Unexpected: {:?}", other),
        }
    }

    // Test using revm's in-memory database directly to verify bytecode is correct
    #[test]
    fn test_revm_in_memory_db() {
        use revm::db::InMemoryDB;

        let mut db = InMemoryDB::default();
        let sender = Address::from([1u8; 20]);

        // Fund sender
        db.insert_account_info(sender, AccountInfo {
            balance: U256::from(10u64).pow(U256::from(20)),
            nonce: 0,
            code_hash: B256::ZERO,
            code: None,
        });

        // Deploy contract init code - fresh from solc 0.8.29 (uses PUSH0 opcode)
        let init_code = hex::decode(
            "6080604052348015600e575f5ffd5b506101268061001c5f395ff3fe6080604052348015600e575f5ffd5b50600436106030575f3560e01c80633fa4f24514603457806360fe47b114604e575b5f5ffd5b603a6066565b60405160459190608a565b60405180910390f35b606460048036038101906060919060ca565b606b565b005b5f5481565b805f8190555050565b5f819050919050565b6084816074565b82525050565b5f602082019050609b5f830184607d565b92915050565b5f5ffd5b60ac816074565b811460b5575f5ffd5b50565b5f8135905060c48160a5565b92915050565b5f6020828403121560dc5760db60a1565b5b5f60e78482850160b8565b9150509291505056fea2646970667358221220c4743886b71e5877ab5d3cedebeb402b1b2d64a4c5cbc2c3ecf6a478e520ec3c64736f6c634300081d0033"
        ).unwrap();

        let mut env = Env::default();
        env.cfg.chain_id = 1337;
        env.block.number = U256::from(1);
        env.block.timestamp = U256::from(1000);
        env.block.gas_limit = U256::from(30_000_000u64);
        env.block.basefee = U256::from(1_000_000_000u64);
        env.tx.caller = sender;
        env.tx.gas_limit = 1_000_000;
        env.tx.gas_price = U256::from(1_000_000_000u64);
        env.tx.transact_to = TransactTo::Create;
        env.tx.data = Bytes::from(init_code);
        env.tx.nonce = Some(0);
        env.tx.chain_id = Some(1337);

        let mut evm = Evm::builder()
            .with_db(db)
            .with_env(Box::new(env))
            .build();

        let result = evm.transact_preverified().expect("Deploy should succeed");
        println!("[InMemoryDB] Deploy result: {:?}", result.result);

        let contract_addr = match &result.result {
            ExecutionResult::Success { output: Output::Create(_, Some(addr)), .. } => *addr,
            _ => panic!("Expected successful create with address"),
        };
        println!("[InMemoryDB] Contract at: {:?}", contract_addr);

        // Commit the state
        evm.db_mut().commit(result.state);

        // Debug: Check what's in the database after commit
        {
            let db = evm.db();
            let account = db.accounts.get(&contract_addr);
            println!("[InMemoryDB] Account in db after commit: {:?}", account.is_some());
            if let Some(acc) = account {
                println!("[InMemoryDB] Account info: balance={}, nonce={}", acc.info.balance, acc.info.nonce);
                println!("[InMemoryDB] Code hash: {:?}", acc.info.code_hash);
                if let Some(ref code) = acc.info.code {
                    println!("[InMemoryDB] Code present: {} bytes", code.len());
                } else {
                    println!("[InMemoryDB] Code is None in AccountInfo");
                }
            }
        }

        // Now call value() - modify the env and reuse the EVM
        let call_data = hex::decode("3fa4f245").unwrap();

        // Modify transaction for the call
        {
            let env = &mut evm.context.evm.env;
            env.tx.transact_to = TransactTo::Call(contract_addr);
            env.tx.data = Bytes::from(call_data);
            env.tx.nonce = Some(1);
            env.tx.gas_limit = 100_000;
        }

        let call_result = evm.transact_preverified().expect("Call should succeed");
        println!("[InMemoryDB] Call result: {:?}", call_result.result);

        match &call_result.result {
            ExecutionResult::Success { output: Output::Call(data), .. } => {
                let value = U256::from_be_slice(data);
                println!("[InMemoryDB] value() returned: {}", value);
                assert_eq!(value, U256::ZERO, "Initial value should be 0");
            }
            ExecutionResult::Halt { reason, .. } => {
                panic!("[InMemoryDB] Call halted: {:?}", reason);
            }
            other => panic!("[InMemoryDB] Unexpected result: {:?}", other),
        }
    }
}
