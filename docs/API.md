# API Specification

## Overview

HyperCore exposes three API surfaces:
1. **Info API** (`POST /info`) - Read-only queries
2. **Exchange API** (`POST /exchange`) - Signed actions
3. **WebSocket** (`ws://host:3001`) - Real-time feeds

Base URLs:
- Mainnet: `https://api.hypercore.xyz`
- Testnet: `https://api.testnet.hypercore.xyz`
- Local: `http://localhost:3000`

## Authentication

### Request Signing
All `/exchange` requests must be signed using EIP-712 typed data signatures.

**Signing Domain**
```json
{
  "name": "HyperCore",
  "version": "1",
  "chainId": 1337,
  "verifyingContract": "0x0000000000000000000000000000000000000000"
}
```

**Request Structure**
```json
{
  "action": { ... },
  "nonce": 1704067200000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

**Nonce Policy**
- Use current timestamp in milliseconds
- Must be greater than last used nonce
- Valid within 1 hour of current time

---

## Info API

### Endpoint
```
POST /info
Content-Type: application/json
```

### Request Types

#### `meta` - Exchange Metadata
```json
// Request
{
  "type": "meta"
}

// Response
{
  "universe": [
    {
      "name": "BTC-PERP",
      "szDecimals": 3,
      "maxLeverage": 50,
      "tickSize": "0.1",
      "lotSize": "0.001",
      "makerFee": "0.0002",
      "takerFee": "0.0005"
    },
    {
      "name": "ETH-PERP",
      "szDecimals": 2,
      "maxLeverage": 50,
      "tickSize": "0.01",
      "lotSize": "0.01",
      "makerFee": "0.0002",
      "takerFee": "0.0005"
    }
  ]
}
```

#### `allMids` - All Mid Prices
```json
// Request
{
  "type": "allMids"
}

// Response
{
  "BTC-PERP": "65432.50",
  "ETH-PERP": "3456.78"
}
```

#### `l2Book` - Order Book Snapshot
```json
// Request
{
  "type": "l2Book",
  "coin": "BTC-PERP",
  "nSigFigs": 5,
  "mantissa": null
}

// Response
{
  "coin": "BTC-PERP",
  "time": 1704067200000,
  "levels": [
    [
      // Bids (price descending)
      {"px": "65430.0", "sz": "1.234", "n": 3},
      {"px": "65420.0", "sz": "2.567", "n": 5},
      {"px": "65410.0", "sz": "0.890", "n": 2}
    ],
    [
      // Asks (price ascending)
      {"px": "65435.0", "sz": "0.567", "n": 2},
      {"px": "65440.0", "sz": "1.234", "n": 4},
      {"px": "65450.0", "sz": "3.456", "n": 6}
    ]
  ]
}
```

#### `clearinghouseState` - User Account State
```json
// Request
{
  "type": "clearinghouseState",
  "user": "0x742d35Cc6634C0532925a3b844Bc9e7595f..."
}

// Response
{
  "marginSummary": {
    "accountValue": "10000.000000",
    "totalNtlPos": "50000.000000",
    "totalRawUsd": "10000.000000",
    "totalMarginUsed": "2000.000000",
    "withdrawable": "8000.000000"
  },
  "crossMarginSummary": {
    "accountValue": "10000.000000",
    "totalNtlPos": "50000.000000",
    "totalMarginUsed": "2000.000000"
  },
  "assetPositions": [
    {
      "position": {
        "coin": "BTC-PERP",
        "szi": "0.500000",
        "leverage": {
          "type": "cross",
          "value": 10
        },
        "entryPx": "64000.000000",
        "positionValue": "32716.250000",
        "unrealizedPnl": "716.250000",
        "returnOnEquity": "0.0716",
        "liquidationPx": "58000.000000"
      }
    }
  ]
}
```

#### `openOrders` - User Open Orders
```json
// Request
{
  "type": "openOrders",
  "user": "0x742d35Cc6634C0532925a3b844Bc9e7595f..."
}

// Response
[
  {
    "coin": "BTC-PERP",
    "side": "B",
    "limitPx": "65000.0",
    "sz": "0.100",
    "oid": 12345678,
    "timestamp": 1704067200000,
    "origSz": "0.200",
    "cloid": "my-order-123"
  },
  {
    "coin": "ETH-PERP",
    "side": "A",
    "limitPx": "3500.00",
    "sz": "1.00",
    "oid": 12345679,
    "timestamp": 1704067200100,
    "origSz": "1.00",
    "cloid": null
  }
]
```

#### `userFills` - User Trade History
```json
// Request
{
  "type": "userFills",
  "user": "0x742d35Cc6634C0532925a3b844Bc9e7595f...",
  "startTime": 1704067200000,
  "endTime": 1704153600000,
  "aggregateByTime": false
}

