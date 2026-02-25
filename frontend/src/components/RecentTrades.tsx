"use client";

import { useRecentTrades } from "@/hooks/useMarketData";

interface RecentTradesProps {
  coin: string;
}

export function RecentTrades({ coin }: RecentTradesProps) {
  const { data: trades, isLoading } = useRecentTrades(coin);

  if (isLoading) {
    return (
      <div className="text-text-tertiary text-xs p-4">
        Loading trades...
      </div>
    );
  }

  if (!trades || trades.length === 0) {
    return (
      <div className="text-text-tertiary text-xs p-4 text-center">
        No recent trades
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead>
          <tr className="text-text-tertiary border-b border-border-secondary">
            <th className="text-left px-3 py-2">Price</th>
            <th className="text-right px-3 py-2">Size</th>
            <th className="text-right px-3 py-2">Time</th>
          </tr>
        </thead>
        <tbody>
          {trades.slice(0, 30).map((t, i) => {
            const isBuy =
              t.side.toLowerCase() === "buy" || t.side === "B";
            return (
              <tr
                key={`${t.hash}-${i}`}
                className="border-b border-border-secondary"
              >
                <td
                  className={`px-3 py-0.5 tabular-nums ${isBuy ? "text-green" : "text-red"}`}
                >
                  ${parseFloat(t.px).toLocaleString()}
                </td>
                <td className="px-3 py-0.5 text-right tabular-nums text-text-secondary">
                  {t.sz}
                </td>
                <td className="px-3 py-0.5 text-right text-text-tertiary">
                  {new Date(t.time).toLocaleTimeString()}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
