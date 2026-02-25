// ============================================================================
// Spot Market Types
// ============================================================================

export interface SpotToken {
  index: number;
  name: string;
  symbol: string;
  weiDecimals: number;
  szDecimals: number;
  systemAddress?: string;
  maxSupply?: string;
}

export interface SpotMarketMeta {
  name: string;       // e.g. "BTC-USDC"
  id: number;         // market ID (128+)
  baseToken: number;  // base token index
  quoteToken: number; // quote token index (0 = USDC)
  tickSize: string;
  lotSize: string;
  minOrderSize: string;
  makerFee: string;
  takerFee: string;
}

export interface SpotMetaResponse {
  tokens: SpotToken[];
  universe: SpotMarketMeta[];
}

export interface SpotBalance {
  symbol: string;
  tokenIndex: number;
  total: string;
  reserved: string;
  available: string;
}

// ============================================================================
// Shared Types
// ============================================================================

export interface L2Level {
  px: string;
  sz: string;
  n: number;
}

export interface L2BookResponse {
  coin: string;
  time: number;
  levels: [L2Level[], L2Level[]]; // [bids, asks]
}

export interface OpenOrder {
  coin: string;
  oid: number;
  side: string;
  limitPx: string;
  sz: string;
  origSz: string;
  timestamp: number;
  cloid?: string;
  marketId?: number;
}

export interface TradeData {
  coin: string;
  side: string;
  px: string;
  sz: string;
  time: number;
  hash: string;
}

export interface CandleData {
  T: number;
  o: string;
  h: string;
  l: string;
  c: string;
  v: string;
  n: number;
}

// ============================================================================
// Spot Exchange API Types
// ============================================================================

export interface SpotOrderWire {
  a: number;    // spot market ID
  b: boolean;   // is_buy
  p: string;    // price
  s: string;    // size
  t: OrderTypeWire; // time in force (no reduce_only for spot)
  c?: string;   // client order id
}

export type OrderTypeWire =
  | { limit: { tif: string } }
  | { trigger: { isMarket: boolean; triggerPx: string; tpsl: string } };

export interface SpotCancelWire {
  a: number;
  o: number;
}

export interface SignatureWire {
  r: string;
  s: string;
  v: number;
}

export interface ExchangeRequest {
  action: ExchangeAction;
  nonce: number;
  signature: SignatureWire;
}

export type ExchangeAction =
  | { type: "spotOrder"; orders: SpotOrderWire[]; grouping: string }
  | { type: "spotCancel"; cancels: SpotCancelWire[] }
  | { type: "spotCancelAll" };

// ============================================================================
// WebSocket Types
// ============================================================================

export interface WsSubscription {
  type: string;
  coin?: string;
  user?: string;
  interval?: string;
}

export interface WsMessage {
  channel: string;
  data: unknown;
}

// ============================================================================
// Order Response Types
// ============================================================================

export interface OrderResponse {
  status: string;
  response?: {
    type: string;
    data: {
      statuses: OrderStatus[];
    };
  };
}

export type OrderStatus =
  | { resting: { oid: number } }
  | { filled: { totalSz: string; avgPx: string; oid: number } }
  | { error: string };
