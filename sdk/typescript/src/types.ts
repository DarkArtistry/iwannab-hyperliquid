/**
 * Type definitions for HyperCore SDK
 */

// ============ Enums ============

export enum OrderSide {
  Buy = 'buy',
  Sell = 'sell',
}

export enum OrderType {
  Limit = 'limit',
  Market = 'market',
  PostOnly = 'postOnly',
  Ioc = 'ioc',
  Fok = 'fok',
}

export enum TimeInForce {
  Gtc = 'Gtc',
  Ioc = 'Ioc',
  Fok = 'Fok',
  Alo = 'Alo',
}

export enum OrderStatus {
  Open = 'open',
  PartiallyFilled = 'partial',
  Filled = 'filled',
  Canceled = 'canceled',
}

// ============ Request Types ============

export interface OrderRequest {
  /** Market symbol (e.g., 'BTC-PERP') or market ID */
  market: string | number;
  /** Order side */
  side: OrderSide;
  /** Limit price (required for limit orders) */
  price?: string;
  /** Order size */
  size: string;
  /** Order type */
  type?: OrderType;
  /** Reduce-only flag */
  reduceOnly?: boolean;
  /** Client order ID for tracking */
  clientOrderId?: string;
}

export interface CancelRequest {
  /** Market symbol or ID */
  market: string | number;
  /** Order ID to cancel */
  orderId: number;
}

export interface CancelByCloidRequest {
  /** Market symbol or ID */
  market: string | number;
  /** Client order ID */
  clientOrderId: string;
}

export interface LeverageUpdateRequest {
  /** Market symbol or ID */
  market: string | number;
  /** New leverage value (1-100) */
  leverage: number;
  /** Cross margin mode */
  isCross?: boolean;
}

export interface TransferRequest {
  /** Destination address */
  destination: string;
  /** Amount to transfer (USDC) */
  amount: string;
}

// ============ Response Types ============

export interface MarketMeta {
  name: string;
  szDecimals: number;
  maxLeverage: number;
  tickSize: string;
  lotSize: string;
  makerFee: string;
  takerFee: string;
}

export interface Position {
  coin: string;
  size: string;
  entryPx: string;
  unrealizedPnl: string;
  realizedPnl: string;
  leverage: number;
  liquidationPx: string | null;
  marginUsed: string;
  returnOnEquity: string;
}

export interface Order {
  coin: string;
  oid: number;
  side: 'B' | 'A';
  limitPx: string;
  sz: string;
  origSz: string;
  timestamp: number;
  cloid?: string;
}

export interface Fill {
  coin: string;
  px: string;
  sz: string;
  side: 'B' | 'A';
  time: number;
  fee: string;
  oid: number;
  tid: number;
  crossed: boolean;
  closedPnl: string;
}

export interface MarginSummary {
  accountValue: string;
  totalNtlPos: string;
  totalRawUsd: string;
  totalMarginUsed: string;
  withdrawable: string;
}

export interface AccountState {
  marginSummary: MarginSummary;
  crossMarginSummary: MarginSummary;
  assetPositions: Array<{ position: Position }>;
}

export interface L2Level {
  px: string;
  sz: string;
  n: number;
}

export interface L2Book {
  coin: string;
  time: number;
  levels: [L2Level[], L2Level[]]; // [bids, asks]
}

export interface FundingRate {
  coin: string;
  fundingRate: string;
  premium: string;
  time: number;
}

// ============ WebSocket Types ============

export interface WsSubscription {
  type: string;
  [key: string]: unknown;
}

export interface WsMessage {
  channel: string;
  data: unknown;
}

export interface TradeMessage {
  coin: string;
  side: 'B' | 'A';
  px: string;
  sz: string;
  time: number;
  hash: string;
  tid: number;
}

export interface L2BookUpdate {
  coin: string;
  time: number;
  levels: [L2Level[], L2Level[]];
}

export interface UserUpdate {
  fills?: Fill[];
  orders?: Array<{
    order: Order;
    status: string;
    statusTimestamp: number;
  }>;
}

// ============ Config Types ============

export interface HyperCoreConfig {
  /** Base URL for HTTP API */
  baseUrl?: string;
  /** WebSocket URL */
  wsUrl?: string;
  /** Private key for signing */
  privateKey?: string;
  /** Chain ID */
  chainId?: number;
  /** Request timeout in ms */
  timeout?: number;
}

// ============ Error Types ============

export interface ApiError {
  status: 'err';
  code: number;
  message: string;
}

export class HyperCoreError extends Error {
  code: number;

  constructor(message: string, code: number) {
    super(message);
    this.name = 'HyperCoreError';
    this.code = code;
  }
}

// ============ Signature Types ============

export interface Signature {
  r: string;
  s: string;
  v: number;
}

export interface SignedRequest<T> {
  action: T;
  nonce: number;
  signature: Signature;
}
