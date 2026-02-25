"use client";

import { useEffect, useRef, useState } from "react";
import { fetchCandles } from "@/lib/api";
import { useAllMids } from "@/hooks/useMarketData";

interface PriceChartProps {
  coin: string;
}

export function PriceChart({ coin }: PriceChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<unknown>(null);
  const seriesRef = useRef<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const { data: mids } = useAllMids();

  useEffect(() => {
    let cancelled = false;

    async function init() {
      if (!containerRef.current) return;

      try {
        const { createChart, ColorType, CrosshairMode } = await import(
          "lightweight-charts"
        );

        if (cancelled || !containerRef.current) return;

        // Clean up old chart
        if (chartRef.current) {
          (chartRef.current as { remove: () => void }).remove();
        }

        const chart = createChart(containerRef.current, {
          width: containerRef.current.clientWidth,
          height: containerRef.current.clientHeight,
          layout: {
            background: { type: ColorType.Solid, color: "#0d1117" },
            textColor: "#8b949e",
            fontSize: 11,
          },
          grid: {
            vertLines: { color: "#1c2128" },
            horzLines: { color: "#1c2128" },
          },
          crosshair: { mode: CrosshairMode.Normal },
          rightPriceScale: {
            borderColor: "#30363d",
          },
          timeScale: {
            borderColor: "#30363d",
            timeVisible: true,
            secondsVisible: false,
          },
        });

        chartRef.current = chart;

        const candleSeries = chart.addCandlestickSeries({
          upColor: "#3fb950",
          downColor: "#f85149",
          borderDownColor: "#f85149",
          borderUpColor: "#3fb950",
          wickDownColor: "#f85149",
          wickUpColor: "#3fb950",
        });

        seriesRef.current = candleSeries;

        // Fetch candle data
        try {
          const candles = await fetchCandles(coin, "1m");
          if (!cancelled && candles && candles.length > 0) {
            const data = candles.map((c) => ({
              time: (c.T / 1000) as number,
              open: parseFloat(c.o),
              high: parseFloat(c.h),
              low: parseFloat(c.l),
              close: parseFloat(c.c),
            }));
            candleSeries.setData(data as never[]);
            chart.timeScale().fitContent();
          }
        } catch {
          // No candle data yet — that's okay for a fresh devnet
        }

        // Handle resize
        const observer = new ResizeObserver((entries) => {
          for (const entry of entries) {
            chart.applyOptions({
              width: entry.contentRect.width,
              height: entry.contentRect.height,
            });
          }
        });
        observer.observe(containerRef.current);

        return () => {
          observer.disconnect();
          chart.remove();
        };
      } catch (e) {
        if (!cancelled) {
          setError(
            e instanceof Error ? e.message : "Failed to load chart"
          );
        }
      }
    }

    init();

    return () => {
      cancelled = true;
    };
  }, [coin]);

  // Update with latest price from mids
  useEffect(() => {
    if (!seriesRef.current || !mids?.[coin]) return;
    const price = parseFloat(mids[coin]);
    if (isNaN(price)) return;

    const now = Math.floor(Date.now() / 1000);
    try {
      (seriesRef.current as { update: (d: unknown) => void }).update({
        time: now,
        open: price,
        high: price,
        low: price,
        close: price,
      });
    } catch {
      // ignore if series doesn't support update
    }
  }, [mids, coin]);

  if (error) {
    return (
      <div className="flex items-center justify-center h-full text-text-tertiary text-sm">
        Chart: {error}
      </div>
    );
  }

  return <div ref={containerRef} className="w-full h-full" />;
}
