"use client";

import { useEffect, useRef, useCallback } from "react";
import { getWebSocket, HyperCoreWebSocket } from "@/lib/websocket";

export function useWebSocket() {
  const wsRef = useRef<HyperCoreWebSocket | null>(null);

  useEffect(() => {
    wsRef.current = getWebSocket();
    wsRef.current.connect();
    return () => {
      // Don't disconnect on unmount — singleton stays alive
    };
  }, []);

  const subscribe = useCallback(
    (subscription: { type: string; [key: string]: unknown }) => {
      wsRef.current?.subscribe(subscription);
    },
    []
  );

  const unsubscribe = useCallback(
    (subscription: { type: string; [key: string]: unknown }) => {
      wsRef.current?.unsubscribe(subscription);
    },
    []
  );

  const on = useCallback(
    (channel: string, handler: (data: unknown) => void) => {
      return wsRef.current?.on(channel, handler) ?? (() => {});
    },
    []
  );

  return { subscribe, unsubscribe, on };
}
