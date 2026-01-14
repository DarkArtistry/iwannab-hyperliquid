// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title ICoreWriter
/// @notice Interface for submitting actions to the HyperCore engine
/// @dev Actions are queued and executed in the NEXT block (non-atomic)
/// This design prevents sandwich attacks on engine state within a single EVM tx
interface ICoreWriter {
    // ============ Events ============

    /// @notice Emitted when an action is queued for execution
    /// @param actionId Unique identifier for tracking the action
    /// @param sender The account that submitted the action
    /// @param actionType Type of action (see ActionType enum)
    /// @param data Encoded action parameters
    event ActionQueued(
        bytes32 indexed actionId,
        address indexed sender,
        uint8 actionType,
        bytes data
    );

    /// @notice Emitted when a queued action is executed
    /// @param actionId The action ID from ActionQueued
    /// @param success Whether the action succeeded
    /// @param result Encoded result or error message
    event ActionExecuted(
        bytes32 indexed actionId,
        bool success,
        bytes result
    );

    /// @notice Emitted when USDC is deposited to Core
    /// @param account The depositing account
    /// @param amount Amount deposited (6 decimals)
    event DepositToCore(address indexed account, uint256 amount);

    /// @notice Emitted when USDC is withdrawn from Core to EVM
    /// @param account The withdrawing account
    /// @param amount Amount withdrawn (6 decimals)
    event WithdrawToEvm(address indexed account, uint256 amount);

    // ============ Enums ============

    /// @notice Action types for the engine
    enum ActionType {
        PlaceOrder,      // 0
        CancelOrder,     // 1
        CancelAllOrders, // 2
        UpdateLeverage,  // 3
        DepositToCore,   // 4
        WithdrawToEvm    // 5
    }

    /// @notice Order types
    enum OrderType {
        Limit,      // 0 - Good-til-canceled limit order
        Market,     // 1 - Market order (IOC behavior)
        PostOnly,   // 2 - Post-only (maker only)
        Ioc,        // 3 - Immediate-or-cancel
        Fok         // 4 - Fill-or-kill
    }

    /// @notice Action status
    enum ActionStatus {
        Pending,    // 0 - Action queued, not yet executed
        Executed,   // 1 - Action executed successfully
        Failed,     // 2 - Action failed
        Expired     // 3 - Action expired (not executed in time)
    }

    // ============ Structs ============

    /// @notice Parameters for placing an order
    struct OrderParams {
        uint8 marketId;     // Market ID (0 = BTC-PERP, 1 = ETH-PERP)
        bool isBuy;         // True = buy/long, false = sell/short
        uint256 price;      // Limit price (8 decimals)
        uint256 size;       // Order size (8 decimals)
        OrderType orderType;// Order type
        bool reduceOnly;    // If true, can only reduce position
    }

    /// @notice Result of action status query
    struct ActionResult {
        ActionStatus status;
        bytes result;       // Encoded success data or error message
        uint256 timestamp;  // When action was processed (0 if pending)
    }

    // ============ Write Functions (Non-Atomic) ============

    /// @notice Place an order on the HyperCore engine
    /// @dev Action is queued and executed in the next block
    /// @param params Order parameters
    /// @return actionId Unique ID to track this action
    function placeOrder(OrderParams calldata params) external returns (bytes32 actionId);

    /// @notice Cancel a specific order
    /// @param marketId The market ID
    /// @param orderId The order ID to cancel
    /// @return actionId Unique ID to track this action
    function cancelOrder(uint8 marketId, uint256 orderId) external returns (bytes32 actionId);

    /// @notice Cancel all open orders in a market
    /// @param marketId The market ID
    /// @return actionId Unique ID to track this action
    function cancelAllOrders(uint8 marketId) external returns (bytes32 actionId);

    /// @notice Update leverage for a market
    /// @param marketId The market ID
    /// @param leverage New leverage value (1-100)
    /// @return actionId Unique ID to track this action
    function updateLeverage(uint8 marketId, uint8 leverage) external returns (bytes32 actionId);

    /// @notice Deposit USDC from EVM balance to Core trading balance
    /// @dev Requires prior ERC20 approval to this contract
    /// @param amount Amount to deposit (6 decimals)
    /// @return actionId Unique ID to track this action
    function depositToCore(uint256 amount) external returns (bytes32 actionId);

    /// @notice Withdraw USDC from Core trading balance to EVM
    /// @param amount Amount to withdraw (6 decimals)
    /// @return actionId Unique ID to track this action
    function withdrawToEvm(uint256 amount) external returns (bytes32 actionId);

    // ============ View Functions ============

    /// @notice Get status of a queued/executed action
    /// @param actionId The action ID
    /// @return result Action status and result
    function getActionStatus(bytes32 actionId) external view returns (ActionResult memory result);

    /// @notice Check if an action is still pending
    /// @param actionId The action ID
    /// @return pending True if action hasn't been executed yet
    function isPending(bytes32 actionId) external view returns (bool pending);

    /// @notice Get the USDC token address used for deposits/withdrawals
    /// @return usdc The USDC token contract address
    function getUsdcToken() external view returns (address usdc);
}
