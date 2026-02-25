"use client";

import { useState } from "react";
import Link from "next/link";
import { ConnectButton } from "@rainbow-me/rainbowkit";
import { useAccount } from "wagmi";
import { useSpotMeta } from "@/hooks/useMarketData";
import { ADMIN_ADDRESS } from "@/lib/constants";

export default function AdminPage() {
  const { address, isConnected } = useAccount();
  const { data: meta } = useSpotMeta();

  const isAdmin =
    address?.toLowerCase() === ADMIN_ADDRESS.toLowerCase();

  const [name, setName] = useState("");
  const [symbol, setSymbol] = useState("");
  const [weiDecimals, setWeiDecimals] = useState("18");
  const [szDecimals, setSzDecimals] = useState("4");
  const [maxSupply, setMaxSupply] = useState("1000000000");
  const [status, setStatus] = useState<string | null>(null);

  const handleDeploy = async () => {
    setStatus("Deploy token functionality requires backend adminDeployToken endpoint. Coming soon.");
  };

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header */}
      <header className="flex items-center justify-between px-6 py-3 border-b border-border-primary bg-bg-secondary">
        <div className="flex items-center gap-4">
          <Link
            href="/"
            className="text-accent font-bold text-lg tracking-tight hover:opacity-80 transition-opacity"
          >
            HyperCore
          </Link>
          <span className="text-text-secondary text-sm">Admin</span>
        </div>
        <ConnectButton
          showBalance={false}
          chainStatus="icon"
          accountStatus="address"
        />
      </header>

      <main className="flex-1 p-6 max-w-2xl mx-auto w-full">
        {!isConnected ? (
          <div className="text-text-tertiary text-center py-12">
            Connect wallet to access admin panel
          </div>
        ) : !isAdmin ? (
          <div className="text-text-tertiary text-center py-12">
            Only the admin wallet can access this page.
            <br />
            <span className="text-xs mt-2 block text-text-tertiary/60">
              Admin: {ADMIN_ADDRESS}
            </span>
          </div>
        ) : (
          <>
            {/* Deploy Token Form */}
            <div className="mb-8">
              <h2 className="text-text-secondary text-sm font-medium mb-4 uppercase tracking-wider">
                Deploy New Token
              </h2>
              <div className="border border-border-primary rounded-lg p-4 bg-bg-secondary space-y-3">
                <div>
                  <label className="text-text-tertiary text-xs block mb-1">
                    Token Name
                  </label>
                  <input
                    type="text"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="e.g. Solana"
                    className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm focus:outline-none focus:border-accent"
                  />
                </div>
                <div>
                  <label className="text-text-tertiary text-xs block mb-1">
                    Symbol
                  </label>
                  <input
                    type="text"
                    value={symbol}
                    onChange={(e) => setSymbol(e.target.value)}
                    placeholder="e.g. SOL"
                    className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm focus:outline-none focus:border-accent"
                  />
                </div>
                <div className="flex gap-3">
                  <div className="flex-1">
                    <label className="text-text-tertiary text-xs block mb-1">
                      Wei Decimals
                    </label>
                    <input
                      type="number"
                      value={weiDecimals}
                      onChange={(e) => setWeiDecimals(e.target.value)}
                      className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm focus:outline-none focus:border-accent"
                    />
                  </div>
                  <div className="flex-1">
                    <label className="text-text-tertiary text-xs block mb-1">
                      Size Decimals
                    </label>
                    <input
                      type="number"
                      value={szDecimals}
                      onChange={(e) => setSzDecimals(e.target.value)}
                      className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm focus:outline-none focus:border-accent"
                    />
                  </div>
                </div>
                <div>
                  <label className="text-text-tertiary text-xs block mb-1">
                    Max Supply
                  </label>
                  <input
                    type="text"
                    value={maxSupply}
                    onChange={(e) => setMaxSupply(e.target.value)}
                    className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm focus:outline-none focus:border-accent"
                  />
                </div>
                <button
                  onClick={handleDeploy}
                  disabled={!name || !symbol}
                  className="w-full py-2.5 rounded font-bold text-sm bg-accent hover:brightness-110 disabled:opacity-50 transition-all"
                >
                  Deploy Token
                </button>
                {status && (
                  <div className="text-text-secondary text-xs p-2 bg-bg-tertiary rounded">
                    {status}
                  </div>
                )}
              </div>
            </div>

            {/* Existing Markets */}
            <div>
              <h2 className="text-text-secondary text-sm font-medium mb-4 uppercase tracking-wider">
                Existing Markets
              </h2>
              <div className="border border-border-primary rounded-lg bg-bg-secondary overflow-hidden">
                {!meta?.universe || meta.universe.length === 0 ? (
                  <div className="text-text-tertiary text-xs p-4 text-center">
                    No markets deployed yet
                  </div>
                ) : (
                  <table className="w-full text-xs">
                    <thead>
                      <tr className="text-text-tertiary border-b border-border-secondary">
                        <th className="text-left px-3 py-2">Market</th>
                        <th className="text-right px-3 py-2">ID</th>
                        <th className="text-right px-3 py-2">Size Decimals</th>
                      </tr>
                    </thead>
                    <tbody>
                      {meta.universe.map((m) => (
                        <tr
                          key={m.id}
                          className="border-b border-border-secondary"
                        >
                          <td className="px-3 py-2 font-medium">{m.name}</td>
                          <td className="px-3 py-2 text-right">{m.id}</td>
                          <td className="px-3 py-2 text-right">
                            {m.lotSize}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            </div>

            {/* Tokens */}
            {meta?.tokens && meta.tokens.length > 0 && (
              <div className="mt-8">
                <h2 className="text-text-secondary text-sm font-medium mb-4 uppercase tracking-wider">
                  Registered Tokens
                </h2>
                <div className="border border-border-primary rounded-lg bg-bg-secondary overflow-hidden">
                  <table className="w-full text-xs">
                    <thead>
                      <tr className="text-text-tertiary border-b border-border-secondary">
                        <th className="text-left px-3 py-2">Symbol</th>
                        <th className="text-left px-3 py-2">Name</th>
                        <th className="text-right px-3 py-2">Index</th>
                        <th className="text-right px-3 py-2">Wei Decimals</th>
                        <th className="text-right px-3 py-2">Max Supply</th>
                      </tr>
                    </thead>
                    <tbody>
                      {meta.tokens.map((t) => (
                        <tr
                          key={t.index}
                          className="border-b border-border-secondary"
                        >
                          <td className="px-3 py-2 font-medium">{t.symbol}</td>
                          <td className="px-3 py-2">{t.name}</td>
                          <td className="px-3 py-2 text-right">{t.index}</td>
                          <td className="px-3 py-2 text-right">
                            {t.weiDecimals}
                          </td>
                          <td className="px-3 py-2 text-right">
                            {t.maxSupply}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </>
        )}
      </main>
    </div>
  );
}
