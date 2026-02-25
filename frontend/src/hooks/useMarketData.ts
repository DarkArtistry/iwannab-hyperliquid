"use client";

import { useQuery } from "@tanstack/react-query";
import {
  fetchSpotL2Book,
  fetchSpotAllMids,
  fetchSpotMeta,
  fetchRecentTrades,
} from "@/lib/api";
import type { L2BookResponse, TradeData, SpotMetaResponse } from "@/lib/types";

export function useSpotMeta() {
  return useQuery<SpotMetaResponse>({
    queryKey: ["spotMeta"],
    queryFn: fetchSpotMeta,
    refetchInterval: 30000,
  });
}

export function useOrderBook(coin: string) {
  return useQuery<L2BookResponse>({
    queryKey: ["spotL2Book", coin],
    queryFn: () => fetchSpotL2Book(coin),
    refetchInterval: 2000,
  });
}

export function useAllMids() {
  return useQuery<Record<string, string>>({
    queryKey: ["spotAllMids"],
    queryFn: fetchSpotAllMids,
    refetchInterval: 1000,
  });
}

export function useRecentTrades(coin: string) {
  return useQuery<TradeData[]>({
    queryKey: ["recentTrades", coin],
    queryFn: () => fetchRecentTrades(coin, 50),
    refetchInterval: 3000,
  });
}
