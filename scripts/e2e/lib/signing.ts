/**
 * Signature Utilities
 *
 * Functions for signing exchange actions using proper EIP-712 typed data.
 *
 * This implementation matches the gateway's EIP-712 encoding exactly,
 * ensuring signatures can be cryptographically verified in production mode.
 */

import { privateKeyToAccount, signTypedData } from 'viem/accounts';
import type { SignedAction } from './types.js';

// EIP-712 Domain
const EIP712_DOMAIN = {
  name: 'HyperCore',
  version: '1',
  chainId: 1337,
  verifyingContract: '0x0000000000000000000000000000000000000000' as `0x${string}`,
};

/**
 * Get EIP-712 types for an action type
 */
function getTypesForAction(actionType: string): Record<string, Array<{ name: string; type: string }>> {
  switch (actionType) {
    case 'order':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'orders', type: 'Order[]' },
          { name: 'grouping', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
        Order: [
          { name: 'a', type: 'uint8' },
          { name: 'b', type: 'bool' },
          { name: 'p', type: 'string' },
          { name: 's', type: 'string' },
          { name: 'r', type: 'bool' },
          { name: 't', type: 'string' },
        ],
      };

    case 'cancel':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'cancels', type: 'Cancel[]' },
          { name: 'nonce', type: 'uint64' },
        ],
        Cancel: [
          { name: 'a', type: 'uint8' },
          { name: 'o', type: 'uint64' },
        ],
      };

    case 'cancelByCloid':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'cancels', type: 'CancelByCloid[]' },
          { name: 'nonce', type: 'uint64' },
        ],
        CancelByCloid: [
          { name: 'asset', type: 'uint8' },
          { name: 'cloid', type: 'string' },
        ],
      };

    case 'cancelAll':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
      };

    case 'updateLeverage':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'asset', type: 'uint8' },
          { name: 'isCross', type: 'bool' },
          { name: 'leverage', type: 'uint8' },
          { name: 'nonce', type: 'uint64' },
        ],
      };

    case 'usdTransfer':
    case 'withdraw':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'destination', type: 'address' },
          { name: 'amount', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
      };

    case 'spotOrder':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'orders', type: 'SpotOrder[]' },
          { name: 'grouping', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
        SpotOrder: [
          { name: 'a', type: 'uint8' },
          { name: 'b', type: 'bool' },
          { name: 'p', type: 'string' },
          { name: 's', type: 'string' },
          { name: 't', type: 'string' },
        ],
      };

    case 'spotCancel':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'cancels', type: 'SpotCancel[]' },
          { name: 'nonce', type: 'uint64' },
        ],
        SpotCancel: [
          { name: 'a', type: 'uint8' },
          { name: 'o', type: 'uint64' },
        ],
      };

    case 'spotCancelAll':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
      };

    case 'viewTransfer':
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'token', type: 'uint8' },
          { name: 'amount', type: 'string' },
          { name: 'toEvm', type: 'bool' },
          { name: 'nonce', type: 'uint64' },
        ],
      };

    default:
      // Generic action type
      return {
        Action: [
          { name: 'type', type: 'string' },
          { name: 'nonce', type: 'uint64' },
        ],
      };
  }
}

/**
 * Transform an order's type field from object to string for EIP-712 signing
 * The wire format uses { limit: { tif: 'Gtc' } } but EIP-712 expects 'gtc'
 */
function transformOrderType(t: unknown): string {
  if (typeof t === 'string') {
    return t.toLowerCase();
  }
  if (t && typeof t === 'object') {
    // Handle { limit: { tif: 'Gtc' } } format
    if ('limit' in t && typeof (t as Record<string, unknown>).limit === 'object') {
      const limit = (t as Record<string, { tif?: string }>).limit;
      return (limit?.tif || 'gtc').toLowerCase();
    }
    // Handle { trigger: ... } format
    if ('trigger' in t) {
      return 'trigger';
    }
  }
  return 'gtc';
}

/**
 * Transform order wire format to EIP-712 message format
 */
function transformOrderForSigning(order: Record<string, unknown>): Record<string, unknown> {
  return {
    a: order.a,
    b: order.b,
    p: order.p,
    s: order.s,
    r: order.r ?? false,
    t: transformOrderType(order.t),
  };
}

/**
 * Transform spot order wire format to EIP-712 message format
 */
function transformSpotOrderForSigning(order: Record<string, unknown>): Record<string, unknown> {
  return {
    a: order.a,
    b: order.b,
    p: order.p,
    s: order.s,
    t: transformOrderType(order.t),
  };
}

/**
 * Get the message object for signing
 * Transforms the action to match EIP-712 type definitions
 */
function getMessageForAction(
  action: Record<string, unknown>,
  nonce: bigint
): Record<string, unknown> {
  const actionType = action.type as string;

  // Transform perp orders
  if (actionType === 'order' && Array.isArray(action.orders)) {
    return {
      type: actionType,
      orders: (action.orders as Array<Record<string, unknown>>).map(transformOrderForSigning),
      grouping: action.grouping || 'na',
      nonce,
    };
  }

  // Transform spot orders
  if (actionType === 'spotOrder' && Array.isArray(action.orders)) {
    return {
      type: actionType,
      orders: (action.orders as Array<Record<string, unknown>>).map(transformSpotOrderForSigning),
      grouping: action.grouping || 'na',
      nonce,
    };
  }

  // For other actions, just add the nonce
  return {
    ...action,
    nonce,
  };
}

/**
 * Parse a signature string into r, s, v components
 */
function parseSignature(signature: `0x${string}`): { r: string; s: string; v: number } {
  // Remove '0x' prefix and ensure we have 130 hex chars (65 bytes)
  const sig = signature.slice(2);

  if (sig.length !== 130) {
    throw new Error(`Invalid signature length: ${sig.length}`);
  }

  const r = `0x${sig.slice(0, 64)}`;
  const s = `0x${sig.slice(64, 128)}`;
  const v = parseInt(sig.slice(128, 130), 16);

  return { r, s, v };
}

/**
 * Sign an action for submission to the exchange
 *
 * This function creates a proper EIP-712 signature that can be
 * cryptographically verified by the gateway.
 *
 * @param action - The action to sign (e.g., order, cancel, viewTransfer)
 * @param privateKey - The private key to sign with
 * @returns Signature components, nonce, and signer address
 */
export async function signAction(
  action: Record<string, unknown>,
  privateKey: `0x${string}`
): Promise<SignedAction> {
  const account = privateKeyToAccount(privateKey);

  // Use timestamp-based nonce (milliseconds) for HyperCore API
  const nonce = Date.now();

  // Get the action type
  const actionType = action.type as string;

  // Get EIP-712 types for this action
  const types = getTypesForAction(actionType);

  // Get the message formatted for EIP-712 signing
  const message = getMessageForAction(action, BigInt(nonce));

  // Sign using EIP-712 typed data
  const signature = await signTypedData({
    privateKey,
    domain: EIP712_DOMAIN,
    types,
    primaryType: 'Action',
    message,
  });

  // Parse signature into r, s, v components
  const { r, s, v } = parseSignature(signature);

  return {
    signature: { r, s, v },
    nonce,
    address: account.address,
  };
}
