"use client";

import { useSpotBalances } from "@/hooks/useAccount";
import { useAccount } from "wagmi";

export function SpotBalances() {
  const { isConnected } = useAccount();
  const { data: balances, isLoading } = useSpotBalances();

  if (!isConnected) {
    return (
      <div className="text-text-tertiary text-xs p-4 text-center">
        Connect wallet to view balances
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="text-text-tertiary text-xs p-4">Loading balances...</div>
    );
  }

  if (!balances || balances.length === 0) {
    return (
      <div className="text-text-tertiary text-xs p-4 text-center">
        No balances
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead>
          <tr className="text-text-tertiary border-b border-border-secondary">
            <th className="text-left px-3 py-2">Token</th>
            <th className="text-right px-3 py-2">Total</th>
            <th className="text-right px-3 py-2">Reserved</th>
            <th className="text-right px-3 py-2">Available</th>
          </tr>
        </thead>
        <tbody>
          {balances.map((b) => (
            <tr
              key={b.symbol}
              className="border-b border-border-secondary hover:bg-bg-hover"
            >
              <td className="px-3 py-2 font-medium">{b.symbol}</td>
              <td className="px-3 py-2 text-right tabular-nums">
                {parseFloat(b.total).toLocaleString(undefined, {
                  minimumFractionDigits: 2,
                  maximumFractionDigits: 8,
                })}
              </td>
              <td className="px-3 py-2 text-right tabular-nums text-text-secondary">
                {parseFloat(b.reserved).toLocaleString(undefined, {
                  minimumFractionDigits: 2,
                  maximumFractionDigits: 8,
                })}
              </td>
              <td className="px-3 py-2 text-right tabular-nums">
                {parseFloat(b.available).toLocaleString(undefined, {
                  minimumFractionDigits: 2,
                  maximumFractionDigits: 8,
                })}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
