import { API_BASE_URL } from "./constants";
import type {
  SpotMetaResponse,
  L2BookResponse,
  SpotBalance,
  OpenOrder,
  TradeData,
  CandleData,
  ExchangeRequest,
} from "./types";

async function postInfo<T>(body: Record<string, unknown>): Promise<T> {
  const res = await fetch(`${API_BASE_URL}/info`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  if (!res.ok || data.error) {
    throw new Error(data.error || `API error: ${res.status}`);
  }
  return data;
}

async function postExchange(body: ExchangeRequest): Promise<unknown> {
  const res = await fetch(`${API_BASE_URL}/exchange`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  if (!res.ok || data.error) {
    throw new Error(data.error || `Exchange error: ${res.status}`);
  }
  // Check for order-level errors in response
  if (data.response?.data?.statuses) {
    const statuses = data.response.data.statuses;
    const errors = statuses.filter(
      (s: Record<string, unknown>) => typeof s.error === "string"
    );
    if (errors.length > 0) {
      throw new Error(errors.map((e: { error: string }) => e.error).join(", "));
    }
  }
  return data;
}

// ============================================================================
// Spot Info Queries
// ============================================================================

export function fetchSpotMeta(): Promise<SpotMetaResponse> {
  return postInfo({ type: "spotMeta" });
}

export function fetchSpotAllMids(): Promise<Record<string, string>> {
  return postInfo({ type: "spotAllMids" });
}

export async function fetchSpotL2Book(coin: string): Promise<L2BookResponse> {
  const raw = await postInfo<{
    levels: [string[][], string[][]];
    coin?: string;
    time?: number;
  }>({ type: "spotL2Book", coin });

  const normalizeLevels = (levels: string[][]) =>
    levels.map((l) => ({
      px: l[0] ?? "0",
      sz: l[1] ?? "0",
      n: 1,
    }));

  return {
    coin: raw.coin ?? coin,
    time: raw.time ?? Date.now(),
    levels: [normalizeLevels(raw.levels[0]), normalizeLevels(raw.levels[1])],
  };
}

export function fetchSpotBalances(user: string): Promise<SpotBalance[]> {
  return postInfo({ type: "spotBalances", user });
}

export function fetchSpotOpenOrders(user: string): Promise<OpenOrder[]> {
  return postInfo({ type: "spotOpenOrders", user });
}

export function fetchRecentTrades(
  coin: string,
  limit = 50
): Promise<TradeData[]> {
  return postInfo({ type: "recentTrades", coin, limit });
}

export function fetchCandles(
  coin: string,
  interval: string,
  startTime?: number,
  endTime?: number
): Promise<CandleData[]> {
  return postInfo({
    type: "candleSnapshot",
    coin,
    interval,
    startTime,
    endTime,
  });
}

// ============================================================================
// Exchange Actions
// ============================================================================

export function submitExchange(request: ExchangeRequest): Promise<unknown> {
  return postExchange(request);
}
