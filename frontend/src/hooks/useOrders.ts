"use client";

import { useQuery } from "@tanstack/react-query";
import { useAccount } from "wagmi";
import { useSignTypedData } from "wagmi";
import { useCallback } from "react";
import { fetchSpotOpenOrders, submitExchange } from "@/lib/api";
import {
  buildSpotOrderTypedData,
  buildSpotCancelTypedData,
  buildSpotCancelAllTypedData,
  parseSignature,
} from "@/lib/signing";
import type { SpotOrderWire, SpotCancelWire, OpenOrder } from "@/lib/types";

export function useSpotOpenOrders() {
  const { address } = useAccount();
  return useQuery<OpenOrder[]>({
    queryKey: ["spotOpenOrders", address],
    queryFn: () => fetchSpotOpenOrders(address!),
    enabled: !!address,
    refetchInterval: 3000,
  });
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function signAction(signTypedDataAsync: any, typedData: any) {
  const signature = await signTypedDataAsync({
    domain: typedData.domain,
    types: typedData.types,
    primaryType: typedData.primaryType,
    message: typedData.message,
  });
  return parseSignature(signature);
}

export function usePlaceSpotOrder() {
  const { signTypedDataAsync } = useSignTypedData();

  return useCallback(
    async (orders: SpotOrderWire[], grouping = "na") => {
      const nonce = Date.now();
      const typedData = buildSpotOrderTypedData(orders, grouping, nonce);
      const sig = await signAction(signTypedDataAsync, typedData);
      return submitExchange({
        action: { type: "spotOrder", orders, grouping },
        nonce,
        signature: sig,
      });
    },
    [signTypedDataAsync]
  );
}

export function useCancelSpotOrder() {
  const { signTypedDataAsync } = useSignTypedData();

  return useCallback(
    async (cancels: SpotCancelWire[]) => {
      const nonce = Date.now();
      const typedData = buildSpotCancelTypedData(cancels, nonce);
      const sig = await signAction(signTypedDataAsync, typedData);
      return submitExchange({
        action: { type: "spotCancel", cancels },
        nonce,
        signature: sig,
      });
    },
    [signTypedDataAsync]
  );
}

export function useCancelAllSpotOrders() {
  const { signTypedDataAsync } = useSignTypedData();

  return useCallback(async () => {
    const nonce = Date.now();
    const typedData = buildSpotCancelAllTypedData(nonce);
    const sig = await signAction(signTypedDataAsync, typedData);
    return submitExchange({
      action: { type: "spotCancelAll" },
      nonce,
      signature: sig,
    });
  }, [signTypedDataAsync]);
}
