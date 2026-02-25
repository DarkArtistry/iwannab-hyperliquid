"use client";

import { useState } from "react";
import { useAccount } from "wagmi";
import { usePlaceSpotOrder } from "@/hooks/useOrders";
import { useSpotBalances } from "@/hooks/useAccount";
import type { SpotOrderWire, SpotMarketMeta } from "@/lib/types";

interface TradeFormProps {
  market: SpotMarketMeta | null;
  prefillPrice?: string;
}

export function TradeForm({ market, prefillPrice }: TradeFormProps) {
  const { isConnected } = useAccount();
  const placeOrder = usePlaceSpotOrder();
  const { data: balancesData } = useSpotBalances();

  const [orderType, setOrderType] = useState<"limit" | "market">("limit");
  const [price, setPrice] = useState(prefillPrice ?? "");
  const [size, setSize] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Update price when prefillPrice changes from orderbook click
  if (prefillPrice && prefillPrice !== price && !isSubmitting) {
    setPrice(prefillPrice);
  }

  // Extract base symbol from market name (e.g. "BTC-USDC" -> "BTC")
  const baseSymbol = market?.name?.split("-")[0] ?? "?";

  // Find USDC balance for buy, base token balance for sell
  const usdcBalance = balancesData?.find(
    (b) => b.symbol === "USDC"
  );
  const baseBalance = balancesData?.find(
    (b) => b.symbol === baseSymbol
  );

  const handleSubmit = async (side: "buy" | "sell") => {
    if (!isConnected) {
      setError("Connect wallet first");
      return;
    }
    if (!market) {
      setError("Market not loaded");
      return;
    }
    if (!size || parseFloat(size) <= 0) {
      setError("Enter a valid size");
      return;
    }
    if (orderType === "limit" && (!price || parseFloat(price) <= 0)) {
      setError("Enter a valid price");
      return;
    }

    setIsSubmitting(true);
    setError(null);
    setSuccess(null);

    try {
      const orderPrice =
        orderType === "market"
          ? side === "buy"
            ? "999999999"
            : "0.01"
          : price;

      const order: SpotOrderWire = {
        a: market.id,
        b: side === "buy",
        p: orderPrice,
        s: size,
        t: { limit: { tif: orderType === "market" ? "Ioc" : "Gtc" } },
      };

      const result = await placeOrder([order], "na");
      setSuccess(
        `${side.toUpperCase()} ${size} ${baseSymbol} @ ${orderType === "market" ? "MARKET" : "$" + price}`
      );
      console.log("Order response:", JSON.stringify(result, null, 2));
      setSize("");
      if (orderType === "limit") setPrice("");
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Order failed";
      setError(msg);
      console.error("Order error:", msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  const fillSize = (pct: number) => {
    if (!market) return;

    // For buy: use USDC balance / price to compute max base amount
    // For sell: use base token balance
    // We default to buy-side calculation here since we don't know side
    const available = parseFloat(usdcBalance?.available ?? "0");
    if (available <= 0) return;

    const refPrice =
      price && parseFloat(price) > 0 ? parseFloat(price) : 0;
    if (refPrice <= 0) return;

    const maxSz = (available * pct) / refPrice;
    // Derive size decimals from lotSize (e.g. "0.001" = 3 decimals)
    const lotParts = market.lotSize?.split(".");
    const szDecimals = lotParts?.[1]?.length ?? 4;
    setSize(maxSz.toFixed(szDecimals));
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      {/* Order Type */}
      <div className="flex gap-1">
        {(["limit", "market"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setOrderType(t)}
            className={`flex-1 py-1 text-xs rounded ${
              orderType === t
                ? "bg-bg-hover text-text-primary"
                : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {t.charAt(0).toUpperCase() + t.slice(1)}
          </button>
        ))}
      </div>

      {/* Price */}
      {orderType === "limit" && (
        <div>
          <label className="text-text-tertiary text-xs block mb-1">
            Price (USDC)
          </label>
          <input
            type="number"
            value={price}
            onChange={(e) => setPrice(e.target.value)}
            placeholder="0.00"
            className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm tabular-nums focus:outline-none focus:border-accent"
          />
        </div>
      )}

      {/* Size */}
      <div>
        <label className="text-text-tertiary text-xs block mb-1">
          Size ({baseSymbol})
        </label>
        <input
          type="number"
          value={size}
          onChange={(e) => setSize(e.target.value)}
          placeholder="0.00"
          className="w-full bg-bg-tertiary border border-border-primary rounded px-3 py-2 text-sm tabular-nums focus:outline-none focus:border-accent"
        />
        <div className="flex gap-1 mt-1">
          {[0.25, 0.5, 0.75, 1.0].map((pct) => (
            <button
              key={pct}
              onClick={() => fillSize(pct)}
              className="flex-1 text-xs py-0.5 text-text-tertiary hover:text-text-secondary bg-bg-tertiary rounded"
            >
              {pct * 100}%
            </button>
          ))}
        </div>
      </div>

      {/* Available Balance */}
      <div className="text-xs text-text-tertiary flex justify-between">
        <span>USDC Available:</span>
        <span className="tabular-nums">
          {parseFloat(usdcBalance?.available ?? "0").toLocaleString(undefined, {
            minimumFractionDigits: 2,
          })}
        </span>
      </div>
      <div className="text-xs text-text-tertiary flex justify-between">
        <span>{baseSymbol} Available:</span>
        <span className="tabular-nums">
          {parseFloat(baseBalance?.available ?? "0").toLocaleString(undefined, {
            minimumFractionDigits: 4,
          })}
        </span>
      </div>

      {/* Buy/Sell Buttons */}
      <div className="flex gap-2">
        <button
          onClick={() => handleSubmit("buy")}
          disabled={!isConnected || isSubmitting}
          className="flex-1 py-2.5 rounded font-bold text-sm bg-green dark:bg-green-dark hover:brightness-110 disabled:opacity-50 transition-all"
        >
          {isSubmitting ? "Signing..." : "BUY"}
        </button>
        <button
          onClick={() => handleSubmit("sell")}
          disabled={!isConnected || isSubmitting}
          className="flex-1 py-2.5 rounded font-bold text-sm bg-red dark:bg-red-dark hover:brightness-110 disabled:opacity-50 transition-all"
        >
          {isSubmitting ? "Signing..." : "SELL"}
        </button>
      </div>

      {/* Feedback */}
      {error && (
        <div className="text-red text-xs p-2 bg-red/10 rounded break-all">
          {error}
        </div>
      )}
      {success && (
        <div className="text-green text-xs p-2 bg-green/10 rounded">
          {success}
        </div>
      )}

      {!isConnected && (
        <div className="text-text-tertiary text-xs text-center">
          Connect wallet to trade
        </div>
      )}
    </div>
  );
}
