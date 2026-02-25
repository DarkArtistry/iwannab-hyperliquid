import type { SignatureWire, SpotOrderWire, SpotCancelWire } from "./types";
import { EIP712_DOMAIN } from "./constants";

// ============================================================================
// EIP-712 Type Definitions for Spot (must match backend exactly)
// ============================================================================

// Type string: "Action(string type,SpotOrder[] orders,string grouping,uint64 nonce)SpotOrder(uint8 a,bool b,string p,string s,string t)"
const spotOrderTypes = {
  Action: [
    { name: "type", type: "string" },
    { name: "orders", type: "SpotOrder[]" },
    { name: "grouping", type: "string" },
    { name: "nonce", type: "uint64" },
  ],
  SpotOrder: [
    { name: "a", type: "uint8" },
    { name: "b", type: "bool" },
    { name: "p", type: "string" },
    { name: "s", type: "string" },
    { name: "t", type: "string" },
  ],
} as const;

// Type string: "Action(string type,SpotCancel[] cancels,uint64 nonce)SpotCancel(uint8 a,uint64 o)"
const spotCancelTypes = {
  Action: [
    { name: "type", type: "string" },
    { name: "cancels", type: "SpotCancel[]" },
    { name: "nonce", type: "uint64" },
  ],
  SpotCancel: [
    { name: "a", type: "uint8" },
    { name: "o", type: "uint64" },
  ],
} as const;

const spotCancelAllTypes = {
  Action: [
    { name: "type", type: "string" },
    { name: "nonce", type: "uint64" },
  ],
} as const;

// ============================================================================
// Helpers
// ============================================================================

/** Convert OrderTypeWire to TIF string for EIP-712 signing (must match backend) */
function orderTypeToTif(t: SpotOrderWire["t"]): string {
  if ("limit" in t) return t.limit.tif.toLowerCase();
  return "trigger";
}

/** Parse a 65-byte hex signature into { r, s, v } */
export function parseSignature(sig: `0x${string}`): SignatureWire {
  const raw = sig.slice(2); // strip 0x
  return {
    r: "0x" + raw.slice(0, 64),
    s: "0x" + raw.slice(64, 128),
    v: parseInt(raw.slice(128, 130), 16),
  };
}

// ============================================================================
// Build EIP-712 Typed Data for Spot actions
// ============================================================================

export function buildSpotOrderTypedData(
  orders: SpotOrderWire[],
  grouping: string,
  nonce: number
) {
  return {
    domain: EIP712_DOMAIN,
    types: spotOrderTypes,
    primaryType: "Action" as const,
    message: {
      type: "spotOrder",
      orders: orders.map((o) => ({
        a: o.a,
        b: o.b,
        p: o.p,
        s: o.s,
        t: orderTypeToTif(o.t),
      })),
      grouping,
      nonce: BigInt(nonce),
    },
  };
}

export function buildSpotCancelTypedData(
  cancels: SpotCancelWire[],
  nonce: number
) {
  return {
    domain: EIP712_DOMAIN,
    types: spotCancelTypes,
    primaryType: "Action" as const,
    message: {
      type: "spotCancel",
      cancels: cancels.map((c) => ({ a: c.a, o: BigInt(c.o) })),
      nonce: BigInt(nonce),
    },
  };
}

export function buildSpotCancelAllTypedData(nonce: number) {
  return {
    domain: EIP712_DOMAIN,
    types: spotCancelAllTypes,
    primaryType: "Action" as const,
    message: {
      type: "spotCancelAll",
      nonce: BigInt(nonce),
    },
  };
}
