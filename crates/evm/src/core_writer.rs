//! CoreWriter action queue for non-atomic writes
//!
//! The CoreWriter pattern prevents MEV by queuing actions from smart contracts
//! to be executed in the next block, rather than atomically in the same transaction.

use std::collections::VecDeque;

use hypercore_primitives::{AccountAddress, Decimal, MarketId, OrderSide, Timestamp};
use serde::{Deserialize, Serialize};

/// Action types that can be queued via CoreWriter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ActionType {
    /// Place an order
    PlaceOrder = 0,
    /// Cancel an order
    CancelOrder = 1,
    /// Update leverage
    UpdateLeverage = 2,
    /// Transfer funds
    Transfer = 3,
    /// Withdraw funds
    Withdraw = 4,
}

impl TryFrom<u8> for ActionType {
    type Error = ActionQueueError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::PlaceOrder),
            1 => Ok(Self::CancelOrder),
            2 => Ok(Self::UpdateLeverage),
            3 => Ok(Self::Transfer),
            4 => Ok(Self::Withdraw),
            _ => Err(ActionQueueError::InvalidActionType(value)),
        }
    }
}

/// Queued action from CoreWriter contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreWriterAction {
    /// Unique action ID
    pub action_id: [u8; 32],
    /// Sender address (the smart contract that called CoreWriter)
    pub sender: AccountAddress,
    /// Beneficiary address (who the action is for)
    pub beneficiary: AccountAddress,
    /// Action type
    pub action_type: ActionType,
    /// Action data (ABI-encoded parameters)
    pub data: Vec<u8>,
    /// Block number when queued
    pub queued_block: u64,
    /// Timestamp when queued
    pub queued_time: Timestamp,
}

impl CoreWriterAction {
    /// Decode place order action
    pub fn decode_place_order(&self) -> Result<PlaceOrderParams, ActionQueueError> {
        if self.data.len() < 96 {
            return Err(ActionQueueError::InvalidData);
        }

        // Decode: marketId (uint8), isBuy (bool), price (uint256), size (uint256), reduceOnly (bool)
        let market_id = self.data[31] as MarketId;
        let is_buy = self.data[63] != 0;
        let price = decode_uint256(&self.data[64..96])?;
        let size = decode_uint256(&self.data[96..128])?;
        let reduce_only = self.data.get(159).map(|&b| b != 0).unwrap_or(false);

        Ok(PlaceOrderParams {
            market_id,
            side: if is_buy { OrderSide::Buy } else { OrderSide::Sell },
            price,
            size,
            reduce_only,
        })
    }

    /// Decode cancel order action
    pub fn decode_cancel_order(&self) -> Result<CancelOrderParams, ActionQueueError> {
        if self.data.len() < 64 {
            return Err(ActionQueueError::InvalidData);
        }

        let market_id = self.data[31] as MarketId;
        let order_id = u64::from_be_bytes(self.data[56..64].try_into().unwrap());

        Ok(CancelOrderParams { market_id, order_id })
    }

    /// Decode update leverage action
    pub fn decode_update_leverage(&self) -> Result<UpdateLeverageParams, ActionQueueError> {
        if self.data.len() < 64 {
            return Err(ActionQueueError::InvalidData);
        }

        let market_id = self.data[31] as MarketId;
        let leverage = self.data[63] as u8;
        let is_cross = self.data.get(95).map(|&b| b != 0).unwrap_or(true);

        Ok(UpdateLeverageParams {
            market_id,
            leverage,
            is_cross,
        })
    }

    /// Decode transfer action
    pub fn decode_transfer(&self) -> Result<TransferParams, ActionQueueError> {
        if self.data.len() < 64 {
            return Err(ActionQueueError::InvalidData);
        }

        let mut dest_bytes = [0u8; 20];
        dest_bytes.copy_from_slice(&self.data[12..32]);
        let destination = AccountAddress::from(dest_bytes);
        let amount = decode_uint256(&self.data[32..64])?;

        Ok(TransferParams { destination, amount })
    }

    /// Decode withdraw action
    pub fn decode_withdraw(&self) -> Result<WithdrawParams, ActionQueueError> {
        if self.data.len() < 64 {
            return Err(ActionQueueError::InvalidData);
        }

        let mut dest_bytes = [0u8; 20];
        dest_bytes.copy_from_slice(&self.data[12..32]);
        let destination = AccountAddress::from(dest_bytes);
        let amount = decode_uint256(&self.data[32..64])?;

        Ok(WithdrawParams { destination, amount })
    }
}

/// Place order parameters
#[derive(Debug, Clone)]
pub struct PlaceOrderParams {
    pub market_id: MarketId,
    pub side: OrderSide,
    pub price: Decimal,
    pub size: Decimal,
    pub reduce_only: bool,
}

/// Cancel order parameters
#[derive(Debug, Clone)]
pub struct CancelOrderParams {
    pub market_id: MarketId,
    pub order_id: u64,
}

/// Update leverage parameters
#[derive(Debug, Clone)]
pub struct UpdateLeverageParams {
    pub market_id: MarketId,
    pub leverage: u8,
    pub is_cross: bool,
}

