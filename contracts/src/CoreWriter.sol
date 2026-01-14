// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ICoreWriter} from "./interfaces/ICoreWriter.sol";

/// @title CoreWriter
/// @notice System contract for submitting actions to the HyperCore engine
/// @dev This contract emits events that the node processes at end-of-block
/// Actions are executed in the NEXT block, making them non-atomic with EVM state
contract CoreWriter is ICoreWriter {
    // ============ Constants ============

    /// @notice The USDC token address (to be set in constructor or immutable)
    address public immutable usdcToken;

    // ============ State ============

    /// @notice Counter for generating unique action IDs
    uint256 private _actionNonce;

    /// @notice Mapping of action ID to its result
    mapping(bytes32 => ActionResult) private _actionResults;

    // ============ Constructor ============

    /// @param _usdc The USDC token address
    constructor(address _usdc) {
        usdcToken = _usdc;
    }

    // ============ External Functions ============

    /// @inheritdoc ICoreWriter
    function placeOrder(OrderParams calldata params) external returns (bytes32 actionId) {
        // Validate parameters
        require(params.size > 0, "CoreWriter: size must be positive");
        require(params.price > 0 || params.orderType == OrderType.Market, "CoreWriter: invalid price");

        // Generate unique action ID
        actionId = _generateActionId(msg.sender, uint8(ActionType.PlaceOrder));

        // Encode action data
        bytes memory data = abi.encode(
            params.marketId,
            params.isBuy,
            params.price,
            params.size,
            uint8(params.orderType),
            params.reduceOnly
        );

        // Emit event for node to process
        emit ActionQueued(actionId, msg.sender, uint8(ActionType.PlaceOrder), data);

        // Initialize pending status
        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });
    }

    /// @inheritdoc ICoreWriter
    function cancelOrder(uint8 marketId, uint256 orderId) external returns (bytes32 actionId) {
        require(orderId > 0, "CoreWriter: invalid order ID");

        actionId = _generateActionId(msg.sender, uint8(ActionType.CancelOrder));

        bytes memory data = abi.encode(marketId, orderId);

        emit ActionQueued(actionId, msg.sender, uint8(ActionType.CancelOrder), data);

        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });
    }

    /// @inheritdoc ICoreWriter
    function cancelAllOrders(uint8 marketId) external returns (bytes32 actionId) {
        actionId = _generateActionId(msg.sender, uint8(ActionType.CancelAllOrders));

        bytes memory data = abi.encode(marketId);

        emit ActionQueued(actionId, msg.sender, uint8(ActionType.CancelAllOrders), data);

        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });
    }

    /// @inheritdoc ICoreWriter
    function updateLeverage(uint8 marketId, uint8 leverage) external returns (bytes32 actionId) {
        require(leverage >= 1 && leverage <= 100, "CoreWriter: invalid leverage");

        actionId = _generateActionId(msg.sender, uint8(ActionType.UpdateLeverage));

        bytes memory data = abi.encode(marketId, leverage);

        emit ActionQueued(actionId, msg.sender, uint8(ActionType.UpdateLeverage), data);

        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });
    }

    /// @inheritdoc ICoreWriter
    function depositToCore(uint256 amount) external returns (bytes32 actionId) {
        require(amount > 0, "CoreWriter: amount must be positive");

        // Transfer USDC from sender to this contract
        // The node will recognize this and credit the Core balance
        (bool success, bytes memory returnData) = usdcToken.call(
            abi.encodeWithSignature(
                "transferFrom(address,address,uint256)",
                msg.sender,
                address(this),
                amount
            )
        );
        require(success && (returnData.length == 0 || abi.decode(returnData, (bool))), "CoreWriter: transfer failed");

        actionId = _generateActionId(msg.sender, uint8(ActionType.DepositToCore));

        bytes memory data = abi.encode(amount);

        emit ActionQueued(actionId, msg.sender, uint8(ActionType.DepositToCore), data);
        emit DepositToCore(msg.sender, amount);

        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });
    }

    /// @inheritdoc ICoreWriter
    function withdrawToEvm(uint256 amount) external returns (bytes32 actionId) {
        require(amount > 0, "CoreWriter: amount must be positive");

        actionId = _generateActionId(msg.sender, uint8(ActionType.WithdrawToEvm));

        bytes memory data = abi.encode(amount);

        emit ActionQueued(actionId, msg.sender, uint8(ActionType.WithdrawToEvm), data);

        _actionResults[actionId] = ActionResult({
            status: ActionStatus.Pending,
            result: "",
            timestamp: 0
        });

        // Note: USDC transfer happens when action is executed by node
        // The node will call _completeWithdraw after processing
    }

    // ============ View Functions ============

    /// @inheritdoc ICoreWriter
    function getActionStatus(bytes32 actionId) external view returns (ActionResult memory) {
        return _actionResults[actionId];
    }

    /// @inheritdoc ICoreWriter
    function isPending(bytes32 actionId) external view returns (bool) {
        return _actionResults[actionId].status == ActionStatus.Pending;
    }

    /// @inheritdoc ICoreWriter
    function getUsdcToken() external view returns (address) {
        return usdcToken;
    }

    // ============ Internal Functions ============

    /// @notice Generate a unique action ID
    function _generateActionId(address sender, uint8 actionType) internal returns (bytes32) {
        return keccak256(
            abi.encodePacked(
                sender,
                actionType,
                block.number,
                block.timestamp,
                ++_actionNonce
            )
        );
    }

    // ============ Node Callback Functions ============
    // These functions are called by the node after processing actions

    /// @notice Called by node to update action result (restricted to system)
    /// @param actionId The action ID
    /// @param success Whether the action succeeded
    /// @param result Encoded result data
    function _setActionResult(
        bytes32 actionId,
        bool success,
        bytes calldata result
    ) external {
        // In production, this would have access control (only callable by node)
        // For now, we allow any caller for testing purposes

        _actionResults[actionId] = ActionResult({
            status: success ? ActionStatus.Executed : ActionStatus.Failed,
            result: result,
            timestamp: block.timestamp
        });

        emit ActionExecuted(actionId, success, result);
    }

    /// @notice Called by node to complete a withdrawal
    /// @param recipient The withdrawal recipient
    /// @param amount Amount to transfer
    function _completeWithdraw(address recipient, uint256 amount) external {
        // In production, this would have access control
        (bool success,) = usdcToken.call(
            abi.encodeWithSignature("transfer(address,uint256)", recipient, amount)
        );
        require(success, "CoreWriter: withdrawal transfer failed");

        emit WithdrawToEvm(recipient, amount);
    }
}
