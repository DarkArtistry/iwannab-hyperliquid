"use client";

import { useSpotBalances } from "@/hooks/useAccount";
import { useAccount } from "wagmi";

export function AccountSummary() {
  const { isConnected } = useAccount();
  const { data: balances, isLoading } = useSpotBalances();

  if (!isConnected) {
    return (
      <div className="flex items-center justify-center px-4 py-3 text-text-tertiary text-sm">
        Connect wallet to view account
      </div>
    );
  }

  if (isLoading || !balances) {
    return (
      <div className="flex items-center justify-center px-4 py-3 text-text-tertiary text-sm">
        Loading balances...
      </div>
    );
  }

  return (
    <div className="flex items-center gap-6 px-4 py-2 text-xs border-y border-border-primary bg-bg-secondary">
      {balances.length === 0 ? (
        <span className="text-text-tertiary">No balances</span>
      ) : (
        balances.map((b) => (
          <div key={b.symbol}>
            <span className="text-text-tertiary">{b.symbol} </span>
            <span className="text-text-primary font-medium tabular-nums">
              {parseFloat(b.total).toLocaleString(undefined, {
                minimumFractionDigits: 2,
                maximumFractionDigits: 6,
              })}
            </span>
          </div>
        ))
      )}
    </div>
  );
}
