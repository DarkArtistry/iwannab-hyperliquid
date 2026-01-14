// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {HyperCore} from "../HyperCore.sol";
import {ICoreWriter} from "../interfaces/ICoreWriter.sol";
import {IPositionPrecompile} from "../interfaces/IPrecompiles.sol";

/// @title VaultExample
/// @notice Example vault contract demonstrating HyperCore integration
/// @dev Shows how to read positions, check health, and place orders
contract VaultExample {
    // ============ State ============

    /// @notice CoreWriter contract address
    ICoreWriter public immutable coreWriter;

    /// @notice Vault manager
    address public manager;

    /// @notice Target leverage for positions
    uint8 public targetLeverage = 10;

    /// @notice Maximum position value (in USDC, 6 decimals)
    uint256 public maxPositionValue = 1_000_000 * 1e6; // $1M

    // ============ Events ============

    event PositionOpened(uint8 indexed marketId, bool isBuy, uint256 size, uint256 price);
    event PositionClosed(uint8 indexed marketId, uint256 size);
    event LeverageUpdated(uint8 indexed marketId, uint8 leverage);

    // ============ Constructor ============

    constructor(address _coreWriter) {
        coreWriter = ICoreWriter(_coreWriter);
        manager = msg.sender;
    }

    // ============ View Functions ============

    /// @notice Get vault's position in a market
    function getVaultPosition(uint8 marketId)
        external
        view
        returns (int256 size, uint256 entryPrice, int256 unrealizedPnl)
    {
        IPositionPrecompile.Position memory pos = HyperCore.getPosition(address(this), marketId);
        return (pos.size, pos.entryPrice, pos.unrealizedPnl);
    }

    /// @notice Get vault's total equity
    function getVaultEquity() external view returns (uint256) {
        return HyperCore.getEquity(address(this));
    }

    /// @notice Get vault's free collateral
    function getVaultFreeCollateral() external view returns (uint256) {
        return HyperCore.getFreeCollateral(address(this));
    }

    /// @notice Check if vault is healthy
    function isVaultHealthy() external view returns (bool) {
        return HyperCore.isAccountHealthy(address(this));
    }

    /// @notice Get current market price
    function getPrice(uint8 marketId) external view returns (uint256 mark, uint256 index) {
        mark = HyperCore.getMarkPrice(marketId);
        index = HyperCore.getIndexPrice(marketId);
    }

    /// @notice Get current funding rate
    function getFunding(uint8 marketId) external view returns (int256 rate, uint256 nextTime) {
        return HyperCore.getFundingRate(marketId);
    }

    /// @notice Calculate potential position size for given USDC amount
    function calculatePositionSize(uint8 marketId, uint256 usdcAmount)
        external
        view
        returns (uint256 size)
    {
        uint256 markPrice = HyperCore.getMarkPrice(marketId);
        if (markPrice == 0) return 0;

        // Convert USDC to 8 decimals and apply leverage
        uint256 notional = HyperCore.usdcTo8Decimals(usdcAmount) * targetLeverage;

        // Size = notional / price
        size = (notional * 1e8) / markPrice;
    }

    // ============ Trading Functions ============

    /// @notice Open a long position
    /// @param marketId Market to trade
    /// @param size Position size (8 decimals)
    /// @param limitPrice Maximum price to pay (8 decimals)
    function openLong(uint8 marketId, uint256 size, uint256 limitPrice)
        external
        onlyManager
        returns (bytes32 actionId)
    {
        // Check market is active
        require(HyperCore.isMarketActive(marketId), "Market not active");

        // Check we have enough collateral
        uint256 freeCollateral = HyperCore.getFreeCollateral(address(this));
        uint256 requiredMargin = (size * limitPrice) / (targetLeverage * 1e8);
        require(
            HyperCore.to6Decimals(requiredMargin) <= freeCollateral,
            "Insufficient collateral"
        );

        // Place order via CoreWriter
        actionId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: marketId,
                isBuy: true,
                price: limitPrice,
                size: size,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        emit PositionOpened(marketId, true, size, limitPrice);
    }

    /// @notice Open a short position
    function openShort(uint8 marketId, uint256 size, uint256 limitPrice)
        external
        onlyManager
        returns (bytes32 actionId)
    {
        require(HyperCore.isMarketActive(marketId), "Market not active");

        uint256 freeCollateral = HyperCore.getFreeCollateral(address(this));
        uint256 requiredMargin = (size * limitPrice) / (targetLeverage * 1e8);
        require(
            HyperCore.to6Decimals(requiredMargin) <= freeCollateral,
            "Insufficient collateral"
        );

        actionId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: marketId,
                isBuy: false,
                price: limitPrice,
                size: size,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        emit PositionOpened(marketId, false, size, limitPrice);
    }

    /// @notice Close entire position in a market
    function closePosition(uint8 marketId)
        external
        onlyManager
        returns (bytes32 actionId)
    {
        int256 positionSize = HyperCore.getPositionSize(address(this), marketId);
        require(positionSize != 0, "No position to close");

        // Get current price for limit order
        uint256 markPrice = HyperCore.getMarkPrice(marketId);

        // Determine order parameters
        bool isBuy = positionSize < 0; // Buy to close short, sell to close long
        uint256 size = positionSize < 0 ? uint256(-positionSize) : uint256(positionSize);

        // Apply 0.5% slippage tolerance
        uint256 limitPrice;
        if (isBuy) {
            limitPrice = (markPrice * 1005) / 1000; // Pay up to 0.5% above mark
        } else {
            limitPrice = (markPrice * 995) / 1000; // Accept down to 0.5% below mark
        }

        actionId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: marketId,
                isBuy: isBuy,
                price: limitPrice,
                size: size,
                orderType: ICoreWriter.OrderType.Ioc,
                reduceOnly: true
            })
        );

        emit PositionClosed(marketId, size);
    }

    /// @notice Cancel all open orders in a market
    function cancelAllOrders(uint8 marketId)
        external
        onlyManager
        returns (bytes32 actionId)
    {
        return coreWriter.cancelAllOrders(marketId);
    }

    /// @notice Update leverage for a market
    function setLeverage(uint8 marketId, uint8 leverage)
        external
        onlyManager
        returns (bytes32 actionId)
    {
        require(leverage >= 1 && leverage <= 50, "Invalid leverage");

        actionId = coreWriter.updateLeverage(marketId, leverage);
        targetLeverage = leverage;

        emit LeverageUpdated(marketId, leverage);
    }

    /// @notice Deposit USDC to trading balance
    function deposit(uint256 amount) external onlyManager returns (bytes32 actionId) {
        // Note: Caller must have approved CoreWriter to spend their USDC
        return coreWriter.depositToCore(amount);
    }

    /// @notice Withdraw USDC from trading balance
    function withdraw(uint256 amount) external onlyManager returns (bytes32 actionId) {
        // Check we have enough withdrawable balance
        uint256 freeCollateral = HyperCore.getFreeCollateral(address(this));
        require(amount <= freeCollateral, "Insufficient withdrawable balance");

        return coreWriter.withdrawToEvm(amount);
    }

    // ============ Admin Functions ============

    /// @notice Transfer manager role
    function setManager(address newManager) external onlyManager {
        require(newManager != address(0), "Invalid manager");
        manager = newManager;
    }

    /// @notice Update maximum position value
    function setMaxPositionValue(uint256 newMax) external onlyManager {
        maxPositionValue = newMax;
    }

    // ============ Modifiers ============

    modifier onlyManager() {
        require(msg.sender == manager, "Not manager");
        _;
    }
}