// Response
[
  {
    "coin": "BTC-PERP",
    "px": "65100.0",
    "sz": "0.100",
    "side": "B",
    "time": 1704067200123,
    "startPosition": "0.400",
    "dir": "Open Long",
    "closedPnl": "0.000000",
    "hash": "0x1234567890abcdef...",
    "oid": 12345678,
    "crossed": true,
    "fee": "3.255000",
    "tid": 987654321
  }
]
```

#### `userFundingHistory` - Funding Payments
```json
// Request
{
  "type": "userFundingHistory",
  "user": "0x742d35Cc6634C0532925a3b844Bc9e7595f...",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// Response
[
  {
    "time": 1704096000000,
    "coin": "BTC-PERP",
    "usdc": "-5.234567",
    "szi": "0.500000",
    "fundingRate": "0.000100"
  }
]
```

#### `fundingHistory` - Market Funding History
```json
// Request
{
  "type": "fundingHistory",
  "coin": "BTC-PERP",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// Response
[
  {
    "coin": "BTC-PERP",
    "fundingRate": "0.000100",
    "premium": "0.000095",
    "time": 1704096000000
  }
]
```

---

## Exchange API

### Endpoint
```
POST /exchange
Content-Type: application/json
```

### Action Types

#### `order` - Place Orders
```json
// Request
{
  "action": {
    "type": "order",
    "orders": [
      {
        "a": 0,
        "b": true,
        "p": "65000.0",
        "s": "0.100",
        "r": false,
        "t": {
          "limit": {
            "tif": "Gtc"
          }
        },
        "c": "my-order-123"
      }
    ],
    "grouping": "na"
  },
  "nonce": 1704067200000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

**Order Fields**
| Field | Name | Type | Description |
|-------|------|------|-------------|
| `a` | asset | u8 | Market ID (0=BTC, 1=ETH) |
| `b` | is_buy | bool | true=buy, false=sell |
| `p` | price | string | Limit price |
| `s` | size | string | Order size |
| `r` | reduce_only | bool | Reduce-only flag |
| `t` | order_type | object | Type and TIF |
| `c` | cloid | string? | Client order ID |

**Order Types**
```json
// Good-til-canceled limit
{"limit": {"tif": "Gtc"}}

// Immediate-or-cancel
{"limit": {"tif": "Ioc"}}

// Fill-or-kill
{"limit": {"tif": "Fok"}}

// Post-only (maker only)
{"limit": {"tif": "Alo"}}

// Market order
{"trigger": {"isMarket": true, "triggerPx": "0", "tpsl": "tp"}}
```

**Grouping**
- `"na"` - No grouping (normal orders)
- `"normalTpsl"` - TP/SL attached to position
- `"positionTpsl"` - Position-wide TP/SL

```json
// Response (success)
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "oid": 12345678,
            "cloid": "my-order-123"
          }
        }
      ]
    }
  }
}

// Response (partial fill)
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "filled": {
            "totalSz": "0.050",
            "avgPx": "65010.5",
            "oid": 12345678
          }
        }
      ]
    }
  }
}

// Response (error)
{
  "status": "err",
  "response": "Insufficient margin"
}
```

#### `cancel` - Cancel Orders
```json
// Request
{
  "action": {
    "type": "cancel",
    "cancels": [
      {"a": 0, "o": 12345678}
    ]
  },
  "nonce": 1704067200000,
  "signature": {...}
}

// Response
{
  "status": "ok",
  "response": {
    "type": "cancel",
    "data": {
      "statuses": ["success"]
    }
  }
}
```

#### `cancelByCloid` - Cancel by Client Order ID
```json
// Request
{
  "action": {
    "type": "cancelByCloid",
    "cancels": [
      {"asset": 0, "cloid": "my-order-123"}
    ]
  },
  "nonce": 1704067200000,
  "signature": {...}
}
```

#### `batchModify` - Modify Orders
```json
// Request
{
  "action": {
    "type": "batchModify",
    "modifies": [
      {
        "oid": 12345678,
        "order": {
          "a": 0,
          "b": true,
          "p": "65100.0",
          "s": "0.150",
          "r": false,
          "t": {"limit": {"tif": "Gtc"}}
        }
      }
    ]
  },
  "nonce": 1704067200000,
  "signature": {...}
}
```

#### `updateLeverage` - Update Leverage
```json
// Request
{
  "action": {
    "type": "updateLeverage",
    "asset": 0,
    "isCross": true,
    "leverage": 20
  },
  "nonce": 1704067200000,
  "signature": {...}
}

// Response
{
  "status": "ok",
  "response": {
    "type": "updateLeverage",
    "data": {
      "leverage": 20,
      "isCross": true
    }
  }
}
```

#### `updateIsolatedMargin` - Add/Remove Margin
```json
// Request
{
  "action": {
    "type": "updateIsolatedMargin",
    "asset": 0,
    "isBuy": true,
    "ntli": 100000000
  },
  "nonce": 1704067200000,
  "signature": {...}
}
```

#### `usdTransfer` - Transfer USDC
```json
// Request
{
  "action": {
    "type": "usdTransfer",
    "destination": "0x...",
    "amount": "1000.000000"
  },
  "nonce": 1704067200000,
  "signature": {...}
}
```

#### `withdraw` - Withdraw to L1
```json
// Request
{
  "action": {
    "type": "withdraw",
    "destination": "0x...",
    "amount": "1000.000000"
  },
  "nonce": 1704067200000,
  "signature": {...}
}
```

---

## WebSocket API

### Connection
```
ws://localhost:3001
wss://ws.hypercore.xyz
```

### Subscription Format
```json
{
  "method": "subscribe",
  "subscription": {
    "type": "<feed_type>",
    ...params
  }
}
```

### Unsubscribe
```json
{
  "method": "unsubscribe",
  "subscription": {
    "type": "<feed_type>",
    ...params
  }
}
```

### Feed Types

#### `allMids` - All Mid Prices
```json
// Subscribe
{"method": "subscribe", "subscription": {"type": "allMids"}}

// Updates
{
  "channel": "allMids",
  "data": {
    "mids": {
      "BTC-PERP": "65432.5",
      "ETH-PERP": "3456.78"
    }
  }
}
```

#### `l2Book` - Order Book
```json
// Subscribe
{
  "method": "subscribe",
  "subscription": {
    "type": "l2Book",
    "coin": "BTC-PERP"
  }
}

// Snapshot
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC-PERP",
    "time": 1704067200000,
    "levels": [[...bids], [...asks]]
  }
}

// Delta updates
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC-PERP",
    "time": 1704067200100,
    "levels": [
      [{"px": "65430.0", "sz": "1.500", "n": 4}],
      []
    ]
  }
}
```

#### `trades` - Trade Feed
```json
// Subscribe
{
  "method": "subscribe",
  "subscription": {
    "type": "trades",
    "coin": "BTC-PERP"
  }
}

// Updates
{
  "channel": "trades",
  "data": [
    {
      "coin": "BTC-PERP",
      "side": "B",
      "px": "65100.0",
      "sz": "0.100",
      "hash": "0x...",
      "time": 1704067200123,
      "tid": 987654321
    }
  ]
}
```

#### `user` - User Updates (Authenticated)
```json
// Subscribe (requires signature)
{
  "method": "subscribe",
  "subscription": {
    "type": "user",
    "user": "0x..."
  },
  "signature": {...}
}

// Fill notification
{
  "channel": "user",
  "data": {
    "fills": [
      {
        "coin": "BTC-PERP",
        "px": "65100.0",
        "sz": "0.100",
        "side": "B",
        "time": 1704067200123,
        "oid": 12345678,
        "fee": "3.255"
      }
    ]
  }
}

// Order update
{
  "channel": "user",
  "data": {
    "orders": [
      {
        "order": {
          "coin": "BTC-PERP",
          "oid": 12345678,
          "side": "B",
          "limitPx": "65000.0",
          "sz": "0.050",
          "origSz": "0.100"
        },
        "status": "partiallyFilled",
        "statusTimestamp": 1704067200123
      }
    ]
  }
}

// Liquidation warning
{
  "channel": "user",
  "data": {
    "liquidation": {
      "coin": "BTC-PERP",
      "leverage": 25,
      "marginRatio": "1.05",
      "isWarning": true
    }
  }
}
```

#### `candle` - Candlestick Data
```json
// Subscribe
{
  "method": "subscribe",
  "subscription": {
    "type": "candle",
    "coin": "BTC-PERP",
    "interval": "1m"
  }
}

// Intervals: 1m, 5m, 15m, 1h, 4h, 1d

// Updates
{
  "channel": "candle",
  "data": {
    "coin": "BTC-PERP",
    "interval": "1m",
    "t": 1704067200000,
    "o": "65000.0",
    "h": "65150.0",
    "l": "64950.0",
    "c": "65100.0",
    "v": "123.456"
  }
}
```

---

## Error Codes

| Code | Message | Description |
|------|---------|-------------|
| 1001 | InvalidSignature | Signature verification failed |
| 1002 | InvalidNonce | Nonce too old or already used |
| 1003 | InsufficientMargin | Not enough free collateral |
| 1004 | InvalidPrice | Price not multiple of tick size |
| 1005 | InvalidSize | Size below lot size |
| 1006 | OrderNotFound | Order ID doesn't exist |
| 1007 | MarketNotFound | Invalid market ID |
| 1008 | PositionNotFound | No position in market |
| 1009 | MaxOrdersExceeded | Too many open orders |
| 1010 | MaxPositionExceeded | Position would exceed limit |
| 1011 | ReduceOnlyViolation | Reduce-only would increase position |
| 1012 | PostOnlyViolation | Post-only order would cross |
| 1013 | MarketPaused | Market not accepting orders |
| 1014 | RateLimited | Too many requests |
| 1015 | InternalError | Unexpected server error |

---

## Rate Limits

| Endpoint | Limit | Window |
|----------|-------|--------|
| /info | 100 | 1 second |
| /exchange | 50 | 1 second |
| WebSocket subscriptions | 20 | 1 second |

Rate limit headers:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 1704067201
```
