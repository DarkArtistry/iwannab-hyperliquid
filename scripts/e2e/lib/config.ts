/**
 * E2E Test Configuration
 *
 * Centralized configuration for all E2E tests. Values can be overridden
 * via environment variables.
 */

export const CONFIG = {
  /** Gateway API URL for HyperCore trading operations */
  GATEWAY_URL: process.env.GATEWAY_URL || 'http://localhost:3000',

  /** EVM JSON-RPC URL for Ethereum-compatible operations */
  EVM_RPC_URL: process.env.EVM_RPC_URL || 'http://localhost:8545',

  /** WebSocket URL for real-time updates */
  WS_URL: process.env.WS_URL || 'ws://localhost:3000/ws',

  /** Chain ID (1337 is standard local development chain) */
  CHAIN_ID: parseInt(process.env.CHAIN_ID || '1337'),

  /** Enable verbose logging */
  VERBOSE: process.env.VERBOSE === 'true',

  /** Request timeout in milliseconds */
  TIMEOUT: 10000,
};

export const MARKETS = {
  BTC_PERP: 'BTC-PERP',
  ETH_PERP: 'ETH-PERP',
};
