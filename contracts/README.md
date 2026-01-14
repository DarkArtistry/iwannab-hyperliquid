# HyperCore Smart Contracts

Solidity contracts for interacting with the HyperCore exchange from the EVM environment.

## Overview

HyperCore uses a dual-execution model:

1. **Read Operations**: Use precompiles at addresses `0x0800-0x0805` to read exchange state
2. **Write Operations**: Use `CoreWriter` contract which queues actions for next-block execution

```
┌─────────────────────────────────────────────────────────────┐
│                    Your Solidity Contract                    │
└─────────────────────────────────────────────────────────────┘
           │                                    │
           │ Read (sync)                        │ Write (queued)
           ▼                                    ▼
┌─────────────────────┐              ┌─────────────────────┐
│     Precompiles     │              │     CoreWriter      │
│  (0x0800-0x0805)    │              │   (ActionQueued)    │
│                     │              │                     │
│ • getPosition()     │              │ • placeOrder()      │
│ • getBalance()      │              │ • cancelOrder()     │
│ • getMarkPrice()    │              │ • depositToCore()   │
│ • getBestBidAsk()   │              │ • withdrawToEvm()   │
└─────────────────────┘              └─────────────────────┘
           │                                    │
           └────────────────┬───────────────────┘
                            │
                            ▼
              ┌─────────────────────────┐
              │    HyperCore Engine     │
              │   (Off-chain state)     │
              └─────────────────────────┘
```

## Installation

```bash
# Install Foundry
curl -L https://foundry.paradigm.xyz | bash
foundryup

# Install dependencies
forge install

# Build
forge build

# Test
forge test
```

## Contract Structure

```
contracts/
├── src/
│   ├── interfaces/
│   │   ├── ICoreWriter.sol      # Write operations interface
│   │   └── IPrecompiles.sol     # Precompile interfaces
│   ├── CoreWriter.sol           # Main write contract
│   ├── HyperCore.sol            # Helper library for reads
│   └── examples/
│       └── VaultExample.sol     # Example integration
├── test/
│   ├── CoreWriter.t.sol         # Unit tests (21 tests)
│   └── Integration.t.sol        # Integration tests (28 tests)
└── script/
    └── Deploy.s.sol             # Deployment script
```

## Usage

### Reading Exchange State

Use the `HyperCore` library to read state via precompiles:

```solidity
import {HyperCore} from "./HyperCore.sol";
import {IPositionPrecompile} from "./interfaces/IPrecompiles.sol";

contract MyStrategy {
    // Read position
    function getMyPosition(uint8 marketId) external view returns (int256 size) {
        return HyperCore.getPositionSize(address(this), marketId);
    }

    // Read full position data
    function getPositionDetails(address user, uint8 marketId)
        external
        view
        returns (int256 size, uint256 entryPrice, int256 unrealizedPnl)
    {
        IPositionPrecompile.Position memory pos = HyperCore.getPosition(user, marketId);
        return (pos.size, pos.entryPrice, pos.unrealizedPnl);
    }

    // Read account data
    function getAccountInfo(address user) external view returns (
        uint256 balance,
        uint256 equity,
        uint256 freeCollateral
    ) {
        balance = HyperCore.getBalance(user);
        equity = HyperCore.getEquity(user);
        freeCollateral = HyperCore.getFreeCollateral(user);
    }

    // Read market data
    function getMarketInfo(uint8 marketId) external view returns (
        uint256 markPrice,
        uint256 indexPrice,
        uint256 bestBid,
        uint256 bestAsk
    ) {
        markPrice = HyperCore.getMarkPrice(marketId);
        indexPrice = HyperCore.getIndexPrice(marketId);
        (bestBid, bestAsk) = HyperCore.getBestBidAsk(marketId);
    }

    // Read funding rate
    function getFundingInfo(uint8 marketId) external view returns (
        int256 rate,
        uint256 nextFundingTime
    ) {
        return HyperCore.getFundingRate(marketId);
    }
}
```

### Writing to Exchange

Use `CoreWriter` for write operations. Actions are queued and executed in the NEXT block:

```solidity
import {ICoreWriter} from "./interfaces/ICoreWriter.sol";

contract TradingBot {
    ICoreWriter public coreWriter;

    constructor(address _coreWriter) {
        coreWriter = ICoreWriter(_coreWriter);
    }

    // Place a limit order
    function placeLimitBuy(uint8 marketId, uint256 price, uint256 size)
        external
        returns (bytes32 actionId)
    {
        return coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: marketId,
                isBuy: true,
                price: price,
                size: size,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
    }

    // Cancel an order
    function cancelOrder(uint8 marketId, uint256 orderId)
        external
        returns (bytes32 actionId)
    {
        return coreWriter.cancelOrder(marketId, orderId);
    }

    // Update leverage
    function setLeverage(uint8 marketId, uint8 leverage)
        external
        returns (bytes32 actionId)
    {
        return coreWriter.updateLeverage(marketId, leverage);
    }

    // Check action status
    function checkAction(bytes32 actionId)
        external
        view
        returns (ICoreWriter.ActionStatus status)
    {
        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        return result.status;
    }
}
```

