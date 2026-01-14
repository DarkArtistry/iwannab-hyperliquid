// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title IPositionPrecompile
/// @notice Read-only precompile for querying position data
/// @dev Address: 0x0000000000000000000000000000000000000800
interface IPositionPrecompile {
    struct Position {
        int256 size;           // Positive = long, negative = short (8 decimals)
        uint256 entryPrice;    // Average entry price (8 decimals)
        int256 unrealizedPnl;  // Unrealized profit/loss (6 decimals, USDC)
        int256 fundingOwed;    // Pending funding payment (6 decimals)
    }

    /// @notice Get position for a user in a market
    /// @param user The account address
    /// @param marketId The market ID (0 = BTC-PERP, 1 = ETH-PERP)
    /// @return position The position data
    function getPosition(address user, uint8 marketId) external view returns (Position memory);

    /// @notice Check if user has an open position in market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return hasPosition True if position exists and size != 0
    function hasPosition(address user, uint8 marketId) external view returns (bool);
}

/// @title IAccountPrecompile
/// @notice Read-only precompile for querying account state
/// @dev Address: 0x0000000000000000000000000000000000000801
interface IAccountPrecompile {
    struct AccountState {
        uint256 balance;                // Available balance (6 decimals)
        uint256 equity;                 // Total equity including unrealized PnL (6 decimals)
        uint256 freeCollateral;         // Collateral available for new positions (6 decimals)
        uint256 totalInitialMargin;     // Total initial margin required (6 decimals)
        uint256 totalMaintenanceMargin; // Total maintenance margin required (6 decimals)
    }

    /// @notice Get account state for a user
    /// @param user The account address
    /// @return state The account state
    function getAccountState(address user) external view returns (AccountState memory);

    /// @notice Get user's USDC balance (available, not including unrealized PnL)
    /// @param user The account address
    /// @return balance The balance in USDC (6 decimals)
    function getBalance(address user) external view returns (uint256);

    /// @notice Get user's total equity
    /// @param user The account address
    /// @return equity Total equity (6 decimals)
    function getEquity(address user) external view returns (uint256);

    /// @notice Check if account is healthy (equity > maintenance margin)
    /// @param user The account address
    /// @return healthy True if account is above liquidation threshold
    function isHealthy(address user) external view returns (bool);
}

/// @title IMarketPrecompile
/// @notice Read-only precompile for querying market state
/// @dev Address: 0x0000000000000000000000000000000000000802
interface IMarketPrecompile {
    struct MarketState {
        uint256 markPrice;        // Current mark price (8 decimals)
        uint256 indexPrice;       // Oracle index price (8 decimals)
        int256 fundingRate;       // Current funding rate (10 decimals)
        uint256 nextFundingTime;  // Next funding timestamp (ms)
        uint256 openInterestLong; // Total long open interest (8 decimals)
        uint256 openInterestShort;// Total short open interest (8 decimals)
    }

    /// @notice Get market state
    /// @param marketId The market ID
    /// @return state The market state
    function getMarketState(uint8 marketId) external view returns (MarketState memory);

    /// @notice Get mark price for a market
    /// @param marketId The market ID
    /// @return price The mark price (8 decimals)
    function getMarkPrice(uint8 marketId) external view returns (uint256);

    /// @notice Get index price for a market
    /// @param marketId The market ID
    /// @return price The index price (8 decimals)
    function getIndexPrice(uint8 marketId) external view returns (uint256);

    /// @notice Check if market is active
    /// @param marketId The market ID
    /// @return active True if market accepts orders
    function isActive(uint8 marketId) external view returns (bool);
}

/// @title IOrderPrecompile
/// @notice Read-only precompile for querying order data
/// @dev Address: 0x0000000000000000000000000000000000000803
interface IOrderPrecompile {
    struct Order {
        uint256 orderId;   // Unique order ID
        uint8 marketId;    // Market ID
        bool isBuy;        // True = buy, false = sell
        uint256 price;     // Limit price (8 decimals)
        uint256 size;      // Original size (8 decimals)
        uint256 remaining; // Remaining size (8 decimals)
        uint64 timestamp;  // Creation timestamp (ms)
    }

    /// @notice Get open orders for user in a market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return orders Array of open orders
    function getOpenOrders(address user, uint8 marketId) external view returns (Order[] memory);

    /// @notice Get count of open orders
    /// @param user The account address
    /// @param marketId The market ID
    /// @return count Number of open orders
    function getOpenOrderCount(address user, uint8 marketId) external view returns (uint256);

    /// @notice Get a specific order by ID
    /// @param marketId The market ID
    /// @param orderId The order ID
    /// @return order The order data
    function getOrder(uint8 marketId, uint256 orderId) external view returns (Order memory);
}

/// @title IFundingPrecompile
/// @notice Read-only precompile for funding rate data
/// @dev Address: 0x0000000000000000000000000000000000000804
interface IFundingPrecompile {
    /// @notice Get current funding rate
    /// @param marketId The market ID
    /// @return rate The funding rate (10 decimals, signed)
    /// @return nextTime Next funding settlement timestamp (ms)
    function getFundingRate(uint8 marketId) external view returns (int256 rate, uint256 nextTime);

    /// @notice Get funding accumulator (for position settlement)
    /// @param marketId The market ID
    /// @return accumulator The funding accumulator value
    function getFundingAccumulator(uint8 marketId) external view returns (int256);

    /// @notice Calculate pending funding for a position
    /// @param user The account address
    /// @param marketId The market ID
    /// @return pending The pending funding payment (positive = user owes)
    function getPendingFunding(address user, uint8 marketId) external view returns (int256);
}

/// @title IOrderBookPrecompile
/// @notice Read-only precompile for order book data
/// @dev Address: 0x0000000000000000000000000000000000000805
interface IOrderBookPrecompile {
    struct Level {
        uint256 price;      // Price level (8 decimals)
        uint256 size;       // Total size at level (8 decimals)
        uint32 orderCount;  // Number of orders at level
    }

    /// @notice Get aggregated order book
    /// @param marketId The market ID
    /// @param depth Number of levels to return
    /// @return bids Bid levels (price descending)
    /// @return asks Ask levels (price ascending)
    function getOrderBook(uint8 marketId, uint8 depth)
        external
        view
        returns (Level[] memory bids, Level[] memory asks);

    /// @notice Get best bid and ask prices
    /// @param marketId The market ID
    /// @return bid Best bid price (0 if no bids)
    /// @return ask Best ask price (0 if no asks)
    function getBestBidAsk(uint8 marketId) external view returns (uint256 bid, uint256 ask);

    /// @notice Get mid price
    /// @param marketId The market ID
    /// @return mid The mid price (average of best bid/ask)
    function getMidPrice(uint8 marketId) external view returns (uint256);

    /// @notice Get spread
    /// @param marketId The market ID
    /// @return spread The bid-ask spread (8 decimals)
    function getSpread(uint8 marketId) external view returns (uint256);
}
