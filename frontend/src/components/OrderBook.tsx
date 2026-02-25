"use client";

import { useOrderBook } from "@/hooks/useMarketData";
import { useMemo } from "react";

interface OrderBookProps {
  coin: string;
  onPriceClick: (price: string) => void;
}

export function OrderBook({ coin, onPriceClick }: OrderBookProps) {
  const { data, isLoading } = useOrderBook(coin);

  const { asks, bids, spread, maxSize } = useMemo(() => {
    if (!data?.levels)
      return { asks: [], bids: [], spread: "0.00", maxSize: 1 };

    const [rawBids, rawAsks] = data.levels;
    const bids = rawBids.slice(0, 15);
    const asks = rawAsks.slice(0, 15).reverse(); // Show asks top-down (highest first)

    const allSizes = [...bids, ...asks].map((l) => parseFloat(l.sz));
    const maxSize = Math.max(...allSizes, 0.001);

    const bestBid = bids[0] ? parseFloat(bids[0].px) : 0;
    const bestAsk = rawAsks[0] ? parseFloat(rawAsks[0].px) : 0;
    const spread =
      bestBid && bestAsk ? (bestAsk - bestBid).toFixed(2) : "0.00";

    return { asks, bids, spread, maxSize };
  }, [data]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-text-tertiary text-sm">
        Loading orderbook...
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full text-xs font-mono">
      <div className="flex justify-between px-2 py-1 text-text-tertiary border-b border-border-secondary">
        <span>Price</span>
        <span>Size</span>
        <span>Total</span>
      </div>

      {/* Asks (sells) — shown in reverse so lowest ask is at bottom */}
      <div className="flex-1 overflow-hidden flex flex-col justify-end">
        {asks.map((level, i) => {
          const sizePct = (parseFloat(level.sz) / maxSize) * 100;
          return (
            <div
              key={`ask-${i}`}
              className="relative flex justify-between px-2 py-0.5 cursor-pointer hover:bg-bg-hover"
              onClick={() => onPriceClick(level.px)}
            >
              <div
                className="absolute right-0 top-0 bottom-0 bg-red/10"
                style={{ width: `${sizePct}%` }}
              />
              <span className="text-red relative z-10">{level.px}</span>
              <span className="text-text-secondary relative z-10">
                {level.sz}
              </span>
              <span className="text-text-tertiary relative z-10">
                {level.n}
              </span>
            </div>
          );
        })}
      </div>

      {/* Spread */}
      <div className="flex justify-center px-2 py-1 text-text-tertiary border-y border-border-secondary bg-bg-tertiary">
        Spread: ${spread}
      </div>

      {/* Bids (buys) */}
      <div className="flex-1 overflow-hidden">
        {bids.map((level, i) => {
          const sizePct = (parseFloat(level.sz) / maxSize) * 100;
          return (
            <div
              key={`bid-${i}`}
              className="relative flex justify-between px-2 py-0.5 cursor-pointer hover:bg-bg-hover"
              onClick={() => onPriceClick(level.px)}
            >
              <div
                className="absolute right-0 top-0 bottom-0 bg-green/10"
                style={{ width: `${sizePct}%` }}
              />
              <span className="text-green relative z-10">{level.px}</span>
              <span className="text-text-secondary relative z-10">
                {level.sz}
              </span>
              <span className="text-text-tertiary relative z-10">
                {level.n}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
