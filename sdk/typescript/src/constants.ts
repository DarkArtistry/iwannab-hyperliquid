/**
 * SDK Constants
 */

// Default endpoints
export const DEFAULT_BASE_URL = 'https://api.hypercore.xyz';
export const DEFAULT_WS_URL = 'wss://ws.hypercore.xyz';
export const DEFAULT_TESTNET_URL = 'https://api.testnet.hypercore.xyz';
export const DEFAULT_TESTNET_WS_URL = 'wss://ws.testnet.hypercore.xyz';

// Chain ID
export const DEFAULT_CHAIN_ID = 1337;

// Market IDs
export const MARKETS = {
  'BTC-PERP': 0,
  'ETH-PERP': 1,
} as const;

// Decimals
export const PRICE_DECIMALS = 8;
export const SIZE_DECIMALS = 8;
export const USDC_DECIMALS = 6;

// EIP-712 Domain
export const EIP712_DOMAIN = {
  name: 'HyperCore',
  version: '1',
  chainId: DEFAULT_CHAIN_ID,
  verifyingContract: '0x0000000000000000000000000000000000000000' as const,
};

// EIP-712 Types
export const EIP712_TYPES = {
  Order: [
    { name: 'marketId', type: 'uint8' },
    { name: 'isBuy', type: 'bool' },
    { name: 'price', type: 'string' },
    { name: 'size', type: 'string' },
    { name: 'orderType', type: 'uint8' },
    { name: 'reduceOnly', type: 'bool' },
    { name: 'nonce', type: 'uint64' },
  ],
  Cancel: [
    { name: 'marketId', type: 'uint8' },
    { name: 'orderId', type: 'uint64' },
    { name: 'nonce', type: 'uint64' },
  ],
  UpdateLeverage: [
    { name: 'marketId', type: 'uint8' },
    { name: 'leverage', type: 'uint8' },
    { name: 'isCross', type: 'bool' },
    { name: 'nonce', type: 'uint64' },
  ],
  Transfer: [
    { name: 'destination', type: 'address' },
    { name: 'amount', type: 'string' },
    { name: 'nonce', type: 'uint64' },
  ],
} as const;

// Error codes
export const ERROR_CODES = {
  INVALID_SIGNATURE: 1001,
  INVALID_NONCE: 1002,
  INSUFFICIENT_MARGIN: 1003,
  INVALID_PRICE: 1004,
  INVALID_SIZE: 1005,
  ORDER_NOT_FOUND: 1006,
  MARKET_NOT_FOUND: 1007,
  POSITION_NOT_FOUND: 1008,
  MAX_ORDERS_EXCEEDED: 1009,
  MAX_POSITION_EXCEEDED: 1010,
  REDUCE_ONLY_VIOLATION: 1011,
  POST_ONLY_VIOLATION: 1012,
  MARKET_PAUSED: 1013,
  RATE_LIMITED: 1014,
  INTERNAL_ERROR: 1015,
} as const;
