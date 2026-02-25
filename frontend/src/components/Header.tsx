"use client";

import Link from "next/link";
import { ConnectButton } from "@rainbow-me/rainbowkit";
import { useAccount } from "wagmi";
import { ADMIN_ADDRESS } from "@/lib/constants";

interface HeaderProps {
  marketName: string;
  midPrice?: string;
}

export function Header({ marketName, midPrice }: HeaderProps) {
  const { address } = useAccount();
  const isAdmin =
    address?.toLowerCase() === ADMIN_ADDRESS.toLowerCase();

  return (
    <header className="flex items-center justify-between px-4 py-2 border-b border-border-primary bg-bg-secondary">
      <div className="flex items-center gap-4">
        <Link
          href="/"
          className="text-accent font-bold text-lg tracking-tight hover:opacity-80 transition-opacity"
        >
          HyperCore
        </Link>
        <span className="text-text-primary font-medium">{marketName}</span>
        {midPrice && (
          <span className="text-xl font-bold tabular-nums">
            $
            {parseFloat(midPrice).toLocaleString(undefined, {
              minimumFractionDigits: 2,
            })}
          </span>
        )}
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
  );
}