### VaultExample

See `src/examples/VaultExample.sol` for a complete vault implementation:

```solidity
import {VaultExample} from "./examples/VaultExample.sol";

// Deploy vault
VaultExample vault = new VaultExample(coreWriterAddress);

// Check vault position
(int256 size, uint256 entryPrice, int256 pnl) = vault.getVaultPosition(0);

// Open position (manager only)
vault.openLong(0, 1e8, 65000e8);  // 1 BTC at $65,000

// Close position
vault.closePosition(0);

// Update leverage
vault.setLeverage(0, 20);
```

## Precompile Addresses

| Address | Name | Description |
|---------|------|-------------|
| `0x0800` | PositionReader | Read user positions |
| `0x0801` | AccountReader | Read account balances |
| `0x0802` | MarketReader | Read market state |
| `0x0803` | OrderReader | Read open orders |
| `0x0804` | FundingReader | Read funding rates |
| `0x0805` | OrderBookReader | Read orderbook depth |

## Market IDs

| ID | Market |
|----|--------|
| 0 | BTC-PERP |
| 1 | ETH-PERP |

## Decimals

| Type | Decimals | Example |
|------|----------|---------|
| Price | 8 | `65000e8` = $65,000 |
| Size | 8 | `1e8` = 1.0 BTC |
| USDC | 6 | `1000e6` = $1,000 |

## Testing

```bash
# Run all tests (49 tests)
forge test

# Run specific test file
forge test --match-path test/CoreWriter.t.sol

# Run with verbosity
forge test -vvv

# Run with gas report
forge test --gas-report

# Run specific test
forge test --match-test "test_placeOrder_limit"
```

### Test Coverage

| File | Tests | Description |
|------|-------|-------------|
| CoreWriter.t.sol | 21 | Unit tests for CoreWriter |
| Integration.t.sol | 28 | End-to-end integration tests |

### Integration Test Scenarios

The integration tests cover:

- **Trading Flow**: Deposit → Place Order → Fill → Withdraw
- **Multi-User**: Multiple traders interacting
- **Order Types**: Limit, Market, IOC, FOK, Post-Only
- **Position Management**: Open, close, partial close
- **Error Handling**: Invalid inputs, insufficient funds
- **Gas Optimization**: Benchmarks for common operations

## Deployment

### Local (Anvil)

```bash
# Start Anvil
anvil --chain-id 1337

# Deploy
forge script script/Deploy.s.sol \
  --rpc-url http://127.0.0.1:8545 \
  --broadcast \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --tc DeployScript
```

### Testnet/Mainnet

```bash
# Set environment variables
export RPC_URL="https://..."
export PRIVATE_KEY="0x..."
export USDC_ADDRESS="0x..."  # Real USDC address

# Deploy
forge script script/Deploy.s.sol \
  --rpc-url $RPC_URL \
  --broadcast \
  --private-key $PRIVATE_KEY \
  --verify \
  --etherscan-api-key $ETHERSCAN_API_KEY \
  --tc DeployScript
```

## Events

### CoreWriter Events

```solidity
// Order/action queued
event ActionQueued(
    bytes32 indexed actionId,
    address indexed sender,
    uint8 actionType,
    bytes data
);

// Action executed
event ActionExecuted(
    bytes32 indexed actionId,
    bool success,
    bytes result
);

// Deposit confirmed
event DepositToCore(address indexed account, uint256 amount);

// Withdrawal completed
event WithdrawToEvm(address indexed account, uint256 amount);
```

### Listening for Events

```javascript
// ethers.js example
const filter = coreWriter.filters.ActionQueued();
coreWriter.on(filter, (actionId, sender, actionType, data) => {
  console.log(`Action ${actionId} queued by ${sender}`);
});
```

## Security Considerations

1. **Non-Atomic Writes**: CoreWriter actions execute in the NEXT block. Do not assume immediate execution.

2. **Action Results**: Always check `getActionStatus()` to verify action success.

3. **Reduce-Only**: Use `reduceOnly: true` for stop-loss orders to prevent accidental position increase.

4. **Slippage**: Market orders and IOC have no price guarantee. Use limit prices when possible.

5. **Access Control**: The VaultExample shows manager-only patterns. Implement appropriate access control.

## Gas Costs

| Operation | Approximate Gas |
|-----------|-----------------|
| `placeOrder` | ~40,000 |
| `cancelOrder` | ~30,000 |
| `depositToCore` | ~75,000 |
| `updateLeverage` | ~35,000 |
| `getPosition` (precompile) | ~2,000 |
| `getMarkPrice` (precompile) | ~1,500 |

## License

MIT
