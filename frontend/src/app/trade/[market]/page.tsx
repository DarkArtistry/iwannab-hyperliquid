"use client";

import { useState } from "react";
import { useParams } from "next/navigation";
import { Header } from "@/components/Header";
import { OrderBook } from "@/components/OrderBook";
import { TradeForm } from "@/components/TradeForm";
import { OpenOrders } from "@/components/OpenOrders";
import { SpotBalances } from "@/components/SpotBalances";
import { AccountSummary } from "@/components/AccountSummary";
import { PriceChart } from "@/components/PriceChart";
import { RecentTrades } from "@/components/RecentTrades";
import { useAllMids, useSpotMeta } from "@/hooks/useMarketData";

type BottomTab = "balances" | "orders" | "trades";

export default function TradePage() {
  const params = useParams();
  const marketName = decodeURIComponent(params.market as string);
  const [prefillPrice, setPrefillPrice] = useState("");
  const [bottomTab, setBottomTab] = useState<BottomTab>("balances");
  const { data: mids } = useAllMids();
  const { data: meta } = useSpotMeta();

  const market = meta?.universe.find((m) => m.name === marketName);
  const midPrice = mids?.[marketName];

  return (
    <div className="flex flex-col h-screen">
      {/* Header */}
      <Header marketName={marketName} midPrice={midPrice} />

      {/* Main Trading Area */}
      <div className="flex-1 flex min-h-0">
        {/* Left: Price Chart */}
        <div className="flex-1 border-r border-border-primary min-w-0">
          <PriceChart coin={marketName} />
        </div>

        {/* Center: Order Book */}
        <div className="w-64 border-r border-border-primary flex flex-col">
          <div className="px-2 py-1.5 text-xs font-medium text-text-secondary border-b border-border-secondary bg-bg-secondary">
            ORDER BOOK
          </div>
          <div className="flex-1 min-h-0">
            <OrderBook coin={marketName} onPriceClick={setPrefillPrice} />
          </div>
        </div>

        {/* Right: Trade Form */}
        <div className="w-72 flex flex-col">
          <div className="px-3 py-1.5 text-xs font-medium text-text-secondary border-b border-border-secondary bg-bg-secondary">
            TRADE {marketName}
          </div>
          <div className="flex-1 overflow-y-auto">
            <TradeForm market={market ?? null} prefillPrice={prefillPrice} />
          </div>
        </div>
      </div>

      {/* Account Summary Bar */}
      <AccountSummary />

      {/* Bottom Panel */}
      <div className="h-64 flex flex-col border-t border-border-primary">
        {/* Tabs */}
        <div className="flex gap-0 border-b border-border-secondary bg-bg-secondary">
          {(
            [
              ["balances", "Balances"],
              ["orders", "Open Orders"],
              ["trades", "Recent Trades"],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              onClick={() => setBottomTab(key)}
              className={`px-4 py-2 text-xs transition-colors border-b-2 ${
                bottomTab === key
                  ? "text-accent border-accent"
                  : "text-text-tertiary border-transparent hover:text-text-secondary"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {/* Tab Content */}
        <div className="flex-1 overflow-y-auto">
          {bottomTab === "balances" && <SpotBalances />}
          {bottomTab === "orders" && <OpenOrders />}
          {bottomTab === "trades" && <RecentTrades coin={marketName} />}
        </div>
      </div>
    </div>
  );
}
