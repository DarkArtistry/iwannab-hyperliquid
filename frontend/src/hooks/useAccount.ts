"use client";

import { useQuery } from "@tanstack/react-query";
import { useAccount as useWagmiAccount } from "wagmi";
import { fetchSpotBalances } from "@/lib/api";
import type { SpotBalance } from "@/lib/types";

export function useSpotBalances() {
  const { address } = useWagmiAccount();

  return useQuery<SpotBalance[]>({
    queryKey: ["spotBalances", address],
    queryFn: () => fetchSpotBalances(address!),
    enabled: !!address,
    refetchInterval: 3000,
  });
}