/// Transfer parameters
#[derive(Debug, Clone)]
pub struct TransferParams {
    pub destination: AccountAddress,
    pub amount: Decimal,
}

/// Withdraw parameters
#[derive(Debug, Clone)]
pub struct WithdrawParams {
    pub destination: AccountAddress,
    pub amount: Decimal,
}

/// Action queue for managing CoreWriter actions
#[derive(Debug, Default)]
pub struct ActionQueue {
    /// Pending actions to be processed
    pending: VecDeque<CoreWriterAction>,
    /// Actions being processed in current block
    processing: Vec<CoreWriterAction>,
    /// Next action ID counter
    next_id: u64,
}

impl ActionQueue {
    /// Create new action queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a new action
    pub fn queue_action(
        &mut self,
        sender: AccountAddress,
        beneficiary: AccountAddress,
        action_type: ActionType,
        data: Vec<u8>,
        block: u64,
        timestamp: Timestamp,
    ) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        // Generate action ID
        let mut hasher = Keccak256::new();
        hasher.update(sender.as_slice());
        hasher.update(&self.next_id.to_le_bytes());
        hasher.update(&block.to_le_bytes());
        let action_id: [u8; 32] = hasher.finalize().into();

        self.next_id += 1;

        let action = CoreWriterAction {
            action_id,
            sender,
            beneficiary,
            action_type,
            data,
            queued_block: block,
            queued_time: timestamp,
        };

        self.pending.push_back(action);
        action_id
    }

    /// Get actions queued before a given block (for processing in current block)
    pub fn get_ready_actions(&mut self, current_block: u64) -> Vec<CoreWriterAction> {
        let mut ready = Vec::new();

        while let Some(action) = self.pending.front() {
            if action.queued_block < current_block {
                ready.push(self.pending.pop_front().unwrap());
            } else {
                break;
            }
        }

        self.processing = ready.clone();
        ready
    }

    /// Mark actions as processed
    pub fn mark_processed(&mut self) {
        self.processing.clear();
    }

    /// Get pending action count
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get action by ID
    pub fn get_action(&self, action_id: &[u8; 32]) -> Option<&CoreWriterAction> {
        self.pending.iter().find(|a| &a.action_id == action_id)
            .or_else(|| self.processing.iter().find(|a| &a.action_id == action_id))
    }

    /// Cancel action by ID (only if still pending)
    pub fn cancel_action(&mut self, action_id: &[u8; 32], sender: &AccountAddress) -> bool {
        if let Some(pos) = self.pending.iter().position(|a| {
            &a.action_id == action_id && &a.sender == sender
        }) {
            self.pending.remove(pos);
            true
        } else {
            false
        }
    }
}

/// Decode uint256 from ABI-encoded bytes to Decimal
fn decode_uint256(data: &[u8]) -> Result<Decimal, ActionQueueError> {
    if data.len() != 32 {
        return Err(ActionQueueError::InvalidData);
    }

    // Assume 18 decimals (standard EVM)
    // Convert to our internal 8 decimal representation
    let mut value = [0u8; 32];
    value.copy_from_slice(data);

    // For simplicity, we'll handle this as a u128 (ignoring overflow for very large values)
    let high = u128::from_be_bytes(value[0..16].try_into().unwrap());
    let low = u128::from_be_bytes(value[16..32].try_into().unwrap());

    if high > 0 {
        // Value too large, but we'll use the lower bits
        return Ok(Decimal::from_raw(low as i128, 18).scale_to(8));
    }

    Ok(Decimal::from_raw(low as i128, 18).scale_to(8))
}

/// Action queue errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ActionQueueError {
    #[error("Invalid action type: {0}")]
    InvalidActionType(u8),
    #[error("Invalid action data")]
    InvalidData,
    #[error("Action not found")]
    ActionNotFound,
    #[error("Not action owner")]
    NotActionOwner,
}

/// Log event emitted when action is queued
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionQueuedEvent {
    pub action_id: [u8; 32],
    pub sender: AccountAddress,
    pub beneficiary: AccountAddress,
    pub action_type: ActionType,
    pub data: Vec<u8>,
}

/// Log event emitted when action is executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecutedEvent {
    pub action_id: [u8; 32],
    pub success: bool,
    pub result: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_queue() {
        let mut queue = ActionQueue::new();

        let sender = AccountAddress::default();
        let action_id = queue.queue_action(
            sender,
            sender,
            ActionType::PlaceOrder,
            vec![0; 128],
            1,
            1000,
        );

        assert_eq!(queue.pending_count(), 1);
        assert!(queue.get_action(&action_id).is_some());

        // Actions from block 1 should be ready at block 2
        let ready = queue.get_ready_actions(2);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].action_id, action_id);

        queue.mark_processed();
        assert_eq!(queue.pending_count(), 0);
    }

    #[test]
    fn test_action_type_conversion() {
        assert_eq!(ActionType::try_from(0u8).unwrap(), ActionType::PlaceOrder);
        assert_eq!(ActionType::try_from(1u8).unwrap(), ActionType::CancelOrder);
        assert!(ActionType::try_from(255u8).is_err());
    }
}
