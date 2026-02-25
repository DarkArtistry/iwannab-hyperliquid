"use client";

import {
  useSpotOpenOrders,
  useCancelSpotOrder,
  useCancelAllSpotOrders,
} from "@/hooks/useOrders";
import { useState } from "react";

export function OpenOrders() {
  const { data: orders, isLoading } = useSpotOpenOrders();
  const cancelOrder = useCancelSpotOrder();
  const cancelAll = useCancelAllSpotOrders();
  const [cancellingId, setCancellingId] = useState<number | null>(null);
  const [cancellingAll, setCancellingAll] = useState(false);

  const handleCancel = async (oid: number, marketId: number) => {
    setCancellingId(oid);
    try {
      await cancelOrder([{ a: marketId, o: oid }]);
    } catch {
      // error
    } finally {
      setCancellingId(null);
    }
  };

  const handleCancelAll = async () => {
    setCancellingAll(true);
    try {
      await cancelAll();
    } catch {
      // error
    } finally {
      setCancellingAll(false);
    }
  };

  if (isLoading) {
    return (
      <div className="text-text-tertiary text-xs p-4">Loading orders...</div>
    );
  }

  if (!orders || orders.length === 0) {
    return (
      <div className="text-text-tertiary text-xs p-4 text-center">
        No open orders
      </div>
    );
  }

  return (
    <div>
      <div className="flex justify-end px-3 py-1">
        <button
          onClick={handleCancelAll}
          disabled={cancellingAll}
          className="text-xs text-red hover:text-red/80 disabled:opacity-50"
        >
          {cancellingAll ? "Cancelling..." : "Cancel All"}
        </button>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-text-tertiary border-b border-border-secondary">
              <th className="text-left px-3 py-2">Market</th>
              <th className="text-left px-3 py-2">Side</th>
              <th className="text-right px-3 py-2">Price</th>
              <th className="text-right px-3 py-2">Size</th>
              <th className="text-right px-3 py-2">Filled</th>
              <th className="text-right px-3 py-2">Time</th>
              <th className="text-center px-3 py-2">Cancel</th>
            </tr>
          </thead>
          <tbody>
            {orders.map((o) => {
              const isBuy = o.side.toLowerCase() === "buy" || o.side === "B";
              const filled = parseFloat(o.origSz) - parseFloat(o.sz);
              const marketId = o.marketId ?? 0;

              return (
                <tr
                  key={o.oid}
                  className="border-b border-border-secondary hover:bg-bg-hover"
                >
                  <td className="px-3 py-2">{o.coin}</td>
                  <td
                    className={`px-3 py-2 ${isBuy ? "text-green" : "text-red"}`}
                  >
                    {isBuy ? "BUY" : "SELL"}
                  </td>
                  <td className="px-3 py-2 text-right tabular-nums">
                    ${parseFloat(o.limitPx).toLocaleString()}
                  </td>
                  <td className="px-3 py-2 text-right tabular-nums">
                    {o.sz}
                  </td>
                  <td className="px-3 py-2 text-right tabular-nums text-text-secondary">
                    {filled.toFixed(5)}
                  </td>
                  <td className="px-3 py-2 text-right text-text-secondary">
                    {new Date(o.timestamp).toLocaleTimeString()}
                  </td>
                  <td className="px-3 py-2 text-center">
                    <button
                      onClick={() => handleCancel(o.oid, marketId)}
                      disabled={cancellingId === o.oid}
                      className="text-red hover:text-red/80 disabled:opacity-50"
                    >
                      {cancellingId === o.oid ? "..." : "X"}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
