"use client";

import Link from "next/link";
import { ConnectButton } from "@rainbow-me/rainbowkit";
import { useSpotMeta, useAllMids } from "@/hooks/useMarketData";
import { ADMIN_ADDRESS } from "@/lib/constants";
import { useAccount } from "wagmi";

export default function HomePage() {
  const { data: meta, isLoading } = useSpotMeta();
  const { data: mids } = useAllMids();
  const { address } = useAccount();

  const isAdmin =
    address?.toLowerCase() === ADMIN_ADDRESS.toLowerCase();

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header */}
      <header className="flex items-center justify-between px-6 py-3 border-b border-border-primary bg-bg-secondary">
        <div className="flex items-center gap-4">
          <span className="text-accent font-bold text-lg tracking-tight">
            HyperCore
          </span>
          <span className="text-text-tertiary text-sm">Spot Exchange</span>
        </div>
        <div className="flex items-center gap-3">
          {isAdmin && (
            <Link
              href="/admin"
              className="text-xs px-3 py-1.5 rounded bg-accent/20 text-accent hover:bg-accent/30 transition-colors"
            >
              Admin
            </Link>
          )}
          <ConnectButton
            showBalance={false}
            chainStatus="icon"
            accountStatus="address"
          />
        </div>
      </header>

      {/* Content */}
      <main className="flex-1 p-6">
        <h2 className="text-text-secondary text-sm font-medium mb-4 uppercase tracking-wider">
          Markets
        </h2>

        {isLoading ? (
          <div className="text-text-tertiary text-sm">Loading markets...</div>
        ) : !meta?.universe || meta.universe.length === 0 ? (
          <div className="text-text-tertiary text-sm">
            No markets available. Deploy tokens to create markets.
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {meta.universe.map((market) => {
              const midPrice = mids?.[market.name];
              const baseToken = meta.tokens.find(
                (t) => t.index === market.baseToken
              );

              return (
                <Link
                  key={market.id}
                  href={`/trade/${encodeURIComponent(market.name)}`}
                  className="block p-4 rounded-lg border border-border-primary bg-bg-secondary hover:bg-bg-hover hover:border-accent/40 transition-all"
                >
                  <div className="flex items-center justify-between mb-2">
                    <span className="font-bold text-text-primary">
                      {market.name}
                    </span>
                    <span className="text-xs text-text-tertiary px-2 py-0.5 rounded bg-bg-tertiary">
                      SPOT
                    </span>
                  </div>
                  <div className="text-xl font-bold tabular-nums mb-1">
                    {midPrice
                      ? `$${parseFloat(midPrice).toLocaleString(undefined, { minimumFractionDigits: 2 })}`
                      : "--"}
                  </div>
                  <div className="text-xs text-text-tertiary">
                    {baseToken?.symbol ?? "?"} / USDC
                  </div>
                </Link>
              );
            })}
          </div>
        )}
      </main>
    </div>
  );
}
