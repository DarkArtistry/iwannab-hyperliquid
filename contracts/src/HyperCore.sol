// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IPositionPrecompile} from "./interfaces/IPrecompiles.sol";
import {IAccountPrecompile} from "./interfaces/IPrecompiles.sol";
import {IMarketPrecompile} from "./interfaces/IPrecompiles.sol";
import {IOrderPrecompile} from "./interfaces/IPrecompiles.sol";
import {IFundingPrecompile} from "./interfaces/IPrecompiles.sol";
import {IOrderBookPrecompile} from "./interfaces/IPrecompiles.sol";
import {ICoreWriter} from "./interfaces/ICoreWriter.sol";

/// @title HyperCore
/// @notice Library for interacting with HyperCore engine from Solidity
/// @dev Provides ergonomic wrappers around precompiles and CoreWriter
library HyperCore {
    // ============ Precompile Addresses ============

    address internal constant POSITION_PRECOMPILE = 0x0000000000000000000000000000000000000800;
    address internal constant ACCOUNT_PRECOMPILE = 0x0000000000000000000000000000000000000801;
    address internal constant MARKET_PRECOMPILE = 0x0000000000000000000000000000000000000802;
    address internal constant ORDER_PRECOMPILE = 0x0000000000000000000000000000000000000803;
    address internal constant FUNDING_PRECOMPILE = 0x0000000000000000000000000000000000000804;
    address internal constant ORDERBOOK_PRECOMPILE = 0x0000000000000000000000000000000000000805;

    // ============ Market IDs ============

    uint8 internal constant BTC_PERP = 0;
    uint8 internal constant ETH_PERP = 1;

    // ============ Decimals ============

    uint8 internal constant PRICE_DECIMALS = 8;
    uint8 internal constant SIZE_DECIMALS = 8;
    uint8 internal constant USDC_DECIMALS = 6;

    // ============ Position Reads ============

    /// @notice Get position for a user in a market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return position The position data
    function getPosition(address user, uint8 marketId)
        internal
        view
        returns (IPositionPrecompile.Position memory)
    {
        return IPositionPrecompile(POSITION_PRECOMPILE).getPosition(user, marketId);
    }

    /// @notice Get position size (positive = long, negative = short)
    /// @param user The account address
    /// @param marketId The market ID
    /// @return size Position size (8 decimals)
    function getPositionSize(address user, uint8 marketId) internal view returns (int256) {
        return getPosition(user, marketId).size;
    }

    /// @notice Check if user is long in a market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return isLong True if position size > 0
    function isLong(address user, uint8 marketId) internal view returns (bool) {
        return getPositionSize(user, marketId) > 0;
    }

    /// @notice Check if user is short in a market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return isShort True if position size < 0
    function isShort(address user, uint8 marketId) internal view returns (bool) {
        return getPositionSize(user, marketId) < 0;
    }

    /// @notice Check if user has any position in market
    /// @param user The account address
    /// @param marketId The market ID
    /// @return hasPosition True if position size != 0
    function hasPosition(address user, uint8 marketId) internal view returns (bool) {
        return IPositionPrecompile(POSITION_PRECOMPILE).hasPosition(user, marketId);
    }

    /// @notice Get unrealized PnL for a position
    /// @param user The account address
    /// @param marketId The market ID
    /// @return pnl Unrealized PnL (6 decimals, USDC)
    function getUnrealizedPnl(address user, uint8 marketId) internal view returns (int256) {
        return getPosition(user, marketId).unrealizedPnl;
    }

    // ============ Account Reads ============

    /// @notice Get full account state
    /// @param user The account address
    /// @return state Account state
    function getAccountState(address user)
        internal
        view
        returns (IAccountPrecompile.AccountState memory)
    {
        return IAccountPrecompile(ACCOUNT_PRECOMPILE).getAccountState(user);
    }

    /// @notice Get account balance
    /// @param user The account address
    /// @return balance Available balance (6 decimals)
    function getBalance(address user) internal view returns (uint256) {
        return IAccountPrecompile(ACCOUNT_PRECOMPILE).getBalance(user);
    }

    /// @notice Get account equity (balance + unrealized PnL)
    /// @param user The account address
    /// @return equity Total equity (6 decimals)
    function getEquity(address user) internal view returns (uint256) {
        return IAccountPrecompile(ACCOUNT_PRECOMPILE).getEquity(user);
    }

    /// @notice Get free collateral (equity - margin used)
    /// @param user The account address
    /// @return freeCollateral Available collateral (6 decimals)
    function getFreeCollateral(address user) internal view returns (uint256) {
        return getAccountState(user).freeCollateral;
    }

    /// @notice Check if account is healthy (above liquidation threshold)
    /// @param user The account address
    /// @return healthy True if account is healthy
    function isAccountHealthy(address user) internal view returns (bool) {
        return IAccountPrecompile(ACCOUNT_PRECOMPILE).isHealthy(user);
    }

    // ============ Market Reads ============

    /// @notice Get full market state
    /// @param marketId The market ID
    /// @return state Market state
    function getMarketState(uint8 marketId)
        internal
        view
        returns (IMarketPrecompile.MarketState memory)
    {
        return IMarketPrecompile(MARKET_PRECOMPILE).getMarketState(marketId);
    }

    /// @notice Get mark price for a market
    /// @param marketId The market ID
    /// @return price Mark price (8 decimals)
    function getMarkPrice(uint8 marketId) internal view returns (uint256) {
        return IMarketPrecompile(MARKET_PRECOMPILE).getMarkPrice(marketId);
    }

    /// @notice Get index price for a market
    /// @param marketId The market ID
    /// @return price Index price (8 decimals)
    function getIndexPrice(uint8 marketId) internal view returns (uint256) {
        return IMarketPrecompile(MARKET_PRECOMPILE).getIndexPrice(marketId);
    }

    /// @notice Check if market is active
    /// @param marketId The market ID
    /// @return active True if market accepts orders
    function isMarketActive(uint8 marketId) internal view returns (bool) {
        return IMarketPrecompile(MARKET_PRECOMPILE).isActive(marketId);
    }

    // ============ Funding Reads ============

    /// @notice Get current funding rate
    /// @param marketId The market ID
    /// @return rate Funding rate (10 decimals)
    /// @return nextTime Next funding time (ms)
    function getFundingRate(uint8 marketId) internal view returns (int256 rate, uint256 nextTime) {
        return IFundingPrecompile(FUNDING_PRECOMPILE).getFundingRate(marketId);
    }

    /// @notice Get pending funding for a position
    /// @param user The account address
    /// @param marketId The market ID
    /// @return pending Pending funding (positive = owes)
    function getPendingFunding(address user, uint8 marketId) internal view returns (int256) {
        return IFundingPrecompile(FUNDING_PRECOMPILE).getPendingFunding(user, marketId);
    }

    // ============ Order Book Reads ============

    /// @notice Get best bid and ask
    /// @param marketId The market ID
    /// @return bid Best bid price (8 decimals)
    /// @return ask Best ask price (8 decimals)
    function getBestBidAsk(uint8 marketId) internal view returns (uint256 bid, uint256 ask) {
        return IOrderBookPrecompile(ORDERBOOK_PRECOMPILE).getBestBidAsk(marketId);
    }

    /// @notice Get mid price
    /// @param marketId The market ID
    /// @return mid Mid price (8 decimals)
    function getMidPrice(uint8 marketId) internal view returns (uint256) {
        return IOrderBookPrecompile(ORDERBOOK_PRECOMPILE).getMidPrice(marketId);
    }

    /// @notice Get spread
    /// @param marketId The market ID
    /// @return spread Bid-ask spread (8 decimals)
    function getSpread(uint8 marketId) internal view returns (uint256) {
        return IOrderBookPrecompile(ORDERBOOK_PRECOMPILE).getSpread(marketId);
    }

    /// @notice Get order book depth
    /// @param marketId The market ID
    /// @param depth Number of levels
    /// @return bids Bid levels
    /// @return asks Ask levels
    function getOrderBook(uint8 marketId, uint8 depth)
        internal
        view
        returns (IOrderBookPrecompile.Level[] memory bids, IOrderBookPrecompile.Level[] memory asks)
    {
        return IOrderBookPrecompile(ORDERBOOK_PRECOMPILE).getOrderBook(marketId, depth);
    }

    // ============ Order Reads ============

    /// @notice Get open orders for user
    /// @param user The account address
    /// @param marketId The market ID
    /// @return orders Open orders
    function getOpenOrders(address user, uint8 marketId)
        internal
        view
        returns (IOrderPrecompile.Order[] memory)
    {
        return IOrderPrecompile(ORDER_PRECOMPILE).getOpenOrders(user, marketId);
    }

    /// @notice Get open order count
    /// @param user The account address
    /// @param marketId The market ID
    /// @return count Number of open orders
    function getOpenOrderCount(address user, uint8 marketId) internal view returns (uint256) {
        return IOrderPrecompile(ORDER_PRECOMPILE).getOpenOrderCount(user, marketId);
    }

    // ============ Utility Functions ============

    /// @notice Convert USDC amount to 8 decimal format
    /// @param usdcAmount Amount in 6 decimals
    /// @return Amount in 8 decimals
    function usdcTo8Decimals(uint256 usdcAmount) internal pure returns (uint256) {
        return usdcAmount * 100;
    }

    /// @notice Convert 8 decimal amount to USDC format
    /// @param amount Amount in 8 decimals
    /// @return usdcAmount Amount in 6 decimals
    function to6Decimals(uint256 amount) internal pure returns (uint256) {
        return amount / 100;
    }

    /// @notice Calculate notional value
    /// @param size Position size (8 decimals)
    /// @param price Price (8 decimals)
    /// @return notional Notional value (8 decimals)
    function calculateNotional(int256 size, uint256 price) internal pure returns (uint256) {
        int256 result = size * int256(price) / 1e8;
        return result < 0 ? uint256(-result) : uint256(result);
    }

    /// @notice Calculate position value at current mark price
    /// @param user The account address
    /// @param marketId The market ID
    /// @return value Position value (8 decimals)
    function getPositionValue(address user, uint8 marketId) internal view returns (uint256) {
        int256 size = getPositionSize(user, marketId);
        uint256 markPrice = getMarkPrice(marketId);
        return calculateNotional(size, markPrice);
    }
}
